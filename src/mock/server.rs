use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, ensure};
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header},
    response::Response,
};
use futures_util::stream;
use serde_json::{Value, json};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{sleep, timeout},
};

use super::{
    anthropic, config, gemini,
    model::{
        ApiError, Config, Frame, Protocol, ProviderProfile, RequestContext, Scenario, WireResponse,
    },
    openai, providers, session,
};

#[derive(Debug, Clone)]
pub struct Options {
    pub provider: String,
    pub models: Vec<String>,
    pub api_key: Option<String>,
    pub suite: session::Suite,
    pub seed: u64,
    pub delay_ms: u64,
    pub chunk_delay_ms: u64,
    pub chunk_bytes: usize,
    pub max_body_bytes: usize,
    pub max_sessions: usize,
    pub session_ttl_secs: u64,
    pub max_inflight: usize,
    pub request_timeout_ms: u64,
    pub base_path: String,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            provider: "openai".into(),
            models: vec![],
            api_key: Some("mock-key".into()),
            suite: session::Suite::Normal,
            seed: 0,
            delay_ms: 0,
            chunk_delay_ms: 0,
            chunk_bytes: 0,
            max_body_bytes: 2 * 1024 * 1024,
            max_sessions: 1024,
            session_ttl_secs: 1800,
            max_inflight: 64,
            request_timeout_ms: 10_000,
            base_path: String::new(),
        }
    }
}

struct App {
    config: Config,
    provider: ProviderProfile,
    options: Options,
    sessions: session::Engine,
    inflight: Arc<Semaphore>,
    requests: AtomicU64,
}

pub fn router(config: Config, mut options: Options) -> Result<Router> {
    config::validate(&config)?;
    ensure!(
        options.delay_ms <= 60_000 && options.chunk_delay_ms <= 60_000,
        "mock delays must be at most 60000 ms"
    );
    ensure!(
        options.chunk_bytes <= 1024 * 1024,
        "chunk bytes must be at most 1 MiB"
    );
    ensure!(
        (1..=64 * 1024 * 1024).contains(&options.max_body_bytes),
        "body limit must be between 1 byte and 64 MiB"
    );
    ensure!(
        (1..=100_000).contains(&options.max_sessions),
        "max sessions must be between 1 and 100000"
    );
    ensure!(
        (1..=4096).contains(&options.max_inflight),
        "max inflight must be between 1 and 4096"
    );
    ensure!(
        options.request_timeout_ms > 0,
        "request timeout must be positive"
    );
    ensure!(options.session_ttl_secs > 0, "session TTL must be positive");
    ensure!(
        options
            .api_key
            .as_ref()
            .is_none_or(|key| !key.is_empty() && key.bytes().all(|byte| byte.is_ascii_graphic())),
        "mock API key must contain visible ASCII characters without whitespace"
    );
    options.base_path = options.base_path.trim_end_matches('/').to_owned();
    ensure!(
        options.base_path.is_empty()
            || (options.base_path.starts_with('/')
                && !options.base_path.contains(['?', '#', '%'])
                && !options
                    .base_path
                    .split('/')
                    .any(|part| matches!(part, "." | ".."))
                && !options
                    .base_path
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control())),
        "base path must be an absolute URL path without query, fragment or dot segments"
    );
    if let Some(script) = &config.script {
        ensure!(
            script
                .steps
                .iter()
                .all(|step| step.delay_ms.is_none_or(|n| n <= 60_000)
                    && step.chunk_delay_ms.is_none_or(|n| n <= 60_000)),
            "script delays must be at most 60000 ms"
        );
    }
    let mut provider = config::provider(&config, &options.provider)?.clone();
    options.provider = provider.id.clone();
    for id in &options.models {
        ensure!(
            provider.models.iter().any(|model| &model.id == id),
            "unknown model {id} for provider {}",
            provider.id
        );
    }
    if !options.models.is_empty() {
        provider
            .models
            .retain(|model| options.models.contains(&model.id));
    }
    ensure!(
        !provider.models.is_empty(),
        "provider {} has no configured model profiles",
        provider.id
    );
    let sessions = session::Engine::new(
        options.seed,
        options.suite,
        config.script.clone(),
        options.max_sessions,
        Duration::from_secs(options.session_ttl_secs),
    );
    let app = Arc::new(App {
        config,
        provider,
        inflight: Arc::new(Semaphore::new(options.max_inflight)),
        options,
        sessions,
        requests: AtomicU64::new(0),
    });
    Ok(Router::new().fallback(handle).with_state(app))
}

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Health,
    Admin,
    Scenarios,
    Profiles,
    Session(String),
    Models(Protocol),
    Generate {
        protocol: Protocol,
        model: Option<String>,
        stream: bool,
    },
    Unsupported(Protocol),
    Missing,
}

