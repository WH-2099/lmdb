use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::model::{ApiError, Fixture, Scenario, Script};

const MAX_KEYS: usize = 32;
const MAX_STATE_BYTES: usize = 1024 * 1024;
const MAX_LEDGER: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    Normal,
    Coverage,
}

#[derive(Clone)]
pub struct Engine(Arc<Mutex<Inner>>);

struct Inner {
    seed: u64,
    suite: Suite,
    script: Option<Script>,
    order: Vec<Scenario>,
    max_sessions: usize,
    ttl: Duration,
    sessions: BTreeMap<String, Session>,
    refused: u64,
}

struct Session {
    last_used: Instant,
    in_flight: bool,
    accepted: u64,
    rejected: u64,
    canceled: u64,
    steps: u64,
    cursor: usize,
    states: BTreeMap<String, (Value, usize)>,
    state_bytes: usize,
    covered: BTreeMap<String, u64>,
    skipped: BTreeMap<String, (u64, &'static str)>,
    ledger: VecDeque<Value>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            last_used: Instant::now(),
            in_flight: false,
            accepted: 0,
            rejected: 0,
            canceled: 0,
            steps: 0,
            cursor: 0,
            states: BTreeMap::new(),
            state_bytes: 2,
            covered: BTreeMap::new(),
            skipped: BTreeMap::new(),
            ledger: VecDeque::new(),
        }
    }
}

impl Session {
    fn log(&mut self, event: Value) {
        if self.ledger.len() == MAX_LEDGER {
            self.ledger.pop_front();
        }
        self.ledger.push_back(event);
    }
}

pub struct Turn {
    pub fixture: Fixture,
    pub state: Value,
    pub number: u64,
    pub request_id: String,
    engine: Engine,
    session: String,
    key: String,
    cursor: usize,
    advance: bool,
    source: &'static str,
    skipped: Vec<(Scenario, &'static str)>,
    finished: bool,
}

impl Engine {
    pub fn new(
        seed: u64,
        suite: Suite,
        script: Option<Script>,
        max_sessions: usize,
        ttl: Duration,
    ) -> Self {
        let mut order = Scenario::ALL.to_vec();
        let mut random = seed;
        for index in (1..order.len()).rev() {
            random = random.wrapping_add(0x9e3779b97f4a7c15);
            let mut mixed = random;
            mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d049bb133111eb);
            mixed ^= mixed >> 31;
            order.swap(index, (mixed % (index as u64 + 1)) as usize);
        }
        Self(Arc::new(Mutex::new(Inner {
            seed,
            suite,
            script,
            order,
            max_sessions,
            ttl,
            sessions: BTreeMap::new(),
            refused: 0,
        })))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn begin(
        &self,
        session: &str,
        key: &str,
        body: &Value,
        stream: bool,
        override_scenario: Option<Scenario>,
    ) -> Result<Turn, ApiError> {
        let mut inner = self.lock();
        if let Err(error) = validate_session(session) {
            inner.refused = inner.refused.saturating_add(1);
            return Err(error);
        }
        if key.is_empty() || key.len() > 1024 || key.chars().any(char::is_control) {
            inner.refused = inner.refused.saturating_add(1);
            return Err(ApiError::invalid(
                "model",
                "Session state keys must contain 1 to 1024 bytes without control characters",
            ));
        }
        inner.expire();
        if inner.sessions.get(session).is_some_and(|s| s.in_flight) {
            inner.refused = inner.refused.saturating_add(1);
            return Err(busy());
        }
        if !inner.sessions.contains_key(session) && inner.sessions.len() >= inner.max_sessions {
            inner.refused = inner.refused.saturating_add(1);
            return Err(ApiError::new(
                429,
                "mock_session_capacity",
                "The session limit has been reached; reset an idle session or wait for its expiry",
            ));
        }
        let current = inner.sessions.entry(session.into()).or_default();
        let number = current.accepted.saturating_add(1);
        let state = current
            .states
            .get(key)
            .map(|(value, _)| value.clone())
            .unwrap_or(Value::Null);
        let cursor = current.cursor;
        let mut next_cursor = cursor;
        let mut skipped = Vec::new();
        let (mut fixture, source, advance) = if let Some(scenario) = override_scenario {
            (
                Fixture {
                    scenario,
                    ..Fixture::default()
                },
                "override",
                false,
            )
        } else if latest_tool_result(key, body) {
            (Fixture::default(), "tool_result_continuation", false)
        } else if let Some(script) = &inner.script {
            if script.steps.is_empty() || (cursor >= script.steps.len() && !script.repeat) {
                (Fixture::default(), "script_exhausted", false)
            } else {
                let index = cursor % script.steps.len();
                next_cursor = if script.repeat {
                    (index + 1) % script.steps.len()
                } else {
                    index + 1
                };
                (script.steps[index].clone(), "script", true)
            }
        } else if inner.suite == Suite::Coverage {
            let mut selected = Fixture::default();
            for offset in 0..inner.order.len() {
                let index = (cursor + offset) % inner.order.len();
                let scenario = inner.order[index];
                next_cursor = (index + 1) % inner.order.len();
                if let Some(reason) = ineligible(scenario, key, body, stream) {
                    skipped.push((scenario, reason));
                } else {
                    selected.scenario = scenario;
                    break;
                }
            }
            (selected, "coverage", true)
        } else {
            (Fixture::default(), "normal", true)
        };
        if fixture.scenario == Scenario::StopSequence
            && matches!(source, "coverage" | "override")
            && let Some(stop) = first_stop(body)
        {
            fixture.text.push_str(stop);
            fixture
                .text
                .push_str(" This text follows the stop sequence.");
        }
        inner
            .sessions
            .get_mut(session)
            .expect("session exists")
            .in_flight = true;
        Ok(Turn {
            fixture,
            state,
            number,
            request_id: request_id(inner.seed, session, number),
            engine: self.clone(),
            session: session.into(),
            key: key.into(),
            cursor: next_cursor,
            advance,
            source,
            skipped,
            finished: false,
        })
    }

