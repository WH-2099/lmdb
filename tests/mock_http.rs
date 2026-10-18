use std::{collections::BTreeSet, time::Duration};

use model_metadata::mock::{
    config,
    model::{Config, Fixture, Protocol, Scenario, Script},
    server::{self, Options},
    session::Suite,
};
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle, time::timeout};

const TEXT: &str = "Mock response: ready ✓ 世界";
const CHAT: &str = "/v1/chat/completions";
const RESPONSES: &str = "/v1/responses";
const MESSAGES: &str = "/v1/messages";
const GEMINI: &str = "/v1beta/models/gemini-4-argon:generateContent";
const GEMINI_STREAM: &str = "/v1beta/models/gemini-4-argon:streamGenerateContent?alt=sse";

struct Server {
    client: Client,
    url: String,
    provider: String,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(provider: &str) -> Self {
        Self::configured(
            config::load(None).unwrap(),
            Options {
                provider: provider.into(),
                ..Options::default()
            },
        )
        .await
    }

    async fn configured(config: Config, options: Options) -> Self {
        let provider = options.provider.clone();
        let base = options.base_path.clone();
        let router = server::router(config, options).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}{base}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            url,
            provider,
            task,
        }
    }

    fn request(&self, method: Method, path: &str, session: &str) -> RequestBuilder {
        let request = self
            .client
            .request(method, format!("{}{path}", self.url))
            .header("x-mock-session", session);
        match self.provider.as_str() {
            "anthropic" => request
                .header("x-api-key", "mock-key")
                .header("anthropic-version", "2023-06-01"),
            "google" | "gemini" => request.header("x-goog-api-key", "mock-key"),
            _ => request.bearer_auth("mock-key"),
        }
    }

    fn post(&self, path: &str, session: &str, body: &Value) -> RequestBuilder {
        self.request(Method::POST, path, session).json(body)
    }

    async fn report(&self, session: &str) -> Value {
        checked(
            self.request(Method::GET, &format!("/__mock/sessions/{session}"), session)
                .send()
                .await
                .unwrap(),
            StatusCode::OK,
        )
        .await
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn checked(response: Response, status: StatusCode) -> Value {
    let actual = response.status();
    let text = response.text().await.unwrap();
    assert_eq!(actual, status, "{text}");
    serde_json::from_str(&text).unwrap()
}

fn request_body(protocol: Protocol) -> Value {
    match protocol {
        Protocol::OpenaiChat => {
            json!({"model":"gpt-6-astra","messages":[{"role":"user","content":"Hello"}]})
        }
        Protocol::OpenaiResponses => json!({"model":"gpt-6-astra","input":"Hello"}),
        Protocol::AnthropicMessages => {
            json!({"model":"claude-opus-5-5","max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]})
        }
        Protocol::GeminiGenerate => {
            json!({"contents":[{"role":"user","parts":[{"text":"Hello"}]}]})
        }
    }
}

fn frames(text: &str) -> Vec<(Option<&str>, Value)> {
    text.split("\n\n")
        .filter_map(|frame| {
            let data = frame.lines().find_map(|line| line.strip_prefix("data: "))?;
            if data == "[DONE]" {
                return None;
            }
            let event = frame.lines().find_map(|line| line.strip_prefix("event: "));
            Some((event, serde_json::from_str(data).expect("valid SSE JSON")))
        })
        .collect()
}

fn chat_tool() -> Value {
    json!({"type":"function","function":{"name":"weather","parameters":{"type":"object","properties":{"city":{"type":"string","enum":["杭州"]}},"required":["city"],"additionalProperties":false}}})
}

#[tokio::test]
async fn native_json_and_sse_reconstruct_text_and_terminal_events() {
    for (provider, protocol, path, stream_path) in [
        ("openai", Protocol::OpenaiChat, CHAT, CHAT),
        ("openai", Protocol::OpenaiResponses, RESPONSES, RESPONSES),
        ("anthropic", Protocol::AnthropicMessages, MESSAGES, MESSAGES),
        ("google", Protocol::GeminiGenerate, GEMINI, GEMINI_STREAM),
    ] {
        let server = Server::configured(
            config::load(None).unwrap(),
            Options {
                provider: provider.into(),
                chunk_bytes: 1,
                ..Options::default()
            },
        )
        .await;
        let mut body = request_body(protocol);
        let response = server.post(path, "json", &body).send().await.unwrap();
        assert_eq!(response.headers()["x-mock-simulated"], "true");
        let request_id = response.headers()["x-mock-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        let value = checked(response, StatusCode::OK).await;
        match protocol {
            Protocol::OpenaiChat => {
                assert_eq!(value["choices"][0]["message"]["content"], TEXT);
                assert_eq!(value["choices"][0]["finish_reason"], "stop");
                assert_eq!(value["id"], format!("chatcmpl_{request_id}"));
            }
            Protocol::OpenaiResponses => {
                assert_eq!(value["output"][0]["content"][0]["text"], TEXT);
                assert_eq!(value["status"], "completed");
                assert_eq!(value["id"], format!("resp_{request_id}"));
            }
            Protocol::AnthropicMessages => {
                assert!(
                    value["content"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|part| part["text"] == TEXT)
                );
                assert_eq!(value["stop_reason"], "end_turn");
            }
            Protocol::GeminiGenerate => {
                assert_eq!(value["candidates"][0]["content"]["parts"][0]["text"], TEXT);
                assert_eq!(value["candidates"][0]["finishReason"], "STOP");
                assert_eq!(value["responseId"], request_id);
            }
        }
        if protocol != Protocol::GeminiGenerate {
            body["stream"] = json!(true);
        }
        if protocol == Protocol::OpenaiChat {
            body["stream_options"] = json!({"include_usage":true});
        }
        let response = server
            .post(stream_path, "stream", &body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        let text = String::from_utf8(response.bytes().await.unwrap().to_vec()).unwrap();
        let events = frames(&text);
        let reconstructed: String = events
            .iter()
            .filter_map(|(_, value)| match protocol {
                Protocol::OpenaiChat => value
                    .pointer("/choices/0/delta/content")
                    .and_then(Value::as_str),
                Protocol::OpenaiResponses if value["type"] == "response.output_text.delta" => {
                    value["delta"].as_str()
                }
                Protocol::AnthropicMessages
                    if value.pointer("/delta/type") == Some(&json!("text_delta")) =>
                {
                    value["delta"]["text"].as_str()
                }
                Protocol::GeminiGenerate => value
                    .pointer("/candidates/0/content/parts/0/text")
                    .and_then(Value::as_str),
                _ => None,
            })
            .collect();
        assert_eq!(reconstructed, TEXT, "{provider} {protocol:?}");
        match protocol {
            Protocol::OpenaiChat => {
                assert!(text.ends_with("data: [DONE]\n\n"));
                assert!(
                    events
                        .iter()
                        .any(|(_, event)| event["usage"]["total_tokens"].as_u64().is_some())
                );
            }
            Protocol::OpenaiResponses => {
                assert_eq!(events.last().unwrap().0, Some("response.completed"))
            }
            Protocol::AnthropicMessages => {
                assert_eq!(events.last().unwrap().0, Some("message_stop"))
            }
            Protocol::GeminiGenerate => {
                assert_eq!(
                    events.last().unwrap().1["candidates"][0]["finishReason"],
                    "STOP"
                );
                assert!(events.last().unwrap().1["usageMetadata"]["totalTokenCount"].is_number());
            }
        }
    }
}

#[tokio::test]
async fn authentication_and_version_errors_use_native_envelopes() {
    for (provider, protocol, path) in [
        ("openai", Protocol::OpenaiChat, CHAT),
        ("anthropic", Protocol::AnthropicMessages, MESSAGES),
        ("google", Protocol::GeminiGenerate, GEMINI),
    ] {
        let server = Server::start(provider).await;
        let response = server
            .client
            .post(format!("{}{path}", server.url))
            .json(&request_body(protocol))
            .send()
            .await
            .unwrap();
        let id = response.headers()["x-mock-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        let error = checked(response, StatusCode::UNAUTHORIZED).await;
        match protocol {
            Protocol::AnthropicMessages => {
                assert_eq!(error["type"], "error");
                assert_eq!(error["error"]["type"], "authentication_error");
                assert_eq!(error["request_id"], id);
            }
            Protocol::GeminiGenerate => {
                assert_eq!(error["error"]["code"], 401);
                assert_eq!(error["error"]["status"], "UNAUTHENTICATED");
            }
            _ => assert_eq!(error["error"]["type"], "authentication_error"),
        }
        if provider == "anthropic" {
            let response = server
                .client
                .post(format!("{}{path}", server.url))
                .header("x-api-key", "mock-key")
                .json(&request_body(protocol))
                .send()
                .await
                .unwrap();
            let error = checked(response, StatusCode::BAD_REQUEST).await;
            assert!(
                error["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("anthropic-version")
            );
        }
    }
}

#[tokio::test]
async fn scripts_advance_on_retry_errors_and_preserve_validation_failures_per_session() {
    let mut config = config::load(None).unwrap();
    config.script = Some(Script {
        steps: vec![
            Fixture {
                scenario: Scenario::RateLimit,
                ..Fixture::default()
            },
            Fixture {
                text: "Recovered ✓".into(),
                ..Fixture::default()
            },
        ],
        repeat: false,
    });
    let server = Server::configured(config, Options::default()).await;
    let mut body = request_body(Protocol::OpenaiChat);
    body["messages"] = json!([]);
    let response = server.post(CHAT, "retry", &body).send().await.unwrap();
    checked(response, StatusCode::BAD_REQUEST).await;
    body = request_body(Protocol::OpenaiChat);
    for session in ["retry", "independent"] {
        let response = server.post(CHAT, session, &body).send().await.unwrap();
        assert_eq!(response.headers()["retry-after"], "1");
        assert_eq!(response.headers()["x-mock-step"], "1");
        checked(response, StatusCode::TOO_MANY_REQUESTS).await;
    }
    let response = server.post(CHAT, "retry", &body).send().await.unwrap();
    assert_eq!(response.headers()["x-mock-step"], "2");
    let value = checked(response, StatusCode::OK).await;
    assert_eq!(value["choices"][0]["message"]["content"], "Recovered ✓");
    let report = server.report("retry").await;
    assert_eq!(report["sessions"][0]["accepted"], 2);
    assert_eq!(report["sessions"][0]["rejected"], 1);
    assert_eq!(report["sessions"][0]["next_index"], 2);
    assert_eq!(
        server.report("independent").await["sessions"][0]["accepted"],
        1
    );
}

#[tokio::test]
async fn chat_tools_and_responses_previous_id_survive_multiple_turns() {
    let server = Server::start("openai").await;
    let mut body = request_body(Protocol::OpenaiChat);
    body["tools"] = json!([chat_tool()]);
    checked(
        server
            .post(CHAT, "native-chat", &body)
            .send()
            .await
            .unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
    let gateway = Server::start("openrouter").await;
    body["model"] = json!("openai/gpt-6-astra");
    let response = gateway
        .post(CHAT, "chat-tools", &body)
        .header("x-mock-scenario", "tool_call")
        .send()
        .await
        .unwrap();
    let value = checked(response, StatusCode::OK).await;
    let assistant = value["choices"][0]["message"].clone();
    let call = &assistant["tool_calls"][0];
    assert_eq!(
        serde_json::from_str::<Value>(call["function"]["arguments"].as_str().unwrap()).unwrap()["city"],
        "杭州"
    );
    body["messages"].as_array_mut().unwrap().extend([
        assistant.clone(),
        json!({"role":"tool","tool_call_id":call["id"],"content":"sunny"}),
    ]);
    let value = checked(
        gateway
            .post(CHAT, "chat-tools", &body)
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(value["choices"][0]["finish_reason"], "stop");
    assert_eq!(value["choices"][0]["message"]["content"], TEXT);

    let mut body = request_body(Protocol::OpenaiResponses);
    let function = chat_tool()["function"].clone();
    body["tools"] =
        json!([{"type":"function","name":function["name"],"parameters":function["parameters"]}]);
    let response = server
        .post(RESPONSES, "responses-tools", &body)
        .header("x-mock-scenario", "tool_call")
        .send()
        .await
        .unwrap();
    let first = checked(response, StatusCode::OK).await;
    let call = first["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "function_call")
        .unwrap();
    let next = json!({"model":"gpt-6-astra","previous_response_id":first["id"],"input":[{"type":"function_call_output","call_id":call["call_id"],"output":"sunny"}]});
    checked(
        server.post(RESPONSES, "other", &next).send().await.unwrap(),
        StatusCode::NOT_FOUND,
    )
    .await;
    let second = checked(
        server
            .post(RESPONSES, "responses-tools", &next)
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(second["previous_response_id"], first["id"]);
    assert_eq!(second["output"][0]["content"][0]["text"], TEXT);
    let next = json!({"model":"gpt-6-astra","previous_response_id":second["id"],"input":"Thanks"});
    assert_eq!(
        checked(
            server
                .post(RESPONSES, "responses-tools", &next)
                .send()
                .await
                .unwrap(),
            StatusCode::OK
        )
        .await["status"],
        "completed"
    );
}

#[tokio::test]
async fn native_thinking_signatures_are_preserved_and_isolated() {
    let server = Server::start("anthropic").await;
    let mut body = request_body(Protocol::AnthropicMessages);
    body["tools"] = json!([{"name":"weather","input_schema":{"type":"object","properties":{}}}]);
    let first = checked(
        server
            .post(MESSAGES, "signed", &body)
            .header("x-mock-scenario", "tool_call")
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let blocks = first["content"].as_array().unwrap();
    let thinking_index = blocks
        .iter()
        .position(|part| part["type"] == "thinking")
        .unwrap();
    let call = blocks
        .iter()
        .find(|part| part["type"] == "tool_use")
        .unwrap();
    body["messages"].as_array_mut().unwrap().extend([json!({"role":"assistant","content":blocks}), json!({"role":"user","content":[{"type":"tool_result","tool_use_id":call["id"],"content":"sunny"}]})]);
    let mut altered = body.clone();
    altered["messages"][1]["content"][thinking_index]["signature"] = json!(format!(
        "{}-tampered",
        blocks[thinking_index]["signature"].as_str().unwrap()
    ));
    checked(
        server
            .post(MESSAGES, "signed", &altered)
            .send()
            .await
            .unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
    checked(
        server
            .post(MESSAGES, "foreign", &body)
            .send()
            .await
            .unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
    assert_eq!(
        checked(
            server.post(MESSAGES, "signed", &body).send().await.unwrap(),
            StatusCode::OK
        )
        .await["stop_reason"],
        "end_turn"
    );

    let server = Server::start("google").await;
    let mut body = request_body(Protocol::GeminiGenerate);
    body["tools"] = json!([{"functionDeclarations":[{"name":"weather","parameters":{"type":"OBJECT","properties":{}}}]}]);
    let first = checked(
        server
            .post(GEMINI, "signed", &body)
            .header("x-mock-scenario", "parallel_tools")
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let content = first["candidates"][0]["content"].clone();
    let parts = content["parts"].as_array().unwrap();
    assert!(parts[0]["thoughtSignature"].is_string());
    let results: Vec<_> = parts.iter().map(|part| json!({"functionResponse":{"id":part["functionCall"]["id"],"name":part["functionCall"]["name"],"response":{"weather":"sunny"}}})).collect();
    body["contents"]
        .as_array_mut()
        .unwrap()
        .extend([content, json!({"role":"user","parts":results})]);
    let mut altered = body.clone();
    altered["contents"][1]["parts"][0]["thoughtSignature"] = json!("tampered");
    checked(
        server
            .post(GEMINI, "signed", &altered)
            .send()
            .await
            .unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
    checked(
        server.post(GEMINI, "foreign", &body).send().await.unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
    let second = checked(
        server.post(GEMINI, "signed", &body).send().await.unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(second["candidates"][0]["content"]["parts"][0]["text"], TEXT);
}

#[tokio::test]
async fn dropping_a_stream_releases_session_and_inflight_capacity() {
    let server = Server::configured(
        config::load(None).unwrap(),
        Options {
            chunk_delay_ms: 250,
            max_inflight: 1,
            ..Options::default()
        },
    )
    .await;
    let mut body = request_body(Protocol::OpenaiChat);
    body["stream"] = json!(true);
    let mut stream = server.post(CHAT, "held", &body).send().await.unwrap();
    assert_eq!(stream.status(), StatusCode::OK);
    assert!(stream.chunk().await.unwrap().is_some());
    checked(
        server
            .post(CHAT, "other", &request_body(Protocol::OpenaiChat))
            .send()
            .await
            .unwrap(),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    let response = server
        .request(Method::DELETE, "/__mock/sessions/held", "held")
        .send()
        .await
        .unwrap();
    checked(response, StatusCode::CONFLICT).await;
    drop(stream);
    timeout(Duration::from_secs(2), async {
        loop {
            if server.report("held").await["sessions"][0]["in_flight"] == false {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("dropped stream released its session");
    checked(
        server
            .post(CHAT, "held", &request_body(Protocol::OpenaiChat))
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    checked(
        server
            .request(Method::DELETE, "/__mock/sessions/held", "held")
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert!(
        server.report("held").await["sessions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn request_body_bounds_and_malformed_payloads_have_json_errors() {
    let server = Server::configured(
        config::load(None).unwrap(),
        Options {
            max_body_bytes: 256,
            base_path: "/mock".into(),
            ..Options::default()
        },
    )
    .await;
    for (body, content_type, status) in [
        ("{".to_owned(), "application/json", StatusCode::BAD_REQUEST),
        ("[]".to_owned(), "application/json", StatusCode::BAD_REQUEST),
        (
            "{}".to_owned(),
            "text/plain",
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            " ".repeat(257),
            "application/json",
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let response = server
            .request(Method::POST, CHAT, "bounds")
            .header("content-type", content_type)
            .body(body)
            .send()
            .await
            .unwrap();
        assert!(checked(response, status).await["error"]["message"].is_string());
    }
    checked(
        server
            .post(CHAT, "bounds", &request_body(Protocol::OpenaiChat))
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let response = server
        .client
        .get(format!(
            "{}/v1/models",
            server.url.trim_end_matches("/mock")
        ))
        .bearer_auth("mock-key")
        .send()
        .await
        .unwrap();
    checked(response, StatusCode::NOT_FOUND).await;
}

#[tokio::test]
async fn body_read_timeout_releases_capacity_and_allows_configured_response_delays() {
    let server = Server::configured(
        config::load(None).unwrap(),
        Options {
            request_timeout_ms: 40,
            delay_ms: 120,
            chunk_delay_ms: 10,
            max_inflight: 1,
            ..Options::default()
        },
    )
    .await;
    let body = request_body(Protocol::OpenaiChat).to_string();
    let slow = reqwest::Body::wrap_stream(futures_util::stream::once(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        Ok::<_, std::io::Error>(body)
    }));
    checked(
        server
            .request(Method::POST, CHAT, "slow-upload")
            .header("content-type", "application/json")
            .body(slow)
            .send()
            .await
            .unwrap(),
        StatusCode::REQUEST_TIMEOUT,
    )
    .await;
    let mut body = request_body(Protocol::OpenaiChat);
    body["stream"] = json!(true);
    let response = server.post(CHAT, "delayed", &body).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.text().await.unwrap().ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn model_lists_preserve_exact_configured_ids_and_selection() {
    let config = config::load(None).unwrap();
    for profile in config
        .providers
        .iter()
        .filter(|profile| !profile.models.is_empty())
    {
        let server = Server::configured(
            config.clone(),
            Options {
                provider: profile.id.clone(),
                ..Options::default()
            },
        )
        .await;
        let path = if profile.id == "google" {
            "/v1beta/models"
        } else {
            "/v1/models"
        };
        let value = checked(
            server
                .request(Method::GET, path, "list")
                .send()
                .await
                .unwrap(),
            StatusCode::OK,
        )
        .await;
        let actual: BTreeSet<_> = if profile.id == "google" {
            value["models"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["baseModelId"].as_str().unwrap())
                .collect()
        } else {
            value["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["id"].as_str().unwrap())
                .collect()
        };
        assert_eq!(
            actual,
            profile
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect(),
            "{}",
            profile.id
        );
        for model in &profile.models {
            let protocol = model.protocols[0];
            let mut body = request_body(protocol);
            let path = match protocol {
                Protocol::OpenaiChat => CHAT.to_owned(),
                Protocol::OpenaiResponses => RESPONSES.to_owned(),
                Protocol::AnthropicMessages => MESSAGES.to_owned(),
                Protocol::GeminiGenerate => format!("/v1beta/models/{}:generateContent", model.id),
            };
            if protocol != Protocol::GeminiGenerate {
                body["model"] = json!(model.id);
            }
            let response = checked(
                server.post(&path, "smoke", &body).send().await.unwrap(),
                StatusCode::OK,
            )
            .await;
            assert_eq!(
                response[if protocol == Protocol::GeminiGenerate {
                    "modelVersion"
                } else {
                    "model"
                }],
                model.id,
                "{}",
                profile.id
            );
        }
    }
    let server = Server::configured(
        config,
        Options {
            models: vec!["gpt-6.1-sol".into()],
            ..Options::default()
        },
    )
    .await;
    let list = checked(
        server
            .request(Method::GET, "/v1/models", "list")
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(list["data"].as_array().unwrap().len(), 1);
    assert_eq!(list["data"][0]["id"], "gpt-6.1-sol");
    checked(
        server
            .post(CHAT, "list", &request_body(Protocol::OpenaiChat))
            .send()
            .await
            .unwrap(),
        StatusCode::NOT_FOUND,
    )
    .await;
}

#[tokio::test]
async fn meta_wonder_enforces_its_reasoning_and_responses_contract() {
    let server = Server::start("meta").await;
    let mut body = request_body(Protocol::OpenaiChat);
    body["model"] = json!("muse-wonder-1.0");
    body["reasoning_effort"] = json!("none");
    checked(
        server.post(CHAT, "meta", &body).send().await.unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
    body["reasoning_effort"] = json!("max");
    let value = checked(
        server
            .post(CHAT, "meta", &body)
            .header("x-mock-scenario", "reasoning")
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(value["choices"][0]["message"]["reasoning_content"], "");
    let mut body = json!({"model":"muse-wonder-1.0","input":"Hello","reasoning":{"effort":"max","summary":"auto"},"max_output_tokens":16});
    body["truncation"] = json!("auto");
    checked(
        server.post(RESPONSES, "meta", &body).send().await.unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
    body["truncation"] = json!("disabled");
    let first = checked(
        server
            .post(RESPONSES, "meta", &body)
            .header("x-mock-scenario", "reasoning")
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let reasoning = first["output"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "reasoning")
        .unwrap();
    assert!(reasoning.get("encrypted_content").is_none());
    body["previous_response_id"] = first["id"].clone();
    body["include"] = json!(["reasoning.encrypted_content"]);
    checked(
        server.post(RESPONSES, "meta", &body).send().await.unwrap(),
        StatusCode::BAD_REQUEST,
    )
    .await;
}

#[tokio::test]
async fn coverage_reports_every_case_and_replays_a_seeded_sequence() {
    let options = Options {
        suite: Suite::Coverage,
        seed: 42,
        ..Options::default()
    };
    let first = Server::configured(config::load(None).unwrap(), options.clone()).await;
    let second = Server::configured(config::load(None).unwrap(), options).await;
    let mut trace = Vec::new();
    for (session, stream) in [("json", false), ("stream", true)] {
        let mut body = request_body(Protocol::OpenaiChat);
        body["stream"] = json!(stream);
        for _ in 0..Scenario::ALL.len() {
            let response = first.post(CHAT, session, &body).send().await.unwrap();
            trace.push((
                session,
                response.headers()["x-mock-scenario"]
                    .to_str()
                    .unwrap()
                    .to_owned(),
                response.status(),
            ));
            // The disconnect case intentionally ends the HTTP body before completion.
            let _ = response.bytes().await;
        }
        let report = first.report(session).await;
        assert_eq!(report["seed"], 42);
        let current = &report["sessions"][0];
        assert_eq!(current["rejected"], 0, "{report}");
        assert_eq!(current["accepted"], Scenario::ALL.len());
        let cases = current["cases"].as_array().unwrap();
        assert_eq!(cases.len(), Scenario::ALL.len());
        assert!(
            cases.iter().all(|case| case["status"] != "pending"),
            "{report}"
        );
        assert!(
            cases
                .iter()
                .any(|case| case["case"] == "tool_call" && case["status"] == "skipped")
        );
        for scenario in ["disconnect", "stream_error", "unknown_event"] {
            let case = cases.iter().find(|case| case["case"] == scenario).unwrap();
            assert_eq!(case["status"], if stream { "covered" } else { "skipped" });
        }
    }
    for (session, scenario, status) in trace {
        let mut body = request_body(Protocol::OpenaiChat);
        body["stream"] = json!(session == "stream");
        let response = second.post(CHAT, session, &body).send().await.unwrap();
        assert_eq!(response.headers()["x-mock-scenario"], scenario);
        assert_eq!(response.status(), status);
        let _ = response.bytes().await;
    }
    for session in ["json", "stream"] {
        let a = first.report(session).await;
        let b = second.report(session).await;
        assert_eq!(a["coverage_order"], b["coverage_order"]);
        assert_eq!(a["sessions"][0]["cases"], b["sessions"][0]["cases"]);
    }
}

#[tokio::test]
async fn two_native_gemini_sessions_cover_every_scenario() {
    let server = Server::configured(
        config::load(None).unwrap(),
        Options {
            provider: "google".into(),
            suite: Suite::Coverage,
            seed: 42,
            ..Options::default()
        },
    )
    .await;
    let mut tools = request_body(Protocol::GeminiGenerate);
    tools["tools"] = json!([{"functionDeclarations":[
        {"name":"weather","parameters":{"type":"OBJECT","properties":{}}},
        {"name":"clock","parameters":{"type":"OBJECT","properties":{}}}
    ]}]);
    tools["generationConfig"] = json!({
        "thinkingConfig":{"thinkingLevel":"HIGH","includeThoughts":true},
        "stopSequences":["STOP"],
        "maxOutputTokens":1024
    });
    let mut structured = request_body(Protocol::GeminiGenerate);
    structured["generationConfig"] = json!({
        "responseMimeType":"application/json",
        "responseJsonSchema":{
            "type":"object",
            "properties":{"ready":{"type":"boolean"}},
            "required":["ready"],
            "additionalProperties":false
        }
    });
    let mut covered = BTreeSet::new();
    for (session, path, body) in [
        ("tools", GEMINI_STREAM, tools),
        ("structured", GEMINI, structured),
    ] {
        for _ in 0..Scenario::ALL.len() {
            let response = server.post(path, session, &body).send().await.unwrap();
            let scenario = response.headers()["x-mock-scenario"]
                .to_str()
                .unwrap()
                .to_owned();
            let bytes = response.bytes().await;
            assert!(
                bytes.is_ok() || scenario == "disconnect",
                "{scenario}: {bytes:?}"
            );
            if scenario == "structured" {
                let response: Value = serde_json::from_slice(&bytes.unwrap()).unwrap();
                let output: Value = serde_json::from_str(
                    response["candidates"][0]["content"]["parts"][0]["text"]
                        .as_str()
                        .unwrap(),
                )
                .unwrap();
                assert!(output["ready"].is_boolean());
                assert_eq!(output.as_object().unwrap().len(), 1);
            }
        }
        let report = server.report(session).await;
        let current = &report["sessions"][0];
        assert_eq!(current["accepted"], Scenario::ALL.len(), "{report}");
        assert_eq!(current["rejected"], 0, "{report}");
        for case in current["cases"].as_array().unwrap() {
            if case["status"] == "covered" {
                covered.insert(case["case"].as_str().unwrap().to_owned());
            }
        }
    }
    assert_eq!(
        covered,
        Scenario::ALL
            .iter()
            .map(|scenario| scenario.name())
            .collect()
    );
    assert_eq!(server.report("structured").await["session_count"], 2);
}