fn route(path: &str, base: &str, provider: &str) -> Result<Route, ApiError> {
    let Some(path) = path
        .strip_prefix(base)
        .filter(|path| base.is_empty() || path.is_empty() || path.starts_with('/'))
    else {
        return Ok(Route::Missing);
    };
    let path = match provider {
        "cohere" => path
            .strip_prefix("/compatibility")
            .filter(|p| p.starts_with("/v1/"))
            .unwrap_or(path),
        "alibaba" => path
            .strip_prefix("/compatible-mode")
            .filter(|p| p.starts_with("/v1/"))
            .unwrap_or(path),
        "zhipuai" if path.starts_with("/api/paas/v4/") => {
            return route(&format!("/v1/{}", &path[13..]), "", "zhipuai");
        }
        _ => path,
    };
    Ok(match path {
        "/health" => Route::Health,
        "/__mock" => Route::Admin,
        "/__mock/scenarios" => Route::Scenarios,
        "/__mock/models" => Route::Profiles,
        "/v1/models" => Route::Models(if provider == "anthropic" {
            Protocol::AnthropicMessages
        } else {
            Protocol::OpenaiChat
        }),
        "/v1beta/models" => Route::Models(Protocol::GeminiGenerate),
        "/v1/chat/completions" => Route::Generate {
            protocol: Protocol::OpenaiChat,
            model: None,
            stream: false,
        },
        "/v1/responses" => Route::Generate {
            protocol: Protocol::OpenaiResponses,
            model: None,
            stream: false,
        },
        "/v1/messages" => Route::Generate {
            protocol: Protocol::AnthropicMessages,
            model: None,
            stream: false,
        },
        _ if path.starts_with("/__mock/sessions/") => Route::Session(decode_segment(&path[17..])?),
        _ if path.starts_with("/v1beta/models/") => {
            let resource = &path[15..];
            if let Some(id) = resource.strip_suffix(":streamGenerateContent") {
                Route::Generate {
                    protocol: Protocol::GeminiGenerate,
                    model: Some(decode_segment(id)?),
                    stream: true,
                }
            } else if let Some(id) = resource.strip_suffix(":generateContent") {
                Route::Generate {
                    protocol: Protocol::GeminiGenerate,
                    model: Some(decode_segment(id)?),
                    stream: false,
                }
            } else {
                Route::Unsupported(Protocol::GeminiGenerate)
            }
        }
        _ if [
            "/v1/embeddings",
            "/v1/audio",
            "/v1/images",
            "/v1/files",
            "/v1/batches",
            "/v1/fine_tuning",
            "/v1/assistants",
            "/v1/threads",
            "/v1/realtime",
            "/v1/moderations",
            "/v1/completions",
            "/v1/responses",
            "/v1/chat/completions",
            "/v1/models",
            "/v1/messages",
            "/v1beta/cachedContents",
            "/v1beta/files",
        ]
        .iter()
        .any(|prefix| {
            path == *prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|tail| tail.starts_with('/'))
        }) =>
        {
            Route::Unsupported(if path.starts_with("/v1beta/") {
                Protocol::GeminiGenerate
            } else if path.starts_with("/v1/messages") {
                Protocol::AnthropicMessages
            } else if path.starts_with("/v1/responses") {
                Protocol::OpenaiResponses
            } else if path.starts_with("/v1/chat/") {
                Protocol::OpenaiChat
            } else {
                default_protocol(provider)
            })
        }
        _ => Route::Missing,
    })
}

fn decode_segment(value: &str) -> Result<String, ApiError> {
    let mut output = Vec::with_capacity(value.len());
    let mut bytes = value.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().and_then(|b| (b as char).to_digit(16));
            let low = bytes.next().and_then(|b| (b as char).to_digit(16));
            match (high, low) {
                (Some(high), Some(low)) => output.push((high * 16 + low) as u8),
                _ => {
                    return Err(ApiError::invalid(
                        "path",
                        "Invalid percent encoding in path",
                    ));
                }
            }
        } else {
            output.push(byte);
        }
    }
    String::from_utf8(output)
        .map_err(|_| ApiError::invalid("path", "Path must contain valid UTF-8"))
}

