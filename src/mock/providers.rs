//! Provider differences layered over the shared wire protocols.
//! Evidence: research/2026-10-19/mock-server.md and data/mock/providers.yaml.
//! Rules apply to documented interfaces or exact model IDs; absent evidence is not rejection.

use serde_json::{Value, json};

use super::model::{ApiError, Frame, Protocol, RequestContext, Scenario, WireResponse};

pub fn validate(ctx: &RequestContext<'_>, body: &Value, _state: &Value) -> Result<(), ApiError> {
    if ctx.provider == "meta" {
        return meta(ctx, body);
    }
    if ctx.protocol == Protocol::OpenaiResponses {
        if ctx.provider == "xai" {
            if present(body, "/instructions") && present(body, "/previous_response_id") {
                return Err(ApiError::invalid(
                    "instructions",
                    "xAI cannot combine instructions with previous_response_id",
                ));
            }
            max_items(body, "/tools", 1000)?;
        }
        return Ok(());
    }
    if ctx.protocol != Protocol::OpenaiChat {
        return Ok(());
    }
    match ctx.provider {
        "deepseek" => deepseek(ctx, body),
        "moonshotai" => moonshot(ctx, body),
        "zhipuai" => zai(ctx, body),
        "minimax" => minimax(ctx, body),
        "mistral" => {
            enumeration(body, "/prompt_mode", &["reasoning"])?;
            enumeration(
                body,
                "/reasoning_effort",
                &["none", "minimal", "low", "medium", "high", "xhigh"],
            )?;
            enumeration(body, "/service_tier", &["auto", "standard_only"])?;
            boolean(body, "/safe_prompt")?;
            integer(body, "/random_seed", i64::MIN, i64::MAX)
        }
        "cohere" => {
            enumeration(body, "/reasoning_effort", &["none", "high"])?;
            reject_fields(
                body,
                &[
                    "store",
                    "metadata",
                    "logit_bias",
                    "top_logprobs",
                    "n",
                    "modalities",
                    "prediction",
                    "audio",
                    "service_tier",
                    "parallel_tool_calls",
                    "connectors",
                    "documents",
                    "citation_options",
                ],
            )
        }
        "alibaba" => {
            number(body, "/temperature", 0.0, 2.0, false, true)?;
            number(body, "/top_p", 0.0, 1.0, true, false)?;
            number(body, "/presence_penalty", -2.0, 2.0, false, false)?;
            integer(body, "/top_k", 0, i64::MAX)?;
            number(body, "/repetition_penalty", 0.0, f64::MAX, true, false)?;
            if body
                .pointer("/response_format/type")
                .and_then(Value::as_str)
                == Some("json_object")
                && !json_instruction(body)
            {
                return Err(ApiError::invalid(
                    "messages",
                    "JSON output requires a JSON instruction in a user or system message",
                ));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// https://dev.meta.ai/docs/models and /docs/reasoning establish Wonder's exact scope.
fn meta(ctx: &RequestContext<'_>, body: &Value) -> Result<(), ApiError> {
    let known = matches!(
        ctx.model.id.as_str(),
        "muse-wonder-1.0"
            | "muse-spark-1.3"
            | "muse-spark-1.3-contributor"
            | "muse-spark-1.2"
            | "muse-spark-1.2-contributor"
            | "muse-spark-1.1"
    );
    if !known
        || !matches!(
            ctx.protocol,
            Protocol::OpenaiChat | Protocol::OpenaiResponses
        )
    {
        return Ok(());
    }
    number(body, "/top_p", 0.0, 1.0, true, false)?;
    number(body, "/temperature", 0.0, 2.0, false, false)?;
    number(body, "/frequency_penalty", -2.0, 2.0, false, false)?;
    number(body, "/presence_penalty", -2.0, 2.0, false, false)?;
    length(body, "/safety_identifier", 64)?;
    enumeration(body, "/prompt_cache_retention", &["in_memory", "24h"])?;
    let effort_path = if ctx.protocol == Protocol::OpenaiResponses {
        "/reasoning/effort"
    } else {
        "/reasoning_effort"
    };
    let efforts: &[&str] = if matches!(ctx.model.id.as_str(), "muse-wonder-1.0" | "muse-spark-1.3")
    {
        &["minimal", "low", "medium", "high", "xhigh", "max"]
    } else {
        &["minimal", "low", "medium", "high", "xhigh"]
    };
    enumeration(body, effort_path, efforts)?;
    if ctx.protocol == Protocol::OpenaiChat {
        reject_fields(
            body,
            &[
                "stop",
                "verbosity",
                "logit_bias",
                "prediction",
                "web_search_options",
                "modalities",
                "audio",
                "max_output_tokens",
            ],
        )?;
        if present(body, "/text/format") {
            return Err(ApiError::invalid(
                "text.format",
                "Use response_format on Chat Completions",
            ));
        }
        integer(body, "/n", 1, 1)?;
        if body.get("logprobs").and_then(Value::as_bool) == Some(true) {
            return Err(ApiError::invalid(
                "logprobs",
                "Meta reasoning models do not expose log probabilities",
            ));
        }
        if present(body, "/top_logprobs") {
            return Err(ApiError::invalid(
                "top_logprobs",
                "top_logprobs requires unsupported logprobs=true",
            ));
        }
    } else {
        // https://dev.meta.ai/docs/protocols/responses#compacting
        enumeration(body, "/truncation", &["disabled"])?;
        enumeration(body, "/reasoning/summary", &["auto", "concise", "detailed"])?;
        integer(body, "/max_output_tokens", 16, i64::MAX)?;
        if body.get("background").and_then(Value::as_bool) == Some(true) && ctx.stream {
            return Err(ApiError::invalid(
                "background",
                "Meta background creation cannot stream",
            ));
        }
        if included(body, "message.output_text.logprobs") {
            return Err(ApiError::invalid(
                "include",
                "Meta reasoning models do not expose log probabilities",
            ));
        }
        if included(body, "reasoning.encrypted_content") && present(body, "/previous_response_id") {
            return Err(ApiError::invalid(
                "include",
                "Choose encrypted reasoning replay or previous_response_id",
            ));
        }
        if let Some(input) = body.get("input").and_then(Value::as_array) {
            let mut pending_reasoning = false;
            for item in input {
                match item.get("type").and_then(Value::as_str) {
                    Some("reasoning") => {
                        if !item.get("summary").is_some_and(Value::is_array)
                            || !item
                                .get("encrypted_content")
                                .and_then(Value::as_str)
                                .is_some_and(|s| !s.is_empty())
                        {
                            return Err(ApiError::invalid(
                                "input",
                                "Replayed reasoning requires summary and encrypted_content",
                            ));
                        }
                        pending_reasoning = true;
                    }
                    Some("function_call") => pending_reasoning = false,
                    _ if item.get("role").and_then(Value::as_str) == Some("assistant") => {
                        pending_reasoning = false
                    }
                    _ if pending_reasoning
                        && !present(body, "/previous_response_id")
                        && matches!(
                            item.get("role").and_then(Value::as_str),
                            Some("user" | "system" | "developer")
                        ) =>
                    {
                        return Err(ApiError::invalid(
                            "input",
                            "Reasoning replay must precede an assistant message or function call",
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn deepseek(ctx: &RequestContext<'_>, body: &Value) -> Result<(), ApiError> {
    if !matches!(ctx.model.id.as_str(), "deepseek-flash" | "deepseek-v4-pro") {
        return Ok(());
    }
    number(body, "/temperature", 0.0, 2.0, false, false)?;
    number(body, "/top_p", 0.0, 1.0, true, false)?;
    enumeration(body, "/thinking/type", &["enabled", "disabled"])?;
    enumeration(
        body,
        "/reasoning_effort",
        &[
            "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
        ],
    )?;
    enumeration(body, "/response_format/type", &["text", "json_object"])?;
    integer(body, "/max_tokens", 1, 393216)?;
    max_items(body, "/stop", 16)?;
    function_names(body, 128)?;
    integer(body, "/top_logprobs", 0, 20)?;
    if present(body, "/top_logprobs") && body.get("logprobs").and_then(Value::as_bool) != Some(true)
    {
        return Err(ApiError::invalid(
            "top_logprobs",
            "top_logprobs requires logprobs=true",
        ));
    }
    if present(body, "/stream_options") && !ctx.stream {
        return Err(ApiError::invalid(
            "stream_options",
            "DeepSeek stream_options requires stream=true",
        ));
    }
    // Explicit contradictory switches have no documented precedence, so leave their mode unknown.
    let switch = body
        .pointer("/thinking/type")
        .and_then(Value::as_str)
        .map(|s| s == "enabled");
    let effort = body
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(|s| s != "none");
    let thinking = match (switch, effort) {
        (Some(a), Some(b)) if a != b => None,
        (Some(a), _) | (_, Some(a)) => Some(a),
        _ => Some(true),
    };
    if thinking == Some(true) {
        if body
            .get("tool_choice")
            .is_some_and(|v| v == "required" || v.is_object())
        {
            return Err(ApiError::invalid(
                "tool_choice",
                "DeepSeek thinking mode does not allow forced tool choices",
            ));
        }
        if present(body, "/tools") {
            for (index, message) in messages(body).enumerate() {
                if message.get("role").and_then(Value::as_str) == Some("assistant")
                    && !message
                        .get("reasoning_content")
                        .is_some_and(Value::is_string)
                {
                    return Err(ApiError::invalid(
                        &format!("messages.{index}.reasoning_content"),
                        "DeepSeek thinking with tools requires historical reasoning_content",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn moonshot(ctx: &RequestContext<'_>, body: &Value) -> Result<(), ApiError> {
    match ctx.model.id.as_str() {
        "kimi-k3" => enumeration(body, "/reasoning_effort", &["low", "high", "max"])?,
        "kimi-k2.7-code" | "kimi-k2.7-code-highspeed" => {
            enumeration(body, "/thinking/type", &["enabled"])?;
            if body.pointer("/thinking/keep").is_some()
                && body.pointer("/thinking/keep").and_then(Value::as_str) != Some("all")
            {
                return Err(ApiError::invalid(
                    "thinking.keep",
                    "Kimi K2.7 Code requires thinking.keep=all",
                ));
            }
        }
        "kimi-k2.6" => {
            enumeration(body, "/thinking/type", &["enabled", "disabled"])?;
            enumeration(body, "/thinking/keep", &["all"])?;
        }
        _ => return Ok(()),
    }
    enumeration(body, "/prompt_cache_options/mode", &["implicit"])?;
    enumeration(body, "/prompt_cache_options/ttl", &["5m", "1h"])?;
    for message in messages(body) {
        if let Some(parts) = message.get("content").and_then(Value::as_array)
            && parts.iter().any(|part| {
                part.get("prompt_cache_breakpoint").is_some()
                    || part.get("type").and_then(Value::as_str) == Some("prompt_cache_breakpoint")
            })
        {
            return Err(ApiError::invalid(
                "messages",
                "Kimi does not support explicit cache breakpoints",
            ));
        }
    }
    Ok(())
}

fn zai(ctx: &RequestContext<'_>, body: &Value) -> Result<(), ApiError> {
    if !matches!(
        ctx.model.id.as_str(),
        "glm-5.3" | "glm-5.3-flash" | "glm-5.2" | "glm-5.1" | "glm-5" | "glm-4.7" | "glm-4.6"
    ) {
        return Ok(());
    }
    number(body, "/temperature", 0.0, 1.0, false, false)?;
    number(body, "/top_p", 0.01, 1.0, false, false)?;
    integer(body, "/max_tokens", 1, 131072)?;
    enumeration(body, "/tool_choice", &["auto"])?;
    enumeration(body, "/response_format/type", &["text", "json_object"])?;
    enumeration(body, "/thinking/type", &["enabled", "disabled"])?;
    boolean(body, "/thinking/clear_thinking")?;
    boolean(body, "/do_sample")?;
    boolean(body, "/tool_stream")?;
    max_items(body, "/tools", 128)?;
    function_names(body, 64)?;
    if messages(body).all(|message| {
        matches!(
            message.get("role").and_then(Value::as_str),
            Some("system" | "assistant")
        )
    }) {
        return Err(ApiError::invalid(
            "messages",
            "Z.ai requires input beyond system and assistant messages",
        ));
    }
    if matches!(ctx.model.id.as_str(), "glm-5.3" | "glm-5.3-flash") {
        enumeration(body, "/thinking/type", &["enabled"])?;
        enumeration(body, "/reasoning_effort", &["low", "high", "max"])?;
    } else if ctx.model.id == "glm-5.2" {
        enumeration(
            body,
            "/reasoning_effort",
            &["none", "minimal", "low", "medium", "high", "xhigh", "max"],
        )?;
    }
    // The source contradicts itself on stop count (one versus four); do not enforce either.
    Ok(())
}

fn minimax(ctx: &RequestContext<'_>, body: &Value) -> Result<(), ApiError> {
    number(body, "/temperature", 0.0, 2.0, false, false)?;
    number(body, "/top_p", 0.0, 1.0, false, false)?;
    integer(body, "/n", 1, 1)?;
    reject_fields(body, &["function_call"])?;
    boolean(body, "/reasoning_split")?;
    enumeration(body, "/service_tier", &["standard", "priority"])?;
    if ctx.model.id == "MiniMax-M3.1-Flash-Preview" {
        if body.pointer("/thinking/type").and_then(Value::as_str) == Some("disabled")
            || body.get("reasoning_effort").and_then(Value::as_str) == Some("none")
        {
            return Err(ApiError::new(
                400,
                "2013",
                "MiniMax-M3.1-Flash-Preview requires adaptive thinking",
            ));
        }
        enumeration(body, "/thinking/type", &["adaptive"])?;
        enumeration(
            body,
            "/reasoning_effort",
            &["low", "medium", "high", "xhigh", "max"],
        )?;
        if body.get("reasoning_split").and_then(Value::as_bool) == Some(false) {
            return Err(ApiError::invalid(
                "reasoning_split",
                "MiniMax-M3.1-Flash-Preview does not support reasoning_split=false",
            ));
        }
    } else if ctx.model.id == "MiniMax-M3" {
        enumeration(body, "/thinking/type", &["adaptive", "disabled"])?;
    }
    // Penalties are ignored, and M2 thinking:disabled is accepted; neither is a rejection.
    Ok(())
}

pub fn transform(
    ctx: &RequestContext<'_>,
    body: &Value,
    response: &mut WireResponse,
) -> Result<(), ApiError> {
    if ctx.provider == "meta" && ctx.protocol == Protocol::OpenaiResponses {
        let summary = present(body, "/reasoning/summary");
        let encrypted = included(body, "reasoning.encrypted_content");
        scrub_meta_reasoning(&mut response.body, summary, encrypted);
        if let Some(events) = &mut response.events {
            events.retain_mut(|frame| {
                let Ok(mut value) = serde_json::from_str::<Value>(&frame.data) else {
                    return true;
                };
                let event = value.get("type").and_then(Value::as_str).unwrap_or("");
                if event.starts_with("response.reasoning_text.")
                    || (!summary && event.starts_with("response.reasoning_summary_"))
                {
                    return false;
                }
                scrub_meta_reasoning(&mut value, summary, encrypted);
                frame.data = value.to_string();
                true
            });
        }
        return Ok(());
    }
    if ctx.protocol != Protocol::OpenaiChat {
        return Ok(());
    }
    chat_output(ctx, body, &mut response.body);
    let usage = response.body.get("usage").filter(|v| !v.is_null()).cloned();
    if let Some(events) = &mut response.events {
        for frame in events.iter_mut() {
            if let Ok(mut value) = serde_json::from_str::<Value>(&frame.data) {
                chat_output(ctx, body, &mut value);
                if ctx.provider == "openrouter"
                    && value.get("error").is_some()
                    && ctx.fixture.scenario == Scenario::StreamError
                {
                    value = json!({"id":ctx.request_id,"object":"chat.completion.chunk","created":0,"model":ctx.model.id,"provider":"mock","error":{"code":500,"message":"Mock provider stream failure","metadata":{"error_type":"server"}},"choices":[{"index":0,"delta":{"content":""},"finish_reason":"error"}]});
                }
                frame.data = value.to_string();
            }
        }
        if (ctx.provider == "deepseek"
            && matches!(ctx.model.id.as_str(), "deepseek-flash" | "deepseek-v4-pro"))
            || ctx.provider == "moonshotai"
        {
            final_chunk_usage(
                events,
                usage,
                ctx.provider == "deepseek",
                body.pointer("/stream_options/include_usage")
                    .and_then(Value::as_bool)
                    == Some(true),
            );
        }
    }
    Ok(())
}

pub fn error(provider: &str, error: &ApiError, request_id: &str) -> Option<WireResponse> {
    if provider != "openrouter" {
        return None;
    }
    // https://openrouter.ai/docs/api_reference/errors-and-debugging#typed-error-codes
    let error_type = match error.status {
        400 => "invalid_request",
        401 => "authentication",
        402 => "payment_required",
        403 => "permission_denied",
        404 => "not_found",
        408 | 504 => "timeout",
        413 => "payload_too_large",
        429 => "rate_limit_exceeded",
        502 => "provider_unavailable",
        503 => "provider_overloaded",
        _ => "server",
    };
    let mut response = WireResponse::json(
        json!({"id":request_id,"error":{"code":error.status,"message":error.message,"metadata":{"error_type":error_type}}}),
    );
    response.status = error.status;
    Some(response)
}

fn chat_output(ctx: &RequestContext<'_>, body: &Value, value: &mut Value) {
    if let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices {
            if ctx.provider == "openrouter"
                && let Some(reason) = choice
                    .get("finish_reason")
                    .filter(|v| !v.is_null())
                    .cloned()
            {
                choice["native_finish_reason"] = reason;
            }
            for key in ["message", "delta"] {
                if let Some(message) = choice.get_mut(key).and_then(Value::as_object_mut) {
                    if ctx.provider == "meta" && message.contains_key("reasoning_content") {
                        message.insert("reasoning_content".into(), json!(""));
                    }
                    if ctx.provider == "minimax"
                        && body.get("reasoning_split").and_then(Value::as_bool) == Some(false)
                        && let Some(reasoning) = message
                            .remove("reasoning_content")
                            .and_then(|v| v.as_str().map(str::to_owned))
                    {
                        let content = message.get("content").and_then(Value::as_str).unwrap_or("");
                        message.insert(
                            "content".into(),
                            json!(format!("<think>{reasoning}</think>{content}")),
                        );
                    }
                }
            }
        }
    }
}

fn final_chunk_usage(
    events: &mut Vec<Frame>,
    body_usage: Option<Value>,
    always: bool,
    include: bool,
) {
    let usage = body_usage.or_else(|| {
        events
            .iter()
            .filter_map(|frame| serde_json::from_str::<Value>(&frame.data).ok())
            .find_map(|value| value.get("usage").filter(|v| !v.is_null()).cloned())
    });
    if !always && !include {
        return;
    }
    let Some(usage) = usage else {
        return;
    };
    events.retain_mut(|frame| {
        let Ok(mut value) = serde_json::from_str::<Value>(&frame.data) else {
            return true;
        };
        let Some(choices) = value.get("choices").and_then(Value::as_array) else {
            return true;
        };
        if choices.is_empty() && value.get("usage").is_some() {
            return false;
        }
        let final_chunk = choices
            .iter()
            .any(|choice| choice.get("finish_reason").is_some_and(|v| !v.is_null()));
        if final_chunk {
            value["usage"] = usage.clone();
        } else if include {
            value["usage"] = Value::Null;
        } else if let Some(object) = value.as_object_mut() {
            object.remove("usage");
        }
        frame.data = value.to_string();
        true
    });
}

fn scrub_meta_reasoning(value: &mut Value, summary: bool, encrypted: bool) {
    match value {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("reasoning") {
                object.remove("content");
                if !summary {
                    object.insert("summary".into(), json!([]));
                }
                if !encrypted {
                    object.remove("encrypted_content");
                }
            }
            for child in object.values_mut() {
                scrub_meta_reasoning(child, summary, encrypted);
            }
        }
        Value::Array(array) => {
            for child in array {
                scrub_meta_reasoning(child, summary, encrypted);
            }
        }
        _ => {}
    }
}

fn present(body: &Value, path: &str) -> bool {
    body.pointer(path).is_some_and(|v| !v.is_null())
}
fn param(path: &str) -> String {
    path.trim_start_matches('/').replace('/', ".")
}
fn messages(body: &Value) -> impl Iterator<Item = &Value> {
    body.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}
fn included(body: &Value, name: &str) -> bool {
    body.get("include")
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|v| v == name))
}

fn enumeration(body: &Value, path: &str, allowed: &[&str]) -> Result<(), ApiError> {
    if let Some(value) = body.pointer(path).filter(|v| !v.is_null())
        && !value.as_str().is_some_and(|s| allowed.contains(&s))
    {
        return Err(ApiError::invalid(
            &param(path),
            format!("Expected one of {}", allowed.join(", ")),
        ));
    }
    Ok(())
}
fn number(
    body: &Value,
    path: &str,
    min: f64,
    max: f64,
    exclusive_min: bool,
    exclusive_max: bool,
) -> Result<(), ApiError> {
    if let Some(value) = body.pointer(path).filter(|v| !v.is_null())
        && !value.as_f64().is_some_and(|n| {
            n >= min && n <= max && (!exclusive_min || n > min) && (!exclusive_max || n < max)
        })
    {
        return Err(ApiError::invalid(
            &param(path),
            "Value is outside the provider's documented numeric range",
        ));
    }
    Ok(())
}
fn integer(body: &Value, path: &str, min: i64, max: i64) -> Result<(), ApiError> {
    if let Some(value) = body.pointer(path).filter(|v| !v.is_null())
        && !value.as_i64().is_some_and(|n| n >= min && n <= max)
    {
        return Err(ApiError::invalid(
            &param(path),
            format!("Expected an integer from {min} to {max}"),
        ));
    }
    Ok(())
}
fn boolean(body: &Value, path: &str) -> Result<(), ApiError> {
    if body
        .pointer(path)
        .is_some_and(|v| !v.is_null() && !v.is_boolean())
    {
        return Err(ApiError::invalid(&param(path), "Expected a boolean"));
    }
    Ok(())
}
fn length(body: &Value, path: &str, max: usize) -> Result<(), ApiError> {
    if body
        .pointer(path)
        .is_some_and(|v| !v.is_null() && !v.as_str().is_some_and(|s| s.chars().count() <= max))
    {
        return Err(ApiError::invalid(
            &param(path),
            format!("Expected a string of at most {max} characters"),
        ));
    }
    Ok(())
}
fn max_items(body: &Value, path: &str, max: usize) -> Result<(), ApiError> {
    if body
        .pointer(path)
        .and_then(Value::as_array)
        .is_some_and(|v| v.len() > max)
    {
        return Err(ApiError::invalid(
            &param(path),
            format!("At most {max} items are supported"),
        ));
    }
    Ok(())
}
fn reject_fields(body: &Value, fields: &[&str]) -> Result<(), ApiError> {
    for field in fields {
        if body.get(*field).is_some_and(|v| !v.is_null()) {
            return Err(ApiError::invalid(
                field,
                format!("{field} is unsupported by this provider interface"),
            ));
        }
    }
    Ok(())
}
fn function_names(body: &Value, max: usize) -> Result<(), ApiError> {
    let mut seen = std::collections::BTreeSet::new();
    for tool in body
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = tool.pointer("/function/name").and_then(Value::as_str)
            && (name.is_empty()
                || name.len() > max
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                || !seen.insert(name))
        {
            return Err(ApiError::invalid(
                "tools",
                "Function names must be unique and follow the provider's name constraints",
            ));
        }
    }
    Ok(())
}
fn json_instruction(body: &Value) -> bool {
    messages(body)
        .filter(|message| {
            matches!(
                message.get("role").and_then(Value::as_str),
                Some("system" | "user")
            )
        })
        .any(|message| {
            message
                .get("content")
                .and_then(Value::as_str)
                .is_some_and(|text| text.to_lowercase().contains("json"))
                || message
                    .get("content")
                    .and_then(Value::as_array)
                    .is_some_and(|parts| {
                        parts.iter().any(|part| {
                            part.get("text")
                                .and_then(Value::as_str)
                                .is_some_and(|text| text.to_lowercase().contains("json"))
                        })
                    })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::model::{Fixture, ModelProfile};
    use axum::http::HeaderMap;

    fn profile(id: &str) -> ModelProfile {
        serde_json::from_value(json!({"id":id,"protocols":["openai_chat","openai_responses"]}))
            .unwrap()
    }
    fn context<'a>(
        provider: &'a str,
        model: &'a ModelProfile,
        fixture: &'a Fixture,
        headers: &'a HeaderMap,
    ) -> RequestContext<'a> {
        RequestContext {
            provider,
            protocol: Protocol::OpenaiChat,
            model,
            fixture,
            request_id: "mock-test",
            session_id: "session",
            turn: 0,
            seed: 0,
            headers,
            stream: false,
        }
    }

    #[test]
    fn meta_wonder_accepts_max_but_rejects_no_reasoning_without_affecting_unknown_models() {
        let model = profile("muse-wonder-1.0");
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let mut ctx = context("meta", &model, &fixture, &headers);
        assert!(validate(&ctx, &json!({"reasoning_effort":"max"}), &Value::Null).is_ok());
        assert_eq!(
            validate(&ctx, &json!({"reasoning_effort":"none"}), &Value::Null)
                .unwrap_err()
                .status,
            400
        );
        assert!(validate(&ctx, &json!({"stop":["END"]}), &Value::Null).is_err());
        let unknown = profile("future-model");
        ctx.model = &unknown;
        assert!(
            validate(
                &ctx,
                &json!({"reasoning_effort":"none","stop":["END"]}),
                &Value::Null
            )
            .is_ok()
        );
    }

    #[test]
    fn meta_responses_checks_replay_and_endpoint_specific_fields() {
        let model = profile("muse-wonder-1.0");
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let mut ctx = context("meta", &model, &fixture, &headers);
        ctx.protocol = Protocol::OpenaiResponses;
        for body in [
            json!({"max_output_tokens":15}),
            json!({"truncation":"auto"}),
            json!({"previous_response_id":"resp_1","include":["reasoning.encrypted_content"]}),
            json!({"input":[{"type":"reasoning","summary":[],"encrypted_content":"opaque"},{"role":"user","content":"next"}]}),
        ] {
            assert!(validate(&ctx, &body, &Value::Null).is_err(), "{body}");
        }
        assert!(validate(&ctx, &json!({"max_output_tokens":16,"input":[{"type":"reasoning","summary":[],"encrypted_content":"opaque"},{"role":"assistant","content":"answer"},{"role":"user","content":"next"}]}), &Value::Null).is_ok());
    }

    #[test]
    fn deepseek_requires_reasoning_only_for_known_thinking_tool_history() {
        let model = profile("deepseek-flash");
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let ctx = context("deepseek", &model, &fixture, &headers);
        let mut body = json!({"tools":[],"messages":[{"role":"assistant","content":"prior"}]});
        assert!(validate(&ctx, &body, &Value::Null).is_err());
        body["thinking"] = json!({"type":"disabled"});
        assert!(validate(&ctx, &body, &Value::Null).is_ok());
        body["thinking"] = json!({"type":"enabled"});
        body["messages"][0]["reasoning_content"] = json!("");
        assert!(validate(&ctx, &body, &Value::Null).is_ok());
    }

    #[test]
    fn ignored_minimax_parameters_and_unknown_zai_models_are_not_rejected() {
        let model = profile("MiniMax-M2.7");
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let ctx = context("minimax", &model, &fixture, &headers);
        assert!(
            validate(
                &ctx,
                &json!({"thinking":{"type":"disabled"},"frequency_penalty":1}),
                &Value::Null
            )
            .is_ok()
        );
        let model = profile("MiniMax-M3.1-Flash-Preview");
        let ctx = context("minimax", &model, &fixture, &headers);
        assert_eq!(
            validate(&ctx, &json!({"reasoning_effort":"none"}), &Value::Null)
                .unwrap_err()
                .code,
            "2013"
        );
        let unknown = profile("future-glm");
        let ctx = context("zhipuai", &unknown, &fixture, &headers);
        assert!(
            validate(
                &ctx,
                &json!({"temperature":1.5,"tool_choice":"required"}),
                &Value::Null
            )
            .is_ok()
        );
    }

    #[test]
    fn provider_stream_shapes_keep_usage_and_hide_meta_private_reasoning() {
        let model = profile("deepseek-flash");
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let mut ctx = context("deepseek", &model, &fixture, &headers);
        ctx.stream = true;
        let mut response = WireResponse::sse(vec![
            Frame::data(json!({"choices":[{"delta":{"content":"hi"},"finish_reason":null}]})),
            Frame::data(json!({"choices":[{"delta":{},"finish_reason":"stop"}]})),
            Frame::data(json!({"choices":[],"usage":{"total_tokens":12}})),
            Frame::done(),
        ]);
        transform(&ctx, &json!({"stream":true}), &mut response).unwrap();
        let events = response.events.unwrap();
        assert_eq!(events.len(), 3);
        let last: Value = serde_json::from_str(&events[1].data).unwrap();
        assert_eq!(last["usage"]["total_tokens"], 12);
        assert_eq!(last["choices"].as_array().unwrap().len(), 1);
        let model = profile("muse-wonder-1.0");
        let ctx = context("meta", &model, &fixture, &headers);
        let mut response = WireResponse::json(
            json!({"choices":[{"message":{"content":"answer","reasoning_content":"private"}}]}),
        );
        transform(&ctx, &json!({}), &mut response).unwrap();
        assert_eq!(
            response.body["choices"][0]["message"]["reasoning_content"],
            ""
        );
    }

    #[test]
    fn openrouter_stream_failure_remains_http_200_with_error_choice() {
        let model = profile("arbitrary/model");
        let fixture = Fixture {
            scenario: Scenario::StreamError,
            ..Fixture::default()
        };
        let headers = HeaderMap::new();
        let mut ctx = context("openrouter", &model, &fixture, &headers);
        ctx.stream = true;
        let mut response = WireResponse::sse(vec![Frame::data(
            json!({"error":{"code":"server_error","message":"failure"}}),
        )]);
        transform(&ctx, &json!({}), &mut response).unwrap();
        assert_eq!(response.status, 200);
        let value: Value = serde_json::from_str(&response.events.unwrap()[0].data).unwrap();
        assert_eq!(value["error"]["code"], 500);
        assert_eq!(value["error"]["metadata"]["error_type"], "server");
        assert_eq!(value["choices"][0]["finish_reason"], "error");
    }
}