    pub fn report(&self, session: Option<&str>) -> Value {
        let mut inner = self.lock();
        inner.expire();
        let sessions: Vec<_> = inner.sessions.iter()
            .filter(|(id, _)| session.is_none_or(|wanted| wanted == id.as_str()))
            .map(|(id, current)| {
                let cases: Vec<_> = Scenario::ALL.iter().map(|scenario| {
                    let name = scenario.name();
                    let covered = current.covered.get(&name).copied().unwrap_or(0);
                    let skipped = current.skipped.get(&name);
                    json!({"case": name, "covered": covered,
                        "skipped": skipped.map_or(0, |(count, _)| *count),
                        "skip_reason": skipped.map(|(_, reason)| reason),
                        "status": if covered > 0 { "covered" } else if skipped.is_some() { "skipped" } else { "pending" }})
                }).collect();
                json!({"session":id,"in_flight":current.in_flight,"accepted":current.accepted,
                    "rejected":current.rejected,"canceled":current.canceled,"steps":current.steps,
                    "next_index":current.cursor,"state_keys":current.states.len(),"state_bytes":current.state_bytes,
                    "cases":cases,"ledger":current.ledger})
            }).collect();
        json!({"seed":inner.seed,"suite":if inner.suite == Suite::Coverage {"coverage"} else {"normal"},
            "scripted":inner.script.is_some(),"session_count":inner.sessions.len(),"max_sessions":inner.max_sessions,
            "ttl_seconds":inner.ttl.as_secs(),"refused_before_turn":inner.refused,
            "coverage_order":inner.order.iter().map(|scenario|scenario.name()).collect::<Vec<_>>(),
            "sequence_policy":{"accepted_http_errors":"advance","validation_errors":"preserve",
                "override":"preserve","tool_result_continuation":"preserve","coverage_exhausted":"repeat",
                "script_exhausted":"text unless repeat is enabled"},
            "limits":{"state_keys_per_session":MAX_KEYS,"state_bytes_per_session":MAX_STATE_BYTES,"ledger_per_session":MAX_LEDGER},
            "sessions":sessions})
    }