fn default_protocol(provider: &str) -> Protocol {
    match provider {
        "anthropic" => Protocol::AnthropicMessages,
        "google" => Protocol::GeminiGenerate,
        _ => Protocol::OpenaiChat,
    }
}

struct Meta {
    session: String,
    request_id: String,
    scenario: Scenario,
    step: u64,
    protocol: Protocol,
    allow: String,
}

async fn handle(State(app): State<Arc<App>>, request: Request) -> Response {
    let mut meta = Meta {
        session: "default".into(),
        request_id: format!(
            "req_mock_{:016x}_{:016x}",
            app.options.seed,
            app.requests.fetch_add(1, Ordering::Relaxed)
        ),
        scenario: Scenario::Text,
        step: 0,
        protocol: default_protocol(&app.provider.id),
        allow: String::new(),
    };
    match dispatch(&app, request, &mut meta).await {
        Ok(response) => response,
        Err(error) => respond_error(&app, &meta, &error),
    }
}

async fn dispatch(app: &Arc<App>, request: Request, meta: &mut Meta) -> Result<Response, ApiError> {
    let (parts, body) = request.into_parts();
    let route = route(parts.uri.path(), &app.options.base_path, &app.provider.id)?;
    meta.protocol = match route {
        Route::Models(protocol)
        | Route::Generate { protocol, .. }
        | Route::Unsupported(protocol) => protocol,
        _ => meta.protocol,
    };
    meta.allow = match &route {
        Route::Generate { .. } => "POST",
        Route::Session(_) => "GET, DELETE",
        Route::Unsupported(_) | Route::Missing => "",
        _ => "GET",
    }
    .into();
    if let Some(session) = header_text(&parts.headers, "x-mock-session")? {
        if session.is_empty()
            || session.len() > 128
            || !session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        {
            return Err(ApiError::invalid(
                "x-mock-session",
                "Session IDs must contain 1 to 128 ASCII letters, digits, dots, hyphens or underscores",
            ));
        }
        meta.session = session.to_owned();
    }
    let override_scenario = header_text(&parts.headers, "x-mock-scenario")?
        .map(|value| {
            serde_json::from_value::<Scenario>(json!(value))
                .map_err(|_| ApiError::invalid("x-mock-scenario", "Unknown mock scenario"))
        })
        .transpose()?;
    if let Some(scenario) = override_scenario {
        meta.scenario = scenario;
    }
    if route != Route::Health
        && !authenticated(
            &app.options.api_key,
            &app.provider.id,
            meta.protocol,
            &parts.headers,
            &parts.uri,
        )
    {
        return Err(ApiError::new(
            401,
            "authentication_error",
            "A valid configured mock API key is required",
        ));
    }
    let method = &parts.method;
    match route {
        Route::Health => {
            require_method(method, &[Method::GET])?;
            return Ok(json_response(
                meta,
                json!({"status":"ok", "simulated":true, "provider":app.provider.id}),
            ));
        }
        Route::Admin | Route::Scenarios | Route::Profiles => {
            require_method(method, &[Method::GET])?;
            let value = match route {
                Route::Admin => {
                    let mut report = app.sessions.report(None);
                    report["simulated"] = json!(true);
                    report["provider"] = json!(app.provider.id);
                    report
                }
                Route::Scenarios => session::plan(),
                _ => {
                    json!({"simulated":true, "provider":app.provider, "sources":app.config.sources})
                }
            };
            return Ok(json_response(meta, value));
        }
        Route::Session(id) => {
            require_method(method, &[Method::GET, Method::DELETE])?;
            let value = if method == Method::DELETE {
                app.sessions.reset(&id)?;
                json!({"id":id,"reset":true,"simulated":true})
            } else {
                app.sessions.report(Some(&id))
            };
            return Ok(json_response(meta, value));
        }
        Route::Models(protocol) => {
            require_method(method, &[Method::GET])?;
            if !app.provider.protocols.contains(&protocol)
                && !(protocol == Protocol::OpenaiChat
                    && app.provider.protocols.contains(&Protocol::OpenaiResponses))
            {
                return Err(ApiError::unsupported(
                    "This provider profile does not expose this model listing protocol",
                ));
            }
            return Ok(json_response(meta, model_list(&app.provider, protocol)));
        }
        Route::Unsupported(_) => {
            return Err(ApiError::unsupported(
                "This native API is outside the implemented mock contract",
            ));
        }
        Route::Missing => return Err(ApiError::new(404, "not_found_error", "Unknown mock route")),
        Route::Generate { .. } => require_method(method, &[Method::POST])?,
    }
    let Route::Generate {
        protocol,
        model,
        stream: path_stream,
    } = route
    else {
        unreachable!()
    };
    if !app.provider.protocols.contains(&protocol) {
        return Err(ApiError::unsupported(
            "This provider profile does not expose the requested protocol",
        ));
    }
    let permit = app.inflight.clone().try_acquire_owned().map_err(|_| {
        ApiError::new(
            503,
            "mock_busy",
            "Mock server has reached its inflight request limit",
        )
    })?;
    let content_type = header_text(&parts.headers, "content-type")?
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim();
    if !content_type.eq_ignore_ascii_case("application/json") {
        return Err(ApiError::new(
            415,
            "invalid_request_error",
            "Content-Type must be application/json",
        ));
    }
    let bytes = timeout(
        Duration::from_millis(app.options.request_timeout_ms),
        to_bytes(body, app.options.max_body_bytes),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            408,
            "request_timeout",
            "Request body exceeded the configured read timeout",
        )
    })?
    .map_err(|error| {
        use std::error::Error;
        let exceeded = error
            .source()
            .is_some_and(|source| source.to_string().contains("length limit"));
        ApiError::new(
            if exceeded { 413 } else { 400 },
            "invalid_request_error",
            if exceeded {
                "Request body exceeds the configured byte limit"
            } else {
                "Could not read request body"
            },
        )
    })?;
    let body: Value = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::invalid("body", "Request body must be valid JSON"))?;
    if !body.is_object() {
        return Err(ApiError::invalid(
            "body",
            "Request body must be a JSON object",
        ));
    }
    if body
        .get("stream")
        .is_some_and(|stream| !stream.is_boolean())
    {
        return Err(ApiError::invalid("stream", "stream must be a boolean"));
    }
    let stream = if protocol == Protocol::GeminiGenerate {
        path_stream
    } else {
        body.get("stream").and_then(Value::as_bool).unwrap_or(false)
    };
    if protocol == Protocol::GeminiGenerate
        && stream
        && query_value(&parts.uri, "alt").as_deref() != Some("sse")
    {
        return Err(ApiError::unsupported(
            "streamGenerateContent requires alt=sse in this mock",
        ));
    }
    let model_id = model
        .as_deref()
        .or_else(|| body.get("model").and_then(Value::as_str))
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ApiError::invalid("model", "A nonempty model ID is required"))?;
    let profile = app
        .provider
        .models
        .iter()
        .find(|profile| profile.id == model_id)
        .ok_or_else(|| {
            ApiError::new(
                404,
                "model_not_found",
                "The requested model is not configured for this mock provider",
            )
        })?;
    if !profile.protocols.contains(&protocol) {
        return Err(ApiError::unsupported(
            "This model profile does not expose the requested protocol",
        ));
    }
    let protocol_key = serde_json::to_value(protocol).expect("protocol enum");
    let key = format!(
        "{}/{}/{}",
        app.provider.id,
        protocol_key.as_str().unwrap(),
        profile.id
    );
    let mut turn = app
        .sessions
        .begin(&meta.session, &key, &body, stream, override_scenario)?;
    meta.request_id = turn.request_id.clone();
    meta.scenario = turn.fixture.scenario;
    meta.step = turn.number;
    let mut state = turn.state.clone();
    let ctx = RequestContext {
        provider: &app.provider.id,
        protocol,
        model: profile,
        fixture: &turn.fixture,
        request_id: &turn.request_id,
        session_id: &meta.session,
        turn: turn.number,
        seed: app.options.seed,
        headers: &parts.headers,
        stream,
    };
    let result = validate(&ctx, &body, &state)
        .and_then(|_| providers::validate(&ctx, &body, &state))
        .and_then(|_| input_limit(&ctx, &body));
    if let Err(error) = result {
        turn.reject(&error);
        return Err(error);
    }
    if !stream
        && matches!(
            ctx.fixture.scenario,
            Scenario::Disconnect | Scenario::UnknownEvent | Scenario::StreamError
        )
    {
        let error = ApiError::unsupported("This mock scenario requires a streaming request");
        turn.reject(&error);
        return Err(error);
    }
    let delay = ctx
        .fixture
        .delay_ms
        .unwrap_or(if ctx.fixture.scenario == Scenario::Slow {
            app.options.delay_ms.max(250)
        } else {
            app.options.delay_ms
        });
    let chunk_delay = ctx
        .fixture
        .chunk_delay_ms
        .unwrap_or(app.options.chunk_delay_ms);
    let response = if let Some(error) = scenario_error(ctx.fixture.scenario, protocol) {
        error_wire(app, meta, &error)
    } else {
        match render(&ctx, &body, &mut state).and_then(|mut response| {
            providers::transform(&ctx, &body, &mut response)?;
            Ok(response)
        }) {
            Ok(response) => response,
            Err(error) => {
                turn.reject(&error);
                return Err(error);
            }
        }
    };
    if delay > 0 {
        sleep(Duration::from_millis(delay)).await;
    }
    turn.accept(state, response.status)?;
    Ok(wire_response(
        meta,
        response,
        Some(StreamGuards {
            _turn: turn,
            _permit: permit,
        }),
        app.options.chunk_bytes,
        chunk_delay,
        meta.scenario == Scenario::Disconnect,
    ))
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, ApiError> {
    headers
        .get(name)
        .map(|value| {
            value
                .to_str()
                .map_err(|_| ApiError::invalid(name, "Header must contain ASCII text"))
        })
        .transpose()
}

