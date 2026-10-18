use std::collections::BTreeMap;

use axum::http::HeaderMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    OpenaiChat,
    OpenaiResponses,
    AnthropicMessages,
    GeminiGenerate,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub url: String,
    pub checked_on: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProfile {
    pub id: String,
    #[serde(default)]
    pub record: Option<String>,
    pub protocols: Vec<Protocol>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub output_modalities: Vec<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub structured_output: Option<bool>,
    /// Explicit model settings consumed by its provider adapter; no inferred aliases.
    #[serde(default)]
    pub behavior: BTreeMap<String, Value>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderProfile {
    pub id: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub protocols: Vec<Protocol>,
    pub models: Vec<ModelProfile>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    #[default]
    Text,
    ToolCall,
    ParallelTools,
    MalformedToolArguments,
    Structured,
    Reasoning,
    Refusal,
    Truncated,
    Empty,
    StopSequence,
    RateLimit,
    Overloaded,
    ServerError,
    Unauthorized,
    Forbidden,
    NotFound,
    ContextOverflow,
    StreamError,
    Disconnect,
    Slow,
    UnknownEvent,
    CacheMiss,
}

impl Scenario {
    pub const ALL: &'static [Self] = &[
        Self::Text,
        Self::ToolCall,
        Self::ParallelTools,
        Self::MalformedToolArguments,
        Self::Structured,
        Self::Reasoning,
        Self::Refusal,
        Self::Truncated,
        Self::Empty,
        Self::StopSequence,
        Self::RateLimit,
        Self::Overloaded,
        Self::ServerError,
        Self::Unauthorized,
        Self::Forbidden,
        Self::NotFound,
        Self::ContextOverflow,
        Self::StreamError,
        Self::Disconnect,
        Self::Slow,
        Self::UnknownEvent,
        Self::CacheMiss,
    ];

    pub fn name(self) -> String {
        serde_json::to_value(self)
            .expect("scenario enum")
            .as_str()
            .unwrap()
            .to_owned()
    }
}

fn default_text() -> String {
    "Mock response: ready ✓ 世界".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    #[serde(default)]
    pub scenario: Scenario,
    #[serde(default = "default_text")]
    pub text: String,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(default)]
    pub tool_arguments: Option<Value>,
    #[serde(default)]
    pub delay_ms: Option<u64>,
    #[serde(default)]
    pub chunk_delay_ms: Option<u64>,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            scenario: Scenario::Text,
            text: default_text(),
            output: None,
            tool_arguments: None,
            delay_ms: None,
            chunk_delay_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub steps: Vec<Fixture>,
    #[serde(default)]
    pub repeat: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub sources: Vec<Source>,
    pub providers: Vec<ProviderProfile>,
    #[serde(default)]
    pub script: Option<Script>,
}

pub struct RequestContext<'a> {
    pub provider: &'a str,
    pub protocol: Protocol,
    pub model: &'a ModelProfile,
    pub fixture: &'a Fixture,
    pub request_id: &'a str,
    pub session_id: &'a str,
    pub turn: u64,
    pub seed: u64,
    pub headers: &'a HeaderMap,
    pub stream: bool,
}

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub param: Option<String>,
}

impl ApiError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            param: None,
        }
    }
    pub fn invalid(param: &str, message: impl Into<String>) -> Self {
        Self {
            param: Some(param.into()),
            ..Self::new(400, "invalid_request_error", message)
        }
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(501, "mock_not_implemented", message)
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub event: Option<String>,
    pub data: String,
}

impl Frame {
    pub fn data(value: Value) -> Self {
        Self {
            event: None,
            data: value.to_string(),
        }
    }
    pub fn named(event: &str, value: Value) -> Self {
        Self {
            event: Some(event.into()),
            data: value.to_string(),
        }
    }
    pub fn done() -> Self {
        Self {
            event: None,
            data: "[DONE]".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WireResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
    pub events: Option<Vec<Frame>>,
}

impl WireResponse {
    pub fn json(body: Value) -> Self {
        Self {
            status: 200,
            headers: BTreeMap::new(),
            body,
            events: None,
        }
    }
    pub fn sse(events: Vec<Frame>) -> Self {
        Self {
            events: Some(events),
            ..Self::json(Value::Null)
        }
    }
}