    pub fn reset(&self, session: &str) -> Result<(), ApiError> {
        validate_session(session)?;
        let mut inner = self.lock();
        inner.expire();
        if inner.sessions.get(session).is_some_and(|s| s.in_flight) {
            return Err(busy());
        }
        inner.sessions.remove(session);
        Ok(())
    }
}

impl Inner {
    fn expire(&mut self) {
        let now = Instant::now();
        self.sessions.retain(|_, session| {
            session.in_flight || now.duration_since(session.last_used) < self.ttl
        });
    }
}

impl Turn {
    pub fn accept(&mut self, state: Value, status: u16) -> Result<(), ApiError> {
        if self.finished {
            return Err(ApiError::new(
                409,
                "mock_turn_finished",
                "The turn has already been accepted or rejected",
            ));
        }
        let bytes = serde_json::to_vec(&state)
            .expect("JSON value serializes")
            .len()
            + serde_json::to_vec(&self.key)
                .expect("state key serializes")
                .len()
            + 1;
        let mut inner = self.engine.lock();
        let session = inner
            .sessions
            .get_mut(&self.session)
            .expect("active session exists");
        let old_bytes = session.states.get(&self.key).map(|(_, bytes)| *bytes);
        let total = session.state_bytes - old_bytes.unwrap_or(0)
            + bytes
            + usize::from(old_bytes.is_none() && !session.states.is_empty());
        let error = if old_bytes.is_none() && session.states.len() >= MAX_KEYS {
            Some(ApiError::new(
                429,
                "mock_state_capacity",
                "A session can retain at most 32 protocol/model state keys",
            ))
        } else if total > MAX_STATE_BYTES {
            Some(ApiError::new(
                429,
                "mock_state_capacity",
                "Session protocol state exceeds 1 MiB; reset the session",
            ))
        } else {
            None
        };
        if let Some(error) = error {
            self.record_rejection(session, &error);
            self.finished = true;
            return Err(error);
        }
        session.states.insert(self.key.clone(), (state, bytes));
        session.state_bytes = total;
        session.accepted = self.number;
        if self.advance {
            session.cursor = self.cursor;
            session.steps = session.steps.saturating_add(1);
        }
        let covered = session
            .covered
            .entry(self.fixture.scenario.name())
            .or_default();
        *covered = covered.saturating_add(1);
        for (scenario, reason) in &self.skipped {
            let skipped = session
                .skipped
                .entry(scenario.name())
                .or_insert((0, reason));
            skipped.0 = skipped.0.saturating_add(1);
            skipped.1 = reason;
        }
        session.log(self.event("accepted", status, None));
        self.finished = true;
        Ok(())
    }

    pub fn reject(&mut self, error: &ApiError) {
        if self.finished {
            return;
        }
        let mut inner = self.engine.lock();
        if let Some(session) = inner.sessions.get_mut(&self.session) {
            self.record_rejection(session, error);
        }
        self.finished = true;
    }

    fn record_rejection(&self, session: &mut Session, error: &ApiError) {
        session.rejected = session.rejected.saturating_add(1);
        session.log(self.event("rejected", error.status, Some(&error.code)));
    }

    fn event(&self, outcome: &str, status: u16, error: Option<&str>) -> Value {
        json!({"turn":self.number,"request_id":self.request_id,"case":self.fixture.scenario,
            "source":self.source,"outcome":outcome,"status":status,
            "error_code":error.map(|code|code.chars().take(64).collect::<String>())})
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut inner = self.engine.lock();
        if let Some(session) = inner.sessions.get_mut(&self.session) {
            if !self.finished {
                session.canceled = session.canceled.saturating_add(1);
                session.log(self.event("canceled", 499, None));
            }
            session.in_flight = false;
            session.last_used = Instant::now();
        }
    }
}

fn busy() -> ApiError {
    ApiError::new(
        409,
        "mock_session_busy",
        "This session already has an active request",
    )
}

fn validate_session(session: &str) -> Result<(), ApiError> {
    if session.is_empty()
        || session.len() > 128
        || !session
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
    {
        return Err(ApiError::invalid(
            "x-mock-session",
            "Session IDs must contain 1 to 128 ASCII letters, digits, underscores, hyphens or periods",
        ));
    }
    Ok(())
}

fn request_id(seed: u64, session: &str, number: u64) -> String {
    let mut hash = Sha256::new();
    hash.update(seed.to_le_bytes());
    hash.update((session.len() as u64).to_le_bytes());
    hash.update(session.as_bytes());
    hash.update(number.to_le_bytes());
    format!("mock_{}", &format!("{:x}", hash.finalize())[..24])
}

fn latest_tool_result(key: &str, body: &Value) -> bool {
    let last = |field: &str| {
        body.get(field)
            .and_then(Value::as_array)
            .and_then(|items| items.last())
    };
    match key.split('/').nth(1) {
        Some("openai_chat") => last("messages").is_some_and(|message| message["role"] == "tool"),
        Some("anthropic_messages") => last("messages").is_some_and(|message| {
            message["role"] == "user"
                && message["content"]
                    .as_array()
                    .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "tool_result"))
        }),
        Some("openai_responses") => {
            last("input").is_some_and(|input| input["type"] == "function_call_output")
        }
        Some("gemini_generate") => last("contents").is_some_and(|content| {
            content["role"] != "model"
                && content["parts"].as_array().is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part.get("functionResponse").is_some()
                            || part.get("function_response").is_some()
                    })
                })
        }),
        _ => false,
    }
}