fn query_value(uri: &Uri, key: &str) -> Option<String> {
    let url = reqwest::Url::parse(&format!(
        "http://localhost/?{}",
        uri.query().unwrap_or_default()
    ))
    .ok()?;
    url.query_pairs()
        .find_map(|(name, value)| (name == key).then(|| value.into_owned()))
}

fn authenticated(
    key: &Option<String>,
    provider: &str,
    protocol: Protocol,
    headers: &HeaderMap,
    uri: &Uri,
) -> bool {
    let Some(key) = key else {
        return true;
    };
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .is_some_and(|(scheme, value)| scheme.eq_ignore_ascii_case("bearer") && value == key);
    let header_matches =
        |name| headers.get(name).and_then(|value| value.to_str().ok()) == Some(key.as_str());
    if protocol == Protocol::GeminiGenerate || provider == "google" {
        header_matches("x-goog-api-key") || query_value(uri, "key").as_deref() == Some(key.as_str())
    } else if protocol == Protocol::AnthropicMessages || provider == "anthropic" {
        bearer || header_matches("x-api-key")
    } else {
        bearer
    }
}

fn require_method(method: &Method, methods: &[Method]) -> Result<(), ApiError> {
    if methods.contains(method) {
        Ok(())
    } else {
        Err(ApiError::new(
            405,
            "method_not_allowed",
            "Method is not allowed for this mock route",
        ))
    }
}

fn input_limit(ctx: &RequestContext<'_>, body: &Value) -> Result<(), ApiError> {
    if let Some(input) = header_text(ctx.headers, "x-mock-input-tokens")? {
        if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ApiError::invalid(
                "x-mock-input-tokens",
                "Simulated input tokens must be an unsigned integer",
            ));
        }
        let input = input.parse::<u64>().map_err(|_| {
            ApiError::invalid(
                "x-mock-input-tokens",
                "Simulated input tokens must be an unsigned integer",
            )
        })?;
        let output = ["max_tokens", "max_completion_tokens", "max_output_tokens"]
            .iter()
            .filter_map(|key| body.get(key).and_then(Value::as_u64))
            .max()
            .or_else(|| {
                [
                    "/generationConfig/maxOutputTokens",
                    "/generationConfig/max_output_tokens",
                    "/generation_config/maxOutputTokens",
                    "/generation_config/max_output_tokens",
                ]
                .iter()
                .find_map(|path| body.pointer(path).and_then(Value::as_u64))
            })
            .unwrap_or(0);
        if ctx
            .model
            .context_window
            .is_some_and(|limit| input.saturating_add(output) > limit)
        {
            return Err(ApiError::new(
                400,
                "context_length_exceeded",
                "Simulated input and requested output exceed the configured context window",
            ));
        }
    }
    Ok(())
}