fn ineligible(scenario: Scenario, key: &str, body: &Value, stream: bool) -> Option<&'static str> {
    let protocol = key.split('/').nth(1);
    let anthropic = protocol == Some("anthropic_messages");
    let gemini = protocol == Some("gemini_generate");
    let gemini_tools = field(body, "toolConfig", "tool_config")
        .and_then(|config| field(config, "functionCallingConfig", "function_calling_config"));
    let gemini_mode = gemini_tools.and_then(|config| config.get("mode"));
    let generation = field(body, "generationConfig", "generation_config");
    let choice = body.pointer("/tool_choice/type").and_then(Value::as_str);
    let forced_tools = body["tool_choice"] == "required"
        || matches!(choice, Some("any" | "tool" | "function"))
        || gemini_mode.is_some_and(|mode| mode == "ANY");
    if forced_tools
        && (matches!(
            scenario,
            Scenario::Text
                | Scenario::Structured
                | Scenario::Reasoning
                | Scenario::Truncated
                | Scenario::Empty
                | Scenario::StopSequence
        ) || (scenario == Scenario::Refusal && !gemini)
            || (scenario == Scenario::StreamError && anthropic))
    {
        return Some("forced tool choice would replace the requested output case");
    }
    if matches!(
        scenario,
        Scenario::StreamError | Scenario::Disconnect | Scenario::UnknownEvent
    ) && !stream
    {
        return Some("requires a streaming request");
    }
    if matches!(
        scenario,
        Scenario::ToolCall | Scenario::ParallelTools | Scenario::MalformedToolArguments
    ) {
        let allowed = gemini_tools
            .and_then(|config| field(config, "allowedFunctionNames", "allowed_function_names"))
            .and_then(Value::as_array);
        let count = body["tools"].as_array().map_or(0, |tools| {
            tools
                .iter()
                .map(|tool| {
                    tool.get("functionDeclarations")
                        .or_else(|| tool.get("function_declarations"))
                        .and_then(Value::as_array)
                        .map_or_else(
                            || {
                                usize::from(
                                    tool.get("name").is_some() || tool.get("function").is_some(),
                                )
                            },
                            |functions| {
                                functions
                                    .iter()
                                    .filter(|function| {
                                        allowed
                                            .is_none_or(|names| names.contains(&function["name"]))
                                    })
                                    .count()
                            },
                        )
                })
                .sum::<usize>()
        });
        if count == 0 {
            return Some("requires at least one declared function tool");
        }
        if body["tool_choice"] == "none"
            || body
                .pointer("/tool_choice/type")
                .is_some_and(|value| value == "none")
            || gemini_mode.is_some_and(|mode| mode == "NONE")
        {
            return Some("requires tool calling to be enabled");
        }
        if scenario == Scenario::ParallelTools
            && (body["parallel_tool_calls"] == false
                || body.pointer("/tool_choice/disable_parallel_tool_use")
                    == Some(&Value::Bool(true))
                || (anthropic
                    && (count < 2
                        || body
                            .pointer("/tool_choice/type")
                            .is_some_and(|value| value == "tool"))))
        {
            return Some("requires parallel tool calls and enough selectable tools");
        }
    }
    if scenario == Scenario::Structured
        && ![
            "/response_format/json_schema/schema",
            "/text/format/schema",
            "/output_config/format/schema",
        ]
        .iter()
        .any(|path| body.pointer(path).is_some())
        && !generation.is_some_and(|config| {
            field(config, "responseSchema", "response_schema").is_some()
                || field(config, "responseJsonSchema", "response_json_schema").is_some()
        })
    {
        return Some("requires an output JSON schema");
    }
    if scenario == Scenario::Reasoning {
        let explicit = body.get("reasoning").is_some()
            || body.get("reasoning_effort").is_some()
            || body.get("thinking").is_some()
            || generation
                .is_some_and(|config| field(config, "thinkingConfig", "thinking_config").is_some());
        if !explicit {
            return Some("requires explicit reasoning or thinking settings in the request");
        }
        if anthropic
            && (!body
                .pointer("/thinking/type")
                .is_some_and(|value| value == "enabled" || value == "adaptive")
                || body["max_tokens"].as_u64() == Some(0))
        {
            return Some("requires an explicit enabled or adaptive thinking request");
        }
        if body["reasoning_effort"] == "none"
            || body
                .pointer("/reasoning/effort")
                .is_some_and(|value| value == "none")
            || generation
                .and_then(|config| field(config, "thinkingConfig", "thinking_config"))
                .and_then(|thinking| field(thinking, "thinkingBudget", "thinking_budget"))
                .and_then(Value::as_i64)
                == Some(0)
        {
            return Some("requires reasoning to be enabled");
        }
    }
    if scenario == Scenario::StopSequence && first_stop(body).is_none() {
        return Some("requires a configured stop sequence");
    }
    None
}