fn validate(ctx: &RequestContext<'_>, body: &Value, state: &Value) -> Result<(), ApiError> {
    match ctx.protocol {
        Protocol::OpenaiChat | Protocol::OpenaiResponses => openai::validate(ctx, body, state),
        Protocol::AnthropicMessages => anthropic::validate(ctx, body, state),
        Protocol::GeminiGenerate => gemini::validate(ctx, body, state),
    }
}

fn render(
    ctx: &RequestContext<'_>,
    body: &Value,
    state: &mut Value,
) -> Result<WireResponse, ApiError> {
    match ctx.protocol {
        Protocol::OpenaiChat | Protocol::OpenaiResponses => openai::render(ctx, body, state),
        Protocol::AnthropicMessages => anthropic::render(ctx, body, state),
        Protocol::GeminiGenerate => gemini::render(ctx, body, state),
    }
}

fn scenario_error(scenario: Scenario, protocol: Protocol) -> Option<ApiError> {
    let (status, code, message) = match scenario {
        Scenario::RateLimit => (429, "rate_limit_error", "Synthetic rate limit exceeded"),
        Scenario::Overloaded => (
            if protocol == Protocol::AnthropicMessages {
                529
            } else {
                503
            },
            "overloaded_error",
            "Synthetic provider overload",
        ),
        Scenario::ServerError => (500, "server_error", "Synthetic provider server error"),
        Scenario::Unauthorized => (
            401,
            "authentication_error",
            "Synthetic authentication failure",
        ),
        Scenario::Forbidden => (403, "permission_error", "Synthetic permission failure"),
        Scenario::NotFound => (404, "not_found_error", "Synthetic resource not found"),
        Scenario::ContextOverflow => (
            400,
            "context_length_exceeded",
            "Synthetic context window overflow",
        ),
        Scenario::CacheMiss => (
            404,
            "cache_not_found",
            "Synthetic cached resource not found",
        ),
        _ => return None,
    };
    Some(ApiError::new(status, code, message))
}

fn error_wire(app: &App, meta: &Meta, error: &ApiError) -> WireResponse {
    let mut response =
        if let Some(response) = providers::error(&app.provider.id, error, &meta.request_id) {
            response
        } else {
            match meta.protocol {
                Protocol::OpenaiChat | Protocol::OpenaiResponses => {
                    openai::error(error, &meta.request_id)
                }
                Protocol::AnthropicMessages => anthropic::error(error, &meta.request_id),
                Protocol::GeminiGenerate => gemini::error(error, &meta.request_id),
            }
        };
    if matches!(error.status, 429 | 503 | 529) {
        response.headers.insert("retry-after".into(), "1".into());
    }
    if error.status == 405 {
        response.headers.insert("allow".into(), meta.allow.clone());
    }
    response
}

fn respond_error(app: &App, meta: &Meta, error: &ApiError) -> Response {
    wire_response(meta, error_wire(app, meta, error), None, 0, 0, false)
}

fn json_response(meta: &Meta, value: Value) -> Response {
    wire_response(meta, WireResponse::json(value), None, 0, 0, false)
}

fn model_list(provider: &ProviderProfile, protocol: Protocol) -> Value {
    let models = provider.models.iter().filter(|model| {
        model.protocols.contains(&protocol)
            || (protocol == Protocol::OpenaiChat
                && model.protocols.contains(&Protocol::OpenaiResponses))
    });
    match protocol {
        Protocol::AnthropicMessages => {
            let data: Vec<_> = models.map(|model| json!({"id":model.id,"type":"model","display_name":model.id,"created_at":"2026-01-01T00:00:00Z"})).collect();
            json!({"first_id":data.first().map(|model| &model["id"]), "last_id":data.last().map(|model| &model["id"]), "data":data,"has_more":false})
        }
        Protocol::GeminiGenerate => json!({"models":models.map(|model| {
            let mut value = json!({"name":format!("models/{}",model.id),"baseModelId":model.id,"displayName":model.id,"supportedGenerationMethods":["generateContent"]});
            if let Some(limit) = model.context_window { value["inputTokenLimit"] = json!(limit); }
            if let Some(limit) = model.max_output_tokens { value["outputTokenLimit"] = json!(limit); }
            value
        }).collect::<Vec<_>>()}),
        _ => {
            json!({"object":"list","data":models.map(|model| json!({"id":model.id,"object":"model","created":0,"owned_by":provider.id})).collect::<Vec<_>>()})
        }
    }
}

struct StreamGuards {
    _turn: session::Turn,
    _permit: OwnedSemaphorePermit,
}

struct StreamState {
    frames: VecDeque<Bytes>,
    chunk_bytes: usize,
    delay: Duration,
    disconnect: bool,
    failed: bool,
    _guards: Option<StreamGuards>,
}

fn encode_frames(frames: Vec<Frame>, chunk_bytes: usize) -> VecDeque<Bytes> {
    let encoded = frames.into_iter().map(|frame| {
        let mut wire = String::new();
        if let Some(event) = frame.event {
            wire.push_str("event: ");
            wire.push_str(&event);
            wire.push('\n');
        }
        for line in frame.data.split('\n') {
            wire.push_str("data: ");
            wire.push_str(line);
            wire.push('\n');
        }
        wire.push('\n');
        wire.into_bytes()
    });
    if chunk_bytes == 0 {
        encoded.map(Bytes::from).collect()
    } else {
        std::iter::once(Bytes::from(encoded.flatten().collect::<Vec<_>>())).collect()
    }
}

fn disconnect_prefix(events: Vec<Frame>) -> Vec<Frame> {
    events
        .into_iter()
        .take_while(|frame| {
            if frame.data == "[DONE]"
                || matches!(
                    frame.event.as_deref(),
                    Some(
                        "message_stop"
                            | "response.completed"
                            | "response.incomplete"
                            | "response.failed"
                    )
                )
            {
                return false;
            }
            let Ok(value) = serde_json::from_str::<Value>(&frame.data) else {
                return true;
            };
            value
                .pointer("/delta/stop_reason")
                .is_none_or(Value::is_null)
                && !value
                    .get("choices")
                    .and_then(Value::as_array)
                    .is_some_and(|choices| {
                        choices.iter().any(|choice| {
                            choice
                                .get("finish_reason")
                                .is_some_and(|reason| !reason.is_null())
                        })
                    })
                && !value
                    .get("candidates")
                    .and_then(Value::as_array)
                    .is_some_and(|candidates| {
                        candidates.iter().any(|candidate| {
                            candidate
                                .get("finishReason")
                                .or_else(|| candidate.get("finish_reason"))
                                .is_some_and(|reason| !reason.is_null())
                        })
                    })
        })
        .take(2)
        .collect()
}