fn field<'a>(value: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    value.get(camel).or_else(|| value.get(snake))
}

fn first_stop(body: &Value) -> Option<&str> {
    body.get("stop")
        .or_else(|| body.get("stop_sequences"))
        .or_else(|| {
            field(body, "generationConfig", "generation_config")
                .and_then(|config| field(config, "stopSequences", "stop_sequences"))
        })
        .and_then(|value| {
            value.as_str().or_else(|| {
                value
                    .as_array()
                    .and_then(|items| items.first())
                    .and_then(Value::as_str)
            })
        })
        .filter(|value| !value.is_empty())
}

pub fn plan() -> Value {
    let cases: Vec<_> = Scenario::ALL.iter().map(|scenario| {
        let preconditions: &[&str] = match scenario {
            Scenario::StreamError | Scenario::Disconnect | Scenario::UnknownEvent => &["streaming request"],
            Scenario::ToolCall | Scenario::MalformedToolArguments => &["declared function tools", "tool calling enabled"],
            Scenario::ParallelTools => &["declared function tools", "parallel tool calling enabled"],
            Scenario::Structured => &["output JSON schema"],
            Scenario::Reasoning => &["model supports reasoning", "explicit reasoning or thinking settings"],
            Scenario::StopSequence => &["configured stop sequence"],
            _ => &[],
        };
        let status = match scenario {
            Scenario::RateLimit => 429, Scenario::Overloaded => 503, Scenario::ServerError => 500,
            Scenario::Unauthorized => 401, Scenario::Forbidden => 403, Scenario::NotFound | Scenario::CacheMiss => 404,
            Scenario::ContextOverflow => 400, _ => 200,
        };
        json!({"case":scenario,"required":true,"synthetic":true,"http_status":status,
            "protocol_status_overrides":if *scenario == Scenario::Overloaded {json!({"anthropic_messages":529})} else {json!({})},
            "preconditions":preconditions})
    }).collect();
    json!({"cases":cases,"coverage":"seeded order, repeated per session; ineligible cases are recorded as skipped",
        "fidelity":"Synthetic protocol fixtures do not measure real model quality, latency, billing or provider safety behavior",
        "continuation":"The final input tool result receives text without advancing the main sequence",
        "override":"An explicit scenario overrides fixture selection without advancing the main sequence"})
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "openai/openai_chat/test";

    fn engine(suite: Suite, script: Option<Script>) -> Engine {
        Engine::new(42, suite, script, 8, Duration::from_secs(60))
    }

    fn script(repeat: bool) -> Option<Script> {
        Some(Script {
            steps: [Scenario::RateLimit, Scenario::ToolCall, Scenario::Refusal]
                .into_iter()
                .map(|scenario| Fixture {
                    scenario,
                    ..Fixture::default()
                })
                .collect(),
            repeat,
        })
    }

    fn begin(engine: &Engine, id: &str) -> Turn {
        engine.begin(id, KEY, &json!({}), false, None).unwrap()
    }

    fn accept(mut turn: Turn, value: Value) -> Scenario {
        turn.accept(value, 200).unwrap();
        turn.fixture.scenario
    }

    #[test]
    fn deterministic_order_ids_and_isolated_sessions() {
        let first = engine(Suite::Coverage, None);
        let second = engine(Suite::Coverage, None);
        assert_eq!(
            first.report(None)["coverage_order"],
            second.report(None)["coverage_order"]
        );
        assert_ne!(
            first.report(None)["coverage_order"],
            Engine::new(43, Suite::Coverage, None, 8, Duration::from_secs(60)).report(None)["coverage_order"]
        );
        for _ in 0..30 {
            let left = begin(&first, "a");
            let right = begin(&second, "a");
            assert_eq!(left.fixture.scenario, right.fixture.scenario);
            assert_eq!(left.request_id, right.request_id);
            accept(left, json!({"secret":"private state"}));
            accept(right, Value::Null);
        }
        let isolated = begin(&first, "b");
        assert_eq!(isolated.number, 1);
        assert!(isolated.state.is_null());
        assert_ne!(isolated.request_id, request_id(42, "a", 1));
        let report = first.report(None).to_string();
        assert!(!report.contains("private state"));
    }

    #[test]
    fn accepted_http_errors_advance_rejection_and_cancellation_preserve_sequence() {
        let engine = engine(Suite::Normal, script(false));
        let mut first = begin(&engine, "a");
        let original_id = first.request_id.clone();
        assert_eq!(first.fixture.scenario, Scenario::RateLimit);
        first.reject(&ApiError::invalid("messages", "private rejected body"));
        drop(first);
        let first = begin(&engine, "a");
        assert_eq!(first.request_id, original_id);
        drop(first);
        let mut first = begin(&engine, "a");
        first.accept(json!({"previous":true}), 429).unwrap();
        drop(first);
        let second = begin(&engine, "a");
        assert_eq!(second.fixture.scenario, Scenario::ToolCall);
        assert_eq!(second.number, 2);
        assert_eq!(second.state, json!({"previous":true}));
        drop(second);
        let report = engine.report(Some("a"));
        assert_eq!(report["sessions"][0]["accepted"], 1);
        assert_eq!(report["sessions"][0]["rejected"], 1);
        assert_eq!(report["sessions"][0]["canceled"], 2);
        assert!(!report.to_string().contains("private rejected body"));
    }

    #[test]
    fn script_repeat_exhaustion_overrides_and_continuations() {
        let engine = engine(Suite::Normal, script(false));
        let override_turn = engine
            .begin("a", KEY, &json!({}), false, Some(Scenario::Empty))
            .unwrap();
        accept(override_turn, Value::Null);
        let continuation = engine
            .begin(
                "a",
                KEY,
                &json!({"messages":[{"role":"tool","content":"done"}]}),
                false,
                None,
            )
            .unwrap();
        assert_eq!(continuation.fixture.scenario, Scenario::Text);
        accept(continuation, Value::Null);
        for scenario in [
            Scenario::RateLimit,
            Scenario::ToolCall,
            Scenario::Refusal,
            Scenario::Text,
            Scenario::Text,
        ] {
            assert_eq!(accept(begin(&engine, "a"), Value::Null), scenario);
        }
        assert_eq!(engine.report(Some("a"))["sessions"][0]["steps"], 3);
        let repeated = self::engine(Suite::Normal, script(true));
        for scenario in [
            Scenario::RateLimit,
            Scenario::ToolCall,
            Scenario::Refusal,
            Scenario::RateLimit,
        ] {
            assert_eq!(accept(begin(&repeated, "a"), Value::Null), scenario);
        }
    }

    #[test]
    fn only_final_input_tool_results_trigger_continuation() {
        let cases = [
            (
                "openai_chat",
                "messages",
                json!({"role":"tool","content":"done"}),
                json!({"role":"user","content":"again"}),
            ),
            (
                "openai_responses",
                "input",
                json!({"type":"function_call_output","output":"done"}),
                json!({"role":"user","content":"again"}),
            ),
            (
                "anthropic_messages",
                "messages",
                json!({"role":"user","content":[{"type":"text","text":"done"},{"type":"tool_result","content":"result"}]}),
                json!({"role":"user","content":"again"}),
            ),
            (
                "gemini_generate",
                "contents",
                json!({"role":"user","parts":[{"text":"done"},{"functionResponse":{"name":"tool"}}]}),
                json!({"role":"user","parts":[{"text":"again"}]}),
            ),
            (
                "gemini_generate",
                "contents",
                json!({"parts":[{"function_response":{"name":"tool"}}]}),
                json!({"parts":[{"text":"again"}]}),
            ),
        ];
        for (protocol, field, tool, next) in cases {
            let key = format!("provider/{protocol}/model");
            assert!(latest_tool_result(&key, &json!({field:[tool.clone()]})));
            assert!(!latest_tool_result(&key, &json!({field:[tool,next]})));
        }
        assert!(!latest_tool_result(
            "provider/openai_responses/model",
            &json!({"input":"hello"})
        ));
        assert!(!latest_tool_result(
            KEY,
            &json!({"messages":[{"role":"user","content":"new turn"}],"input":[{"type":"function_call_output"}]})
        ));
    }

    #[test]
    fn coverage_skips_are_committed_only_with_accepted_turns() {
        let engine = engine(Suite::Coverage, None);
        engine.lock().order = vec![
            Scenario::StreamError,
            Scenario::ToolCall,
            Scenario::Structured,
            Scenario::StopSequence,
            Scenario::Text,
        ];
        let mut rejected = begin(&engine, "a");
        assert_eq!(rejected.fixture.scenario, Scenario::Text);
        rejected.reject(&ApiError::invalid("messages", "invalid"));
        drop(rejected);
        assert!(engine.lock().sessions["a"].skipped.is_empty());
        accept(begin(&engine, "a"), Value::Null);
        let report = engine.report(Some("a"));
        let cases = report["sessions"][0]["cases"].as_array().unwrap();
        assert!(cases.iter().any(|case| case["case"] == "stream_error"
            && case["skipped"] == 1
            && case["status"] == "skipped"));
        let mut stream = engine.begin("a", KEY, &json!({}), true, None).unwrap();
        assert_eq!(stream.fixture.scenario, Scenario::StreamError);
        stream.accept(Value::Null, 200).unwrap();
    }

    #[test]
    fn gateway_model_names_do_not_change_protocol_preconditions() {
        let gateway = "openrouter/openai_chat/anthropic/claude";
        let anthropic = "anthropic/anthropic_messages/claude";
        let body = json!({"reasoning":{"effort":"high"},"tools":[{"type":"function","function":{"name":"read"}}]});
        assert_eq!(ineligible(Scenario::Reasoning, gateway, &body, false), None);
        assert_eq!(
            ineligible(Scenario::ParallelTools, gateway, &body, false),
            None
        );
        assert!(ineligible(Scenario::Reasoning, anthropic, &body, false).is_some());
        assert!(ineligible(Scenario::ParallelTools, anthropic, &body, false).is_some());
        let forced = json!({"tool_choice":"required"});
        assert!(
            ineligible(
                Scenario::Refusal,
                "openrouter/openai_chat/google/gemini",
                &forced,
                false
            )
            .is_some()
        );
        assert_eq!(
            ineligible(
                Scenario::Refusal,
                "google/gemini_generate/gemini",
                &forced,
                false
            ),
            None
        );
    }

    #[test]
    fn state_is_isolated_by_key_and_capacity_failures_are_atomic() {
        let engine = engine(Suite::Normal, None);
        for index in 0..MAX_KEYS {
            let key = format!("provider/openai_chat/{index}");
            let turn = engine.begin("a", &key, &json!({}), false, None).unwrap();
            assert!(turn.state.is_null());
            accept(turn, json!({"value":index}));
        }
        let mut extra = begin(&engine, "a");
        assert_eq!(
            extra.accept(Value::Null, 200).unwrap_err().code,
            "mock_state_capacity"
        );
        drop(extra);
        let mut first = engine
            .begin("a", "provider/openai_chat/0", &json!({}), false, None)
            .unwrap();
        assert_eq!(first.state, json!({"value":0}));
        assert_eq!(first.number, 33);
        assert_eq!(
            first
                .accept(json!("x".repeat(MAX_STATE_BYTES)), 200)
                .unwrap_err()
                .code,
            "mock_state_capacity"
        );
        drop(first);
        let previous = engine
            .begin("a", "provider/openai_chat/0", &json!({}), false, None)
            .unwrap();
        assert_eq!(previous.state, json!({"value":0}));
        assert_eq!(previous.number, 33);
    }

    #[test]
    fn aggregate_state_byte_limit_and_ledger_boundaries() {
        let engine = engine(Suite::Normal, None);
        let overhead = 2 + serde_json::to_vec(KEY).unwrap().len() + 1 + 2;
        accept(
            begin(&engine, "a"),
            json!("s".repeat(MAX_STATE_BYTES - overhead)),
        );
        assert_eq!(engine.lock().sessions["a"].state_bytes, MAX_STATE_BYTES);
        let mut larger = begin(&engine, "a");
        assert!(
            larger
                .accept(json!("s".repeat(MAX_STATE_BYTES - overhead + 1)), 200)
                .is_err()
        );
        drop(larger);
        let mut other = engine
            .begin("a", "another/model", &json!({}), false, None)
            .unwrap();
        assert!(other.accept(Value::Null, 200).is_err());
        drop(other);
        for _ in 0..MAX_LEDGER + 5 {
            accept(begin(&engine, "b"), Value::Null);
        }
        let report = engine.report(Some("b"));
        assert_eq!(
            report["sessions"][0]["ledger"].as_array().unwrap().len(),
            MAX_LEDGER
        );
        assert_eq!(report["sessions"][0]["ledger"][0]["turn"], 6);
    }

    #[test]
    fn ttl_expires_only_idle_sessions_and_reset_releases_capacity() {
        let engine = Engine::new(0, Suite::Normal, None, 1, Duration::from_secs(1));
        let active = begin(&engine, "a");
        engine.lock().sessions.get_mut("a").unwrap().last_used =
            Instant::now() - Duration::from_secs(2);
        assert_eq!(
            engine
                .begin("b", KEY, &json!({}), false, None)
                .err()
                .unwrap()
                .code,
            "mock_session_capacity"
        );
        assert_eq!(engine.reset("a").unwrap_err().code, "mock_session_busy");
        drop(active);
        engine.lock().sessions.get_mut("a").unwrap().last_used =
            Instant::now() - Duration::from_secs(2);
        let replacement = begin(&engine, "b");
        assert_eq!(replacement.number, 1);
        drop(replacement);
        assert_eq!(engine.report(None)["session_count"], 1);
        engine.reset("b").unwrap();
        assert_eq!(engine.report(None)["session_count"], 0);
        assert!(
            engine
                .begin("bad session", KEY, &json!({}), false, None)
                .is_err()
        );
        assert!(engine.begin("世界", KEY, &json!({}), false, None).is_err());
    }

    #[tokio::test]
    async fn canceling_suspended_stream_releases_guard_without_holding_a_mutex() {
        let engine = engine(Suite::Normal, script(false));
        let turn = begin(&engine, "a");
        let task = tokio::spawn(async move {
            let _turn = turn;
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        assert!(engine.0.try_lock().is_ok());
        assert_eq!(
            engine
                .begin("a", KEY, &json!({}), false, None)
                .err()
                .unwrap()
                .code,
            "mock_session_busy"
        );
        accept(begin(&engine, "b"), Value::Null);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let resumed = begin(&engine, "a");
        assert_eq!(resumed.fixture.scenario, Scenario::RateLimit);
        assert_eq!(resumed.number, 1);
        drop(resumed);
        let mut accepted = begin(&engine, "a");
        accepted.accept(Value::Null, 429).unwrap();
        assert!(engine.begin("a", KEY, &json!({}), false, None).is_err());
        drop(accepted);
        assert_eq!(begin(&engine, "a").fixture.scenario, Scenario::ToolCall);
    }
}