fn wire_response(
    meta: &Meta,
    mut wire: WireResponse,
    guards: Option<StreamGuards>,
    chunk_bytes: usize,
    delay_ms: u64,
    disconnect: bool,
) -> Response {
    let streaming = wire.events.is_some();
    let body = if let Some(events) = wire.events.take() {
        let events = if disconnect {
            disconnect_prefix(events)
        } else {
            events
        };
        let mut frames = encode_frames(events, chunk_bytes);
        if disconnect && frames.iter().all(Bytes::is_empty) {
            frames = std::iter::once(Bytes::from_static(b": simulated disconnect\n\n")).collect();
        }
        let state = StreamState {
            frames,
            chunk_bytes,
            delay: Duration::from_millis(delay_ms),
            disconnect,
            failed: false,
            _guards: guards,
        };
        Body::from_stream(stream::unfold(state, |mut state| async move {
            if state.failed {
                return None;
            }
            while state.frames.front().is_some_and(Bytes::is_empty) {
                state.frames.pop_front();
            }
            if state.frames.is_empty() {
                if state.disconnect {
                    // Let Hyper flush the prefix before aborting the HTTP body.
                    tokio::task::yield_now().await;
                    state.failed = true;
                    return Some((
                        Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "Synthetic mock stream disconnect",
                        )),
                        state,
                    ));
                }
                return None;
            }
            if !state.delay.is_zero() {
                sleep(state.delay).await;
            }
            let chunk = if state.chunk_bytes == 0 {
                state.frames.pop_front().unwrap()
            } else {
                let frame = state.frames.front_mut().unwrap();
                frame.split_to(frame.len().min(state.chunk_bytes))
            };
            Some((Ok(chunk), state))
        }))
    } else {
        Body::from(wire.body.to_string())
    };
    let mut response = Response::new(body);
    *response.status_mut() =
        StatusCode::from_u16(wire.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(if streaming {
            "text/event-stream"
        } else {
            "application/json"
        }),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    for (name, value) in wire.headers {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            headers.insert(name, value);
        }
    }
    for (name, value) in [
        ("x-mock-simulated", "true".to_owned()),
        ("x-mock-session", meta.session.clone()),
        ("x-mock-request-id", meta.request_id.clone()),
        ("x-mock-scenario", meta.scenario.name()),
        ("x-mock-step", meta.step.to_string()),
        (
            if meta.protocol == Protocol::AnthropicMessages {
                "request-id"
            } else {
                "x-request-id"
            },
            meta.request_id.clone(),
        ),
    ] {
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.insert(name, value);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_preserve_model_ids_and_enforce_prefix_boundaries() {
        assert_eq!(
            route("/mock/v1/chat/completions", "/mock", "openai").unwrap(),
            Route::Generate {
                protocol: Protocol::OpenaiChat,
                model: None,
                stream: false
            }
        );
        assert_eq!(
            route("/mockish/v1/chat/completions", "/mock", "openai").unwrap(),
            Route::Missing
        );
        assert_eq!(
            route(
                "/v1beta/models/Vendor%2FCase:v1:generateContent",
                "",
                "google"
            )
            .unwrap(),
            Route::Generate {
                protocol: Protocol::GeminiGenerate,
                model: Some("Vendor/Case:v1".into()),
                stream: false
            }
        );
        assert_eq!(
            route("/api/paas/v4/chat/completions", "", "zhipuai").unwrap(),
            Route::Generate {
                protocol: Protocol::OpenaiChat,
                model: None,
                stream: false
            }
        );
        assert_eq!(
            route("/compatibility/v1/chat/completions", "", "openai").unwrap(),
            Route::Missing
        );
        assert_eq!(
            route("/v1/models/claude-example", "", "anthropic").unwrap(),
            Route::Unsupported(Protocol::AnthropicMessages)
        );
    }

    #[test]
    fn authentication_uses_native_headers_and_exact_fake_token() {
        let key = Some("fake".to_owned());
        let uri = "/v1beta/models?key=fake".parse().unwrap();
        let mut headers = HeaderMap::new();
        assert!(authenticated(
            &key,
            "google",
            Protocol::GeminiGenerate,
            &headers,
            &uri
        ));
        assert!(!authenticated(
            &key,
            "openai",
            Protocol::OpenaiChat,
            &headers,
            &uri
        ));
        headers.insert("x-api-key", HeaderValue::from_static("fake"));
        assert!(authenticated(
            &key,
            "anthropic",
            Protocol::AnthropicMessages,
            &headers,
            &uri
        ));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer fake-extra"),
        );
        assert!(!authenticated(
            &key,
            "openai",
            Protocol::OpenaiChat,
            &headers,
            &uri
        ));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer fake"),
        );
        assert!(authenticated(
            &key,
            "openai",
            Protocol::OpenaiChat,
            &headers,
            &uri
        ));
    }

    #[test]
    fn fragmented_sse_preserves_unicode_and_event_boundaries() {
        let frames = vec![
            Frame::named("delta", json!({"text":"✓ 世界"})),
            Frame::done(),
        ];
        let expected = encode_frames(frames.clone(), 0)
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut bytes = encode_frames(frames, 1).pop_front().unwrap();
        let mut fragmented = Vec::new();
        while !bytes.is_empty() {
            fragmented.extend_from_slice(&bytes.split_to(1));
        }
        assert_eq!(fragmented, expected);
        assert_eq!(
            String::from_utf8(fragmented).unwrap(),
            "event: delta\ndata: {\"text\":\"✓ 世界\"}\n\ndata: [DONE]\n\n"
        );
    }

    #[test]
    fn disconnect_does_not_forward_short_or_empty_success_terminators() {
        let final_frame = Frame::data(json!({"candidates":[{"finishReason":"STOP"}]}));
        assert!(disconnect_prefix(vec![final_frame.clone()]).is_empty());
        let prefix = disconnect_prefix(vec![
            Frame::data(json!({"candidates":[{"content":{"parts":[{"text":"x"}]}}]})),
            final_frame,
        ]);
        assert_eq!(prefix.len(), 1);
        assert!(!prefix[0].data.contains("finishReason"));
        assert!(
            disconnect_prefix(vec![
                Frame::data(json!({"choices":[{"finish_reason":"stop"}]})),
                Frame::done()
            ])
            .is_empty()
        );
    }
}
