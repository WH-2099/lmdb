use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::model::{ApiError, Frame, Protocol, RequestContext, Scenario, WireResponse};

// Wire schemas: https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create
// Responses events: https://developers.openai.com/api/reference/resources/responses/streaming-events
fn responses(ctx: &RequestContext<'_>) -> bool {
    ctx.protocol == Protocol::OpenaiResponses
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str, ApiError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::invalid(name, format!("{name} must be a nonempty string")))
}

fn invalid(param: &str, message: &str) -> ApiError {
    ApiError::invalid(param, message)
}

fn positive(body: &Value, key: &str) -> Result<(), ApiError> {
    if let Some(value) = body.get(key).filter(|v| !v.is_null())
        && value.as_u64().is_none_or(|n| n == 0)
    {
        return Err(invalid(key, "Expected a positive integer"));
    }
    Ok(())
}

fn range(body: &Value, key: &str, min: f64, max: f64) -> Result<(), ApiError> {
    if let Some(value) = body.get(key).filter(|v| !v.is_null())
        && value.as_f64().is_none_or(|n| n < min || n > max)
    {
        return Err(ApiError::invalid(
            key,
            format!("{key} must be between {min} and {max}"),
        ));
    }
    Ok(())
}

fn capability(ctx: &RequestContext<'_>, modality: &str, param: &str) -> Result<(), ApiError> {
    if !ctx.model.input_modalities.is_empty()
        && !ctx.model.input_modalities.iter().any(|m| m == modality)
    {
        return Err(ApiError::invalid(
            param,
            format!("Model {} does not support {modality} input", ctx.model.id),
        ));
    }
    Ok(())
}

fn content(
    ctx: &RequestContext<'_>,
    value: &Value,
    role: &str,
    response: bool,
) -> Result<(), ApiError> {
    if value.is_string() {
        return capability(ctx, "text", "content");
    }
    let parts = value.as_array().ok_or_else(|| {
        invalid(
            "content",
            "Content must be a string or an array of content parts",
        )
    })?;
    for part in parts {
        let kind = field(part, "type")?;
        let text_kind = if response { "input_text" } else { "text" };
        if !response
            && ctx.provider == "openai"
            && matches!(role, "system" | "developer" | "tool")
            && kind != "text"
        {
            return Err(invalid(
                "content",
                "System, developer and tool messages accept text content only",
            ));
        }
        match kind {
            k if k == text_kind || (response && kind == "output_text" && role == "assistant") => {
                if !part.get("text").is_some_and(Value::is_string) {
                    return Err(invalid("content.text", "Text parts require a text string"));
                }
                capability(ctx, "text", "content")?;
            }
            "refusal" if role == "assistant" => {
                field(part, "refusal")?;
            }
            "image_url" if !response && role == "user" => {
                capability(ctx, "image", "content")?;
                let image = &part["image_url"];
                field(image, "url")?;
                if image.get("detail").is_some_and(|v| {
                    !matches!(v.as_str(), Some("auto" | "low" | "high" | "original"))
                }) {
                    return Err(invalid("image_url.detail", "Unsupported image detail"));
                }
            }
            "input_image" if response => {
                capability(ctx, "image", "content")?;
                if !["image_url", "file_id"].iter().any(|k| {
                    part.get(k)
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.is_empty())
                }) {
                    return Err(invalid(
                        "content",
                        "input_image requires image_url or file_id",
                    ));
                }
            }
            "file" if !response && role == "user" => {
                if !["file_id", "file_data"].iter().any(|k| {
                    part["file"]
                        .get(k)
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.is_empty())
                }) {
                    return Err(invalid(
                        "content.file",
                        "File parts require file_id or file_data",
                    ));
                }
            }
            "input_file" if response => {
                if !["file_id", "file_data", "file_url"].iter().any(|k| {
                    part.get(k)
                        .and_then(Value::as_str)
                        .is_some_and(|s| !s.is_empty())
                }) {
                    return Err(invalid(
                        "content",
                        "input_file requires file_id, file_data or file_url",
                    ));
                }
            }
            "input_audio" => {
                capability(ctx, "audio", "content")?;
                if ctx.provider == "openai" {
                    return Err(ApiError::unsupported(
                        "The mock does not implement OpenAI audio input",
                    ));
                }
            }
            "video_url" if ctx.provider != "openai" => {
                capability(ctx, "video", "content")?;
            }
            _ if ctx.provider != "openai" => {}
            _ => {
                return Err(ApiError::invalid(
                    "content.type",
                    format!("Unsupported content type {kind} for role {role}"),
                ));
            }
        }
    }
    if !response
        && role == "assistant"
        && ctx.provider == "openai"
        && parts.iter().any(|v| v["type"] == "refusal")
        && parts.len() != 1
    {
        return Err(invalid(
            "content",
            "An assistant refusal must be its sole content part",
        ));
    }
    Ok(())
}

fn tools<'a>(ctx: &RequestContext<'_>, body: &'a Value) -> Result<Vec<&'a Value>, ApiError> {
    let Some(value) = body.get("tools") else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| invalid("tools", "tools must be an array"))?;
    let mut names = BTreeSet::new();
    let mut result = Vec::new();
    for tool in list {
        if tool["type"] != "function" {
            return Err(ApiError::unsupported(
                "The mock implements function tools; hosted and custom tools are not implemented",
            ));
        }
        let function = if responses(ctx) {
            tool
        } else {
            &tool["function"]
        };
        if function
            .get("strict")
            .is_some_and(|v| !v.is_null() && !v.is_boolean())
        {
            return Err(invalid("tools.strict", "strict must be a boolean"));
        }
        let name = field(function, "name")?;
        if ctx.provider == "openai"
            && (name.len() > 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
        {
            return Err(invalid(
                "tools.name",
                "Function names require 1 to 64 letters, digits, underscores or hyphens",
            ));
        }
        if !names.insert(name) {
            return Err(invalid("tools", "Function names must be unique"));
        }
        if let Some(schema) = function.get("parameters").filter(|v| !v.is_null()) {
            if !schema.is_object() {
                return Err(invalid(
                    "tools.parameters",
                    "Function parameters must be a JSON Schema object",
                ));
            }
            super::schema::validate(schema)?;
            if ctx.provider == "openai" && function["strict"] == true {
                strict_schema(schema)?;
            }
        }
        result.push(function);
    }
    Ok(result)
}

// https://developers.openai.com/api/docs/guides/function-calling#strict-mode
fn strict_schema(schema: &Value) -> Result<(), ApiError> {
    if schema["type"] == "object" || schema.get("properties").is_some() {
        if schema["additionalProperties"] != false {
            return Err(invalid(
                "tools.parameters",
                "Strict object schemas require additionalProperties:false",
            ));
        }
        if let Some(props) = schema["properties"].as_object() {
            let required = schema["required"].as_array();
            for (name, child) in props {
                if !required.is_some_and(|r| r.iter().any(|v| v.as_str() == Some(name))) {
                    return Err(invalid(
                        "tools.parameters",
                        "Strict schemas require every property in required",
                    ));
                }
                strict_schema(child)?;
            }
        }
    }
    if let Some(items) = schema.get("items") {
        strict_schema(items)?;
    }
    for keyword in ["anyOf", "oneOf", "allOf"] {
        if let Some(children) = schema[keyword].as_array() {
            for child in children {
                strict_schema(child)?;
            }
        }
    }
    if let Some(defs) = schema["$defs"].as_object() {
        for child in defs.values() {
            strict_schema(child)?;
        }
    }
    Ok(())
}

fn forced_tool<'a>(ctx: &RequestContext<'_>, body: &'a Value) -> Option<&'a str> {
    if responses(ctx) {
        body["tool_choice"]["name"].as_str()
    } else {
        body["tool_choice"]["function"]["name"].as_str()
    }
}

fn model_rules(ctx: &RequestContext<'_>, body: &Value, has_tools: bool) -> Result<(), ApiError> {
    let effort = if responses(ctx) {
        body.pointer("/reasoning/effort")
    } else {
        body.get("reasoning_effort")
    };
    let setting = &ctx.model.behavior;
    let default_effort = setting
        .get("default_reasoning_effort")
        .and_then(Value::as_str)
        .unwrap_or("medium");
    let effective = effort.and_then(Value::as_str).unwrap_or(default_effort);
    if let Some(effort) = effort.filter(|v| !v.is_null()) {
        if !effort.is_string() {
            return Err(invalid(
                "reasoning_effort",
                "Reasoning effort must be a string",
            ));
        }
        if ctx.model.reasoning == Some(false) {
            return Err(invalid(
                "reasoning_effort",
                "This model does not support reasoning",
            ));
        }
        if ctx.provider == "openai"
            && !matches!(
                effort.as_str(),
                Some("none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max")
            )
        {
            return Err(invalid("reasoning_effort", "Unsupported reasoning effort"));
        }
        if let Some(allowed) = setting.get("reasoning_efforts").and_then(Value::as_array)
            && !allowed.contains(effort)
        {
            return Err(invalid(
                "reasoning_effort",
                "Reasoning effort is not supported by this model",
            ));
        }
    }
    if has_tools && ctx.model.tool_call == Some(false) {
        return Err(invalid("tools", "This model does not support tool calling"));
    }
    if ctx.provider != "openai" {
        return Ok(());
    }
    // Exact model sets only: https://developers.openai.com/api/docs/guides/latest-model#migration-quickstart
    let id = ctx.model.id.as_str();
    let astra = id == "gpt-6-astra";
    let sol61 = id == "gpt-6.1-sol";
    let sol_luna = matches!(id, "gpt-6-sol" | "gpt-6-luna");
    if (astra || sol61) && effective == "none" || sol61 && effective == "minimal" {
        return Err(invalid(
            "reasoning_effort",
            "This model does not support the requested reasoning effort",
        ));
    }
    if !responses(ctx) && has_tools {
        if astra || sol61 || setting.get("chat_tools") == Some(&Value::Bool(false)) {
            return Err(invalid(
                "tools",
                "This model requires the Responses API for tool calling",
            ));
        }
        if (sol_luna
            || setting.get("chat_tools_requires_reasoning_none") == Some(&Value::Bool(true)))
            && effective != "none"
        {
            return Err(invalid(
                "reasoning_effort",
                "Chat tool calling with this model requires reasoning_effort:none",
            ));
        }
    }
    let sampling_limited = astra
        || sol61
        || sol_luna
        || setting.get("sampling_requires_reasoning_none") == Some(&Value::Bool(true));
    if sampling_limited && effective != "none" {
        for param in ["temperature", "top_p", "top_logprobs"] {
            if body.get(param).is_some_and(|v| !v.is_null()) {
                return Err(ApiError::invalid(
                    param,
                    "Sampling parameters are unsupported while reasoning is enabled",
                ));
            }
        }
        if !responses(ctx) && body.get("logprobs").is_some_and(|v| !v.is_null()) {
            return Err(invalid(
                "logprobs",
                "logprobs is unsupported while reasoning is enabled",
            ));
        }
        if body["include"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "message.output_text.logprobs"))
        {
            return Err(invalid(
                "include",
                "Output log probabilities are unsupported while reasoning is enabled",
            ));
        }
    }
    if matches!(
        id,
        "o1" | "o1-mini" | "o1-preview" | "o3" | "o3-mini" | "o4-mini"
    ) && body.get("max_tokens").is_some()
    {
        return Err(invalid(
            "max_tokens",
            "Use max_completion_tokens with this model",
        ));
    }
    if matches!(id, "o3" | "o4-mini") && body.get("stop").is_some_and(|v| !v.is_null()) {
        return Err(invalid(
            "stop",
            "This model does not support stop sequences",
        ));
    }
    if let Some(params) = setting
        .get("unsupported_parameters")
        .and_then(Value::as_array)
    {
        for name in params.iter().filter_map(Value::as_str) {
            if body.get(name).is_some_and(|v| !v.is_null()) {
                return Err(ApiError::invalid(
                    name,
                    "Parameter is unsupported by this model",
                ));
            }
        }
    }
    Ok(())
}

fn known_call(state: &Value, id: &str, name: &str, arguments: &str) -> Result<(), ApiError> {
    if let Some(call) = state.pointer("/openai/calls").and_then(|v| v.get(id))
        && (call["name"] != name || call["arguments"] != arguments)
    {
        return Err(invalid(
            "tool_calls",
            "A returned tool call ID was replayed with a different name or arguments",
        ));
    }
    Ok(())
}

fn validate_chat(ctx: &RequestContext<'_>, body: &Value, state: &Value) -> Result<(), ApiError> {
    let messages = body["messages"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| invalid("messages", "messages must be a nonempty array"))?;
    let mut pending = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for message in messages {
        let role = field(message, "role")?;
        if !matches!(
            role,
            "system" | "developer" | "user" | "assistant" | "tool" | "function"
        ) {
            return Err(invalid("messages.role", "Unsupported message role"));
        }
        if role == "function" || message.get("function_call").is_some() {
            return Err(ApiError::unsupported(
                "Deprecated function_call messages are not implemented; use tool_calls",
            ));
        }
        if !pending.is_empty() && role != "tool" {
            return Err(invalid(
                "messages",
                "Every assistant tool call requires a matching tool response before the next message",
            ));
        }
        if role == "tool" {
            let id = field(message, "tool_call_id")?;
            if !pending.remove(id) {
                return Err(invalid(
                    "tool_call_id",
                    "Tool result has no unmatched preceding assistant call",
                ));
            }
        }
        if let Some(calls) = message.get("tool_calls") {
            if role != "assistant" {
                return Err(invalid(
                    "tool_calls",
                    "Only assistant messages can contain tool calls",
                ));
            }
            let calls = calls
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or_else(|| invalid("tool_calls", "tool_calls must be a nonempty array"))?;
            for call in calls {
                if call["type"] != "function" {
                    return Err(ApiError::unsupported(
                        "Only function tool call history is implemented",
                    ));
                }
                let id = field(call, "id")?;
                let name = field(&call["function"], "name")?;
                let arguments = call["function"]["arguments"].as_str().ok_or_else(|| {
                    invalid(
                        "tool_calls.arguments",
                        "Tool call arguments must be an encoded JSON string",
                    )
                })?;
                if !ids.insert(id) {
                    return Err(invalid("tool_calls.id", "Tool call IDs must be unique"));
                }
                known_call(state, id, name, arguments)?;
                pending.insert(id);
            }
        }
        if let Some(value) = message.get("content").filter(|v| !v.is_null()) {
            content(ctx, value, role, false)?;
        } else if role != "assistant"
            || (message.get("tool_calls").is_none() && message.get("content").is_none())
        {
            return Err(invalid(
                "messages.content",
                "Message content is required unless the assistant makes tool calls",
            ));
        }
    }
    if !pending.is_empty() {
        return Err(invalid(
            "messages",
            "Missing tool results for assistant tool calls",
        ));
    }
    Ok(())
}

fn previous<'a>(body: &Value, state: &'a Value) -> Result<Option<&'a Value>, ApiError> {
    let Some(id) = body.get("previous_response_id").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let id = id.as_str().filter(|s| !s.is_empty()).ok_or_else(|| {
        invalid(
            "previous_response_id",
            "previous_response_id must be a nonempty string",
        )
    })?;
    state
        .pointer("/openai/responses")
        .and_then(|r| r.get(id))
        .map(Some)
        .ok_or_else(|| {
            ApiError::new(
                404,
                "response_not_found",
                "Previous response was not found in this mock session",
            )
        })
}

fn response_pending(body: &Value, state: &Value) -> Result<BTreeSet<String>, ApiError> {
    Ok(previous(body, state)?
        .and_then(|r| r["pending"].as_array())
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default())
}

fn validate_responses(
    ctx: &RequestContext<'_>,
    body: &Value,
    state: &Value,
) -> Result<(), ApiError> {
    let mut pending = response_pending(body, state)?;
    if body.get("conversation").is_some_and(|v| !v.is_null()) {
        return Err(ApiError::unsupported(
            "Conversation resources are not implemented; use previous_response_id",
        ));
    }
    if body["background"] == true {
        if ctx.provider == "meta" && ctx.stream {
            return Err(invalid(
                "background",
                "Meta Responses cannot combine background:true with stream:true",
            ));
        }
        return Err(ApiError::unsupported(
            "Background response polling is not implemented",
        ));
    }
    if let Some(instructions) = body.get("instructions").filter(|v| !v.is_null())
        && !instructions.is_string()
    {
        return Err(invalid("instructions", "instructions must be a string"));
    }
    let Some(input) = body.get("input") else {
        if body.get("instructions").is_none() && body.get("previous_response_id").is_none() {
            return Err(invalid(
                "input",
                "Provide input, instructions or previous_response_id",
            ));
        }
        if !pending.is_empty() {
            return Err(invalid("input", "Missing function call outputs"));
        }
        return Ok(());
    };
    if input.is_string() {
        capability(ctx, "text", "input")?;
    } else {
        let items = input
            .as_array()
            .ok_or_else(|| invalid("input", "input must be a string or an array"))?;
        let mut calls = BTreeSet::new();
        for item in items {
            match item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("message")
            {
                "message" => {
                    let role = field(item, "role")?;
                    if !matches!(role, "user" | "system" | "developer" | "assistant") {
                        return Err(invalid("input.role", "Unsupported Responses message role"));
                    }
                    content(ctx, &item["content"], role, true)?;
                }
                "function_call" => {
                    let id = field(item, "call_id")?;
                    let name = field(item, "name")?;
                    let args = item["arguments"].as_str().ok_or_else(|| {
                        invalid(
                            "arguments",
                            "Function arguments must be an encoded JSON string",
                        )
                    })?;
                    if !calls.insert(id) || !pending.insert(id.to_owned()) {
                        return Err(invalid("call_id", "Duplicate function call ID"));
                    }
                    known_call(state, id, name, args)?;
                }
                "function_call_output" => {
                    let id = field(item, "call_id")?;
                    if !pending.remove(id) {
                        return Err(invalid(
                            "call_id",
                            "Function output does not match an outstanding call",
                        ));
                    }
                    if !item["output"].is_string() && !item["output"].is_array() {
                        return Err(invalid(
                            "output",
                            "Function output must be a string or content array",
                        ));
                    }
                    if let Some(parts) = item["output"].as_array() {
                        content(ctx, &Value::Array(parts.clone()), "user", true)?;
                    }
                }
                "reasoning" => {
                    if ctx.provider == "meta" && item.get("id").is_none() {
                        continue;
                    }
                    let id = field(item, "id")?;
                    if let Some(known) = state.pointer("/openai/items").and_then(|v| v.get(id))
                        && known.get("encrypted_content").is_some()
                        && item.get("encrypted_content") != known.get("encrypted_content")
                    {
                        return Err(invalid(
                            "encrypted_content",
                            "Returned reasoning content must be replayed unchanged",
                        ));
                    }
                }
                other => {
                    return Err(ApiError::unsupported(format!(
                        "Responses input item {other} is not implemented"
                    )));
                }
            }
        }
    }
    if !pending.is_empty() {
        return Err(invalid("input", "Missing function call outputs"));
    }
    Ok(())
}

pub fn validate(ctx: &RequestContext<'_>, body: &Value, state: &Value) -> Result<(), ApiError> {
    if !body.is_object() {
        return Err(invalid("body", "Request body must be a JSON object"));
    }
    field(body, "model")?;
    if ctx.provider == "openai"
        && let Some(value) = ctx.headers.get("x-client-request-id")
        && (value.as_bytes().len() > 512 || !value.as_bytes().is_ascii())
    {
        return Err(invalid(
            "X-Client-Request-Id",
            "Client request IDs require at most 512 ASCII characters",
        ));
    }
    if responses(ctx)
        && body
            .get("reasoning")
            .is_some_and(|v| !v.is_null() && !v.is_object())
    {
        return Err(invalid("reasoning", "reasoning must be an object"));
    }
    if ctx.provider == "openai"
        && responses(ctx)
        && body.pointer("/reasoning/summary").is_some_and(|v| {
            !v.is_null() && !matches!(v.as_str(), Some("auto" | "concise" | "detailed"))
        })
    {
        return Err(invalid(
            "reasoning.summary",
            "Unsupported reasoning summary setting",
        ));
    }
    for name in ["stream", "parallel_tool_calls", "store", "logprobs"] {
        if body
            .get(name)
            .is_some_and(|v| !v.is_null() && !v.is_boolean())
        {
            return Err(ApiError::invalid(name, "Expected a boolean"));
        }
    }
    for name in [
        "max_tokens",
        "max_completion_tokens",
        "max_output_tokens",
        "n",
    ] {
        positive(body, name)?;
    }
    if let Some(max) = ctx.model.max_output_tokens {
        for name in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
            if body[name].as_u64().is_some_and(|n| n > max) {
                return Err(ApiError::invalid(
                    name,
                    format!("Requested output limit exceeds this model's {max} token maximum"),
                ));
            }
        }
    }
    for (name, min, max) in [
        ("temperature", 0., 2.),
        ("top_p", 0., 1.),
        ("presence_penalty", -2., 2.),
        ("frequency_penalty", -2., 2.),
        ("top_logprobs", 0., 20.),
    ] {
        if ctx.provider == "openai" {
            range(body, name, min, max)?;
        } else if body
            .get(name)
            .is_some_and(|v| !v.is_null() && !v.is_number())
        {
            return Err(ApiError::invalid(name, "Expected a number"));
        }
    }
    if ctx.provider == "openai"
        && body
            .get("top_logprobs")
            .is_some_and(|v| !v.is_null() && v.as_u64().is_none())
    {
        return Err(invalid("top_logprobs", "top_logprobs must be an integer"));
    }
    if !responses(ctx)
        && body.get("top_logprobs").is_some_and(|v| !v.is_null())
        && body["logprobs"] != true
    {
        return Err(invalid(
            "top_logprobs",
            "top_logprobs requires logprobs:true",
        ));
    }
    if ctx.provider == "openai"
        && body.get("stream_options").is_some_and(|v| !v.is_null())
        && !ctx.stream
    {
        return Err(invalid(
            "stream_options",
            "stream_options requires stream:true",
        ));
    }
    if let Some(options) = body.get("stream_options").filter(|v| !v.is_null()) {
        if !options.is_object() {
            return Err(invalid(
                "stream_options",
                "stream_options must be an object",
            ));
        }
        for key in ["include_usage", "include_obfuscation"] {
            if options.get(key).is_some_and(|v| !v.is_boolean()) {
                return Err(ApiError::invalid(key, "Expected a boolean"));
            }
        }
    }
    if let Some(stop) = body.get("stop").filter(|v| !v.is_null())
        && !stop.is_string()
        && !stop.as_array().is_some_and(|a| {
            !a.is_empty()
                && (ctx.provider != "openai" || a.len() <= 4)
                && a.iter().all(Value::is_string)
        })
    {
        return Err(invalid(
            "stop",
            "stop must be a string or one to four strings",
        ));
    }
    if ctx.provider == "openai"
        && (body.get("audio").is_some_and(|v| !v.is_null())
            || body["modalities"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v != "text")))
    {
        return Err(ApiError::unsupported(
            "The mock implements text output; OpenAI audio generation is not implemented",
        ));
    }
    let definitions = tools(ctx, body)?;
    model_rules(ctx, body, !definitions.is_empty())?;
    if let Some(format) = format(ctx, body).filter(|v| !v.is_null()) {
        match format["type"].as_str() {
            Some("text" | "json_object") => {}
            Some("json_schema") => {
                let definition = if responses(ctx) {
                    format
                } else {
                    &format["json_schema"]
                };
                field(definition, "name")?;
                let schema = definition
                    .get("schema")
                    .filter(|v| v.is_object())
                    .ok_or_else(|| {
                        invalid("response_format", "json_schema requires a schema object")
                    })?;
                super::schema::validate(schema)?;
                if ctx.provider == "openai" && definition["strict"] == true {
                    strict_schema(schema)?;
                }
            }
            _ => return Err(invalid("response_format", "Unsupported response format")),
        }
    }
    if let Some(choice) = body.get("tool_choice").filter(|v| !v.is_null()) {
        let allowed = matches!(choice.as_str(), Some("none" | "auto" | "required"))
            || ctx.provider != "openai" && choice == "any";
        if !allowed {
            let name = forced_tool(ctx, body)
                .ok_or_else(|| invalid("tool_choice", "Unsupported tool choice"))?;
            if choice["type"] != "function" || !definitions.iter().any(|t| t["name"] == name) {
                return Err(invalid(
                    "tool_choice",
                    "Forced function is absent from tools",
                ));
            }
        }
        if definitions.is_empty() && choice != "none" && choice != "auto" {
            return Err(invalid(
                "tool_choice",
                "Tool choice requires a tool definition",
            ));
        }
    }
    if ctx.fixture.scenario == Scenario::Reasoning && ctx.model.reasoning == Some(false) {
        return Err(invalid(
            "scenario",
            "Reasoning fixture requires a model with reasoning support",
        ));
    }
    if format(ctx, body).is_some_and(|f| f["type"] == "json_schema")
        && ctx.model.structured_output == Some(false)
    {
        return Err(invalid(
            "response_format",
            "This model does not support structured outputs",
        ));
    }
    if responses(ctx) {
        validate_responses(ctx, body, state)
    } else {
        validate_chat(ctx, body, state)
    }
}

fn format<'a>(ctx: &RequestContext<'_>, body: &'a Value) -> Option<&'a Value> {
    if responses(ctx) {
        body.pointer("/text/format")
    } else {
        body.get("response_format")
    }
}

fn recent_tool_result(ctx: &RequestContext<'_>, body: &Value) -> bool {
    let list = if responses(ctx) {
        body["input"].as_array()
    } else {
        body["messages"].as_array()
    };
    list.and_then(|a| a.last()).is_some_and(|v| {
        if responses(ctx) {
            v["type"] == "function_call_output"
        } else {
            v["role"] == "tool"
        }
    })
}

fn output_calls(ctx: &RequestContext<'_>, body: &Value) -> Result<Vec<Value>, ApiError> {
    if ctx.fixture.scenario == Scenario::Refusal {
        return Ok(Vec::new());
    }
    let fixture_tools = matches!(
        ctx.fixture.scenario,
        Scenario::ToolCall | Scenario::ParallelTools | Scenario::MalformedToolArguments
    );
    let forced = forced_tool(ctx, body);
    let required = matches!(body["tool_choice"].as_str(), Some("required" | "any"));
    if !(forced.is_some() || required || fixture_tools && !recent_tool_result(ctx, body)) {
        return Ok(Vec::new());
    }
    if body["tool_choice"] == "none" {
        return Err(invalid(
            "tool_choice",
            "The tool-call fixture conflicts with tool_choice:none",
        ));
    }
    let definitions = tools(ctx, body)?;
    if definitions.is_empty() {
        return Err(invalid(
            "tools",
            "The tool-call fixture requires a function definition",
        ));
    }
    let selected: Vec<_> = definitions
        .into_iter()
        .filter(|t| forced.is_none_or(|n| t["name"] == n))
        .collect();
    if selected.is_empty() {
        return Err(invalid(
            "tool_choice",
            "Forced function is absent from tools",
        ));
    }
    let parallel = ctx.fixture.scenario == Scenario::ParallelTools
        && body["parallel_tool_calls"] != false
        && forced.is_none();
    let count = if parallel { selected.len().max(2) } else { 1 };
    let mut calls = Vec::new();
    let mut remaining =
        output_budget(ctx, body).saturating_sub(reasoning_text(ctx, body).chars().count() as u64);
    for index in 0..count {
        let function = selected[index % selected.len()];
        let mut arguments = if ctx.fixture.scenario == Scenario::MalformedToolArguments {
            "{\"mock\":\"unfinished".to_owned()
        } else {
            let empty = json!({"type":"object","properties":{},"additionalProperties":false});
            let schema = function
                .get("parameters")
                .filter(|v| !v.is_null())
                .unwrap_or(&empty);
            super::schema::sample(schema, ctx.fixture.tool_arguments.as_ref())?.to_string()
        };
        let mut truncated = false;
        let count = arguments.chars().count() as u64;
        if count > remaining {
            arguments = arguments.chars().take(remaining as usize).collect();
            truncated = true;
        }
        remaining = remaining.saturating_sub(count);
        if ctx.fixture.scenario == Scenario::Truncated {
            arguments = arguments
                .chars()
                .take(arguments.chars().count().div_ceil(2))
                .collect();
            truncated = true;
        }
        calls.push(json!({"id":format!("call_{}_{}",ctx.request_id,index),"name":function["name"],"arguments":arguments,"truncated":truncated}));
    }
    Ok(calls)
}

fn text_output(ctx: &RequestContext<'_>, body: &Value) -> Result<String, ApiError> {
    if ctx.fixture.scenario == Scenario::Empty {
        return Ok(String::new());
    }
    if let Some(format) = format(ctx, body).filter(|v| !v.is_null()) {
        match format["type"].as_str() {
            Some("text") => {}
            Some("json_object") => {
                if ctx.provider == "cohere"
                    && let Some(schema) = format.get("schema")
                {
                    return Ok(
                        super::schema::sample(schema, ctx.fixture.output.as_ref())?.to_string()
                    );
                }
                let value = ctx
                    .fixture
                    .output
                    .clone()
                    .unwrap_or_else(|| json!({"mock":ctx.fixture.text}));
                if !value.is_object() {
                    return Err(invalid(
                        "fixture.output",
                        "JSON object format requires an object fixture",
                    ));
                }
                return Ok(value.to_string());
            }
            Some("json_schema") => {
                let definition = if responses(ctx) {
                    format
                } else {
                    &format["json_schema"]
                };
                field(definition, "name")?;
                let schema = definition
                    .get("schema")
                    .ok_or_else(|| invalid("response_format", "json_schema requires a schema"))?;
                return Ok(super::schema::sample(schema, ctx.fixture.output.as_ref())?.to_string());
            }
            _ => return Err(invalid("response_format", "Unsupported response format")),
        }
    }
    if ctx.fixture.scenario == Scenario::Structured {
        return Ok(ctx
            .fixture
            .output
            .clone()
            .unwrap_or_else(|| json!({"mock":ctx.fixture.text}))
            .to_string());
    }
    Ok(match &ctx.fixture.output {
        Some(Value::String(s)) => s.clone(),
        Some(value) => value.to_string(),
        None => ctx.fixture.text.clone(),
    })
}

fn trim_output(ctx: &RequestContext<'_>, body: &Value, mut text: String) -> (String, bool) {
    let stops: Vec<&str> = match body.get("stop") {
        Some(Value::String(s)) => vec![s],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if let Some(end) = stops.iter().filter_map(|s| text.find(s)).min() {
        text.truncate(end);
    }
    let reasoning = reasoning_text(ctx, body);
    let max = output_budget(ctx, body).saturating_sub(reasoning.chars().count() as u64);
    let mut truncated =
        ctx.fixture.scenario == Scenario::Reasoning && reasoning != "Mock reasoning summary.";
    if text.chars().count() as u64 > max {
        text = text.chars().take(max as usize).collect();
        truncated = true;
    }
    if ctx.fixture.scenario == Scenario::Truncated {
        text = text
            .chars()
            .take(text.chars().count().div_ceil(2))
            .collect();
        truncated = true;
    }
    (text, truncated)
}

fn fragments(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(7).map(|c| c.iter().collect()).collect()
}

fn output_budget(ctx: &RequestContext<'_>, body: &Value) -> u64 {
    let requested = if responses(ctx) {
        body["max_output_tokens"].as_u64()
    } else {
        body["max_completion_tokens"]
            .as_u64()
            .or_else(|| body["max_tokens"].as_u64())
    };
    requested
        .or(ctx.model.max_output_tokens)
        .unwrap_or(u64::MAX)
}

fn reasoning_text(ctx: &RequestContext<'_>, body: &Value) -> String {
    if ctx.fixture.scenario != Scenario::Reasoning {
        return String::new();
    }
    "Mock reasoning summary."
        .chars()
        .take(output_budget(ctx, body).min(usize::MAX as u64) as usize)
        .collect()
}

fn usage(ctx: &RequestContext<'_>, body: &Value, visible: &str) -> Value {
    // Input is estimated; each output Unicode scalar is one mock token.
    let input = (body.to_string().chars().count() as u64).div_ceil(4).max(1);
    let reasoning = reasoning_text(ctx, body).chars().count() as u64;
    let output = visible.chars().count() as u64 + reasoning;
    if responses(ctx) {
        json!({"input_tokens":input,"input_tokens_details":{"cached_tokens":0},"output_tokens":output,"output_tokens_details":{"reasoning_tokens":reasoning},"total_tokens":input+output})
    } else {
        json!({"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output,"prompt_tokens_details":{"cached_tokens":0,"audio_tokens":0},"completion_tokens_details":{"reasoning_tokens":reasoning,"audio_tokens":0,"accepted_prediction_tokens":0,"rejected_prediction_tokens":0}})
    }
}

fn token_probabilities(body: &Value, text: &str, chat: bool) -> Value {
    Value::Array(
        text.chars()
            .map(|character| {
                let token = character.to_string();
                let mut candidate = json!({"token":token,"logprob":-0.1});
                if chat {
                    candidate["bytes"] = json!(token.as_bytes());
                }
                let mut value = candidate.clone();
                value["top_logprobs"] = if body["top_logprobs"].as_u64().unwrap_or(0) > 0 {
                    json!([candidate])
                } else {
                    json!([])
                };
                value
            })
            .collect(),
    )
}

fn chat_probabilities(body: &Value, text: &str, refusal: bool) -> Value {
    if body["logprobs"] != true {
        return Value::Null;
    }
    let values = token_probabilities(body, text, true);
    if refusal {
        json!({"content":null,"refusal":values})
    } else {
        json!({"content":values,"refusal":null})
    }
}

fn response_probabilities(body: &Value, text: &str) -> Value {
    if body["include"]
        .as_array()
        .is_some_and(|a| a.iter().any(|v| v == "message.output_text.logprobs"))
    {
        token_probabilities(body, text, false)
    } else {
        json!([])
    }
}

fn remember_calls(state: &mut Value, calls: &[Value]) {
    for call in calls {
        state["openai"]["calls"][call["id"].as_str().unwrap()] = call.clone();
    }
}

fn chat_chunk(ctx: &RequestContext<'_>, id: &str, choices: Value, usage: Option<Value>) -> Value {
    let mut chunk = json!({"id":id,"object":"chat.completion.chunk","created":1_700_000_000+ctx.turn,"model":ctx.model.id,"choices":choices});
    if let Some(usage) = usage {
        chunk["usage"] = usage;
    }
    chunk
}

fn chat(
    ctx: &RequestContext<'_>,
    body: &Value,
    state: &mut Value,
) -> Result<WireResponse, ApiError> {
    let id = format!("chatcmpl_{}", ctx.request_id);
    let calls = output_calls(ctx, body)?;
    let refusal = ctx.fixture.scenario == Scenario::Refusal;
    let (text, truncated) = if !calls.is_empty() {
        (
            String::new(),
            ctx.fixture.scenario == Scenario::Reasoning
                && reasoning_text(ctx, body) != "Mock reasoning summary.",
        )
    } else if refusal {
        trim_output(ctx, body, ctx.fixture.text.clone())
    } else {
        trim_output(ctx, body, text_output(ctx, body)?)
    };
    let truncated = truncated || calls.iter().any(|c| c["truncated"] == true);
    let finish = if truncated {
        "length"
    } else if !calls.is_empty() {
        "tool_calls"
    } else {
        "stop"
    };
    let mut message = json!({"role":"assistant","content":text,"refusal":null});
    if refusal {
        message["content"] = Value::Null;
        message["refusal"] = json!(text);
    }
    if !calls.is_empty() {
        message["content"] = Value::Null;
        message["tool_calls"] = Value::Array(calls.iter().map(|c| json!({"id":c["id"],"type":"function","function":{"name":c["name"],"arguments":c["arguments"]}})).collect());
    }
    let reasoning = ctx.fixture.scenario == Scenario::Reasoning && ctx.provider != "openai";
    if reasoning {
        message["reasoning_content"] = json!(reasoning_text(ctx, body));
    }
    let count = body["n"].as_u64().unwrap_or(1);
    if count > 128 {
        return Err(ApiError::unsupported(
            "The mock supports at most 128 chat choices",
        ));
    }
    let visible = if calls.is_empty() {
        text.clone()
    } else {
        calls
            .iter()
            .filter_map(|c| c["arguments"].as_str())
            .collect::<Vec<_>>()
            .join("")
    };
    let mut totals = usage(ctx, body, &visible.repeat(count as usize));
    let reasoning_tokens = totals["completion_tokens_details"]["reasoning_tokens"]
        .as_u64()
        .unwrap_or(0);
    let extra_reasoning = reasoning_tokens * count.saturating_sub(1);
    totals["completion_tokens_details"]["reasoning_tokens"] = json!(reasoning_tokens * count);
    totals["completion_tokens"] =
        json!(totals["completion_tokens"].as_u64().unwrap() + extra_reasoning);
    totals["total_tokens"] = json!(totals["total_tokens"].as_u64().unwrap() + extra_reasoning);
    let probabilities = if calls.is_empty() {
        chat_probabilities(body, &text, refusal)
    } else {
        Value::Null
    };
    let choice_calls: Vec<Vec<Value>> = (0..count)
        .map(|index| {
            calls
                .iter()
                .map(|call| {
                    let mut call = call.clone();
                    if count > 1 {
                        call["id"] =
                            json!(format!("{}_choice_{index}", call["id"].as_str().unwrap()));
                    }
                    call
                })
                .collect()
        })
        .collect();
    for calls in &choice_calls {
        remember_calls(state, calls);
    }
    let choices: Vec<_> = (0..count)
        .map(|index| {
            let mut message = message.clone();
            if let Some(tool_calls) = message["tool_calls"].as_array_mut() {
                for (tool_index, call) in tool_calls.iter_mut().enumerate() {
                    call["id"] = choice_calls[index as usize][tool_index]["id"].clone();
                }
            }
            json!({"index":index,"message":message,"logprobs":probabilities,"finish_reason":finish})
        })
        .collect();
    let complete = json!({"id":id,"object":"chat.completion","created":1_700_000_000+ctx.turn,"model":ctx.model.id,"choices":choices,"usage":totals,"service_tier":"default"});
    if !ctx.stream {
        return Ok(WireResponse::json(complete));
    }
    let include_usage = body.pointer("/stream_options/include_usage") == Some(&Value::Bool(true));
    let mut frames = Vec::new();
    let mut push = |index: u64, delta: Value, finish_reason: Option<&str>| {
        let probabilities = delta
            .get("content")
            .or_else(|| delta.get("refusal"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map_or(Value::Null, |s| {
                chat_probabilities(body, s, delta.get("refusal").is_some())
            });
        frames.push(Frame::data(chat_chunk(
            ctx,
            &id,
            json!([{"index":index,"delta":delta,"logprobs":probabilities,"finish_reason":finish_reason}]),
            include_usage.then_some(Value::Null),
        )));
    };
    for index in 0..count {
        push(index, json!({"role":"assistant","content":""}), None);
        if reasoning {
            push(
                index,
                json!({"reasoning_content":reasoning_text(ctx, body)}),
                None,
            );
        }
        if ctx.fixture.scenario == Scenario::UnknownEvent {
            push(index, json!({"mock_future_field":{"version":1}}), None);
        }
        if !calls.is_empty() {
            for (tool_index, call) in choice_calls[index as usize].iter().enumerate() {
                push(
                    index,
                    json!({"tool_calls":[{"index":tool_index,"id":call["id"],"type":"function","function":{"name":call["name"],"arguments":""}}]}),
                    None,
                );
            }
            let pieces: Vec<_> = calls
                .iter()
                .map(|c| fragments(c["arguments"].as_str().unwrap()))
                .collect();
            for offset in 0..pieces.iter().map(Vec::len).max().unwrap_or(0) {
                for (tool_index, parts) in pieces.iter().enumerate() {
                    if let Some(part) = parts.get(offset) {
                        push(
                            index,
                            json!({"tool_calls":[{"index":tool_index,"function":{"arguments":part}}]}),
                            None,
                        );
                    }
                }
            }
        } else {
            for part in fragments(&text) {
                if refusal {
                    push(index, json!({"refusal":part}), None);
                } else {
                    push(index, json!({"content":part}), None);
                }
                if ctx.fixture.scenario == Scenario::StreamError {
                    break;
                }
            }
        }
        if ctx.fixture.scenario != Scenario::StreamError {
            push(index, json!({}), Some(finish));
        }
    }
    if ctx.fixture.scenario == Scenario::StreamError {
        frames.push(Frame::data(error_body(&ApiError::new(
            500,
            "server_error",
            "Mock stream failed after generation started",
        ))));
    } else {
        if include_usage {
            frames.push(Frame::data(chat_chunk(ctx, &id, json!([]), Some(totals))));
        }
        frames.push(Frame::done());
    }
    let mut wire = WireResponse::sse(frames);
    wire.body = complete;
    Ok(wire)
}

fn response_object(
    ctx: &RequestContext<'_>,
    body: &Value,
    id: &str,
    output: Vec<Value>,
    status: &str,
    totals: Value,
) -> Value {
    json!({"id":id,"object":"response","created_at":1_700_000_000+ctx.turn,"status":status,"completed_at":if status=="in_progress" {Value::Null} else {json!(1_700_000_000+ctx.turn)},"error":null,"incomplete_details":if status=="incomplete" {json!({"reason":"max_output_tokens"})} else {Value::Null},"model":ctx.model.id,"output":output,"usage":totals,"instructions":body.get("instructions"),"max_output_tokens":body.get("max_output_tokens"),"parallel_tool_calls":body.get("parallel_tool_calls").unwrap_or(&Value::Bool(true)),"previous_response_id":body.get("previous_response_id"),"reasoning":body.get("reasoning").cloned().unwrap_or_else(||json!({"effort":null,"summary":null})),"store":body.get("store").unwrap_or(&Value::Bool(true)),"text":body.get("text").cloned().unwrap_or_else(||json!({"format":{"type":"text"}})),"tool_choice":body.get("tool_choice").cloned().unwrap_or_else(||json!("auto")),"tools":body.get("tools").cloned().unwrap_or_else(||json!([])),"temperature":body.get("temperature").cloned().unwrap_or_else(||json!(1.0)),"top_p":body.get("top_p").cloned().unwrap_or_else(||json!(1.0)),"truncation":body.get("truncation").cloned().unwrap_or_else(||json!("disabled")),"metadata":body.get("metadata").cloned().unwrap_or_else(||json!({}))})
}

fn event(frames: &mut Vec<Frame>, name: &str, mut data: Value) {
    data["type"] = json!(name);
    data["sequence_number"] = json!(frames.len());
    frames.push(Frame::named(name, data));
}

fn response(
    ctx: &RequestContext<'_>,
    body: &Value,
    state: &mut Value,
) -> Result<WireResponse, ApiError> {
    let id = format!("resp_{}", ctx.request_id);
    let calls = output_calls(ctx, body)?;
    let refusal = ctx.fixture.scenario == Scenario::Refusal;
    let (text, truncated) = if !calls.is_empty() {
        (
            String::new(),
            ctx.fixture.scenario == Scenario::Reasoning
                && reasoning_text(ctx, body) != "Mock reasoning summary.",
        )
    } else if refusal {
        trim_output(ctx, body, ctx.fixture.text.clone())
    } else {
        trim_output(ctx, body, text_output(ctx, body)?)
    };
    let truncated = truncated || calls.iter().any(|c| c["truncated"] == true);
    let mut output = Vec::new();
    if ctx.fixture.scenario == Scenario::Reasoning {
        let summary = if body
            .pointer("/reasoning/summary")
            .is_some_and(|v| !v.is_null())
        {
            json!([{"type":"summary_text","text":reasoning_text(ctx, body)}])
        } else {
            json!([])
        };
        let mut item =
            json!({"id":format!("rs_{}",ctx.request_id),"type":"reasoning","summary":summary});
        if body["store"] == false {
            item["encrypted_content"] = json!(format!("mock_opaque_{}", ctx.request_id));
        }
        output.push(item);
    }
    if calls.is_empty() {
        if ctx.fixture.scenario != Scenario::Empty {
            let part = if refusal {
                json!({"type":"refusal","refusal":text})
            } else {
                json!({"type":"output_text","text":text,"annotations":[],"logprobs":response_probabilities(body, &text)})
            };
            output.push(json!({"id":format!("msg_{}",ctx.request_id),"type":"message","role":"assistant","status":if truncated {"incomplete"} else {"completed"},"content":[part]}));
        }
    } else {
        for (index, call) in calls.iter().enumerate() {
            output.push(json!({"id":format!("fc_{}_{}",ctx.request_id,index),"type":"function_call","call_id":call["id"],"name":call["name"],"arguments":call["arguments"],"status":if truncated {"incomplete"} else {"completed"}}));
        }
    }
    let visible = if calls.is_empty() {
        text.clone()
    } else {
        calls
            .iter()
            .filter_map(|c| c["arguments"].as_str())
            .collect::<Vec<_>>()
            .join("")
    };
    let totals = usage(ctx, body, &visible);
    let status = if truncated { "incomplete" } else { "completed" };
    let mut result = response_object(ctx, body, &id, output.clone(), status, totals);
    remember_calls(state, &calls);
    for item in &output {
        state["openai"]["items"][item["id"].as_str().unwrap()] = item.clone();
    }
    if body["store"] != false {
        state["openai"]["responses"][&id] = json!({"response":result,"pending":calls.iter().map(|c|c["id"].clone()).collect::<Vec<_>>()});
    }
    if !ctx.stream {
        return Ok(WireResponse::json(result));
    }
    let mut frames = Vec::new();
    let initial = response_object(ctx, body, &id, Vec::new(), "in_progress", Value::Null);
    event(&mut frames, "response.created", json!({"response":initial}));
    event(
        &mut frames,
        "response.in_progress",
        json!({"response":initial}),
    );
    if ctx.fixture.scenario == Scenario::UnknownEvent {
        event(
            &mut frames,
            "response.mock.future",
            json!({"response_id":id,"payload":{"version":1}}),
        );
    }
    for (index, item) in output.iter().enumerate() {
        let item_id = item["id"].as_str().unwrap();
        let mut initial_item = item.clone();
        if item["type"] == "message" {
            initial_item["content"] = json!([]);
            initial_item["status"] = json!("in_progress");
        }
        if item["type"] == "function_call" {
            initial_item["arguments"] = json!("");
            initial_item["status"] = json!("in_progress");
        }
        if item["type"] == "reasoning" {
            initial_item["summary"] = json!([]);
        }
        event(
            &mut frames,
            "response.output_item.added",
            json!({"output_index":index,"item":initial_item}),
        );
        if item["type"] == "function_call" {
            for part in fragments(item["arguments"].as_str().unwrap()) {
                event(
                    &mut frames,
                    "response.function_call_arguments.delta",
                    json!({"item_id":item_id,"output_index":index,"delta":part}),
                );
            }
            event(
                &mut frames,
                "response.function_call_arguments.done",
                json!({"item_id":item_id,"output_index":index,"arguments":item["arguments"]}),
            );
        } else if item["type"] == "message" {
            let part = &item["content"][0];
            let mut start = part.clone();
            let key = if refusal { "refusal" } else { "text" };
            start[key] = json!("");
            if !refusal {
                start["logprobs"] = json!([]);
            }
            event(
                &mut frames,
                "response.content_part.added",
                json!({"item_id":item_id,"output_index":index,"content_index":0,"part":start}),
            );
            for piece in fragments(&text) {
                let name = if refusal {
                    "response.refusal.delta"
                } else {
                    "response.output_text.delta"
                };
                let mut data =
                    json!({"item_id":item_id,"output_index":index,"content_index":0,"delta":piece});
                if !refusal {
                    data["logprobs"] = response_probabilities(body, &piece);
                }
                event(&mut frames, name, data);
                if ctx.fixture.scenario == Scenario::StreamError {
                    result["status"] = json!("failed");
                    result["error"] = json!({"code":"server_error","message":"Mock stream failed after generation started"});
                    result["output"] = json!([]);
                    result["usage"] = Value::Null;
                    result["completed_at"] = Value::Null;
                    if body["store"] != false {
                        state["openai"]["responses"][&id]["response"] = result.clone();
                    }
                    event(&mut frames, "response.failed", json!({"response":result}));
                    return Ok(WireResponse::sse(frames));
                }
            }
            let mut done = json!({"item_id":item_id,"output_index":index,"content_index":0});
            done[key] = json!(text);
            if !refusal {
                done["logprobs"] = response_probabilities(body, &text);
            }
            event(
                &mut frames,
                if refusal {
                    "response.refusal.done"
                } else {
                    "response.output_text.done"
                },
                done,
            );
            event(
                &mut frames,
                "response.content_part.done",
                json!({"item_id":item_id,"output_index":index,"content_index":0,"part":part}),
            );
        } else if let Some(parts) = item["summary"].as_array() {
            for (summary_index, part) in parts.iter().enumerate() {
                event(
                    &mut frames,
                    "response.reasoning_summary_part.added",
                    json!({"item_id":item_id,"output_index":index,"summary_index":summary_index,"part":{"type":"summary_text","text":""}}),
                );
                event(
                    &mut frames,
                    "response.reasoning_summary_text.delta",
                    json!({"item_id":item_id,"output_index":index,"summary_index":summary_index,"delta":part["text"]}),
                );
                event(
                    &mut frames,
                    "response.reasoning_summary_text.done",
                    json!({"item_id":item_id,"output_index":index,"summary_index":summary_index,"text":part["text"]}),
                );
                event(
                    &mut frames,
                    "response.reasoning_summary_part.done",
                    json!({"item_id":item_id,"output_index":index,"summary_index":summary_index,"part":part}),
                );
            }
        }
        event(
            &mut frames,
            "response.output_item.done",
            json!({"output_index":index,"item":item}),
        );
    }
    if ctx.fixture.scenario == Scenario::StreamError {
        result["status"] = json!("failed");
        result["error"] =
            json!({"code":"server_error","message":"Mock stream failed after generation started"});
        result["usage"] = Value::Null;
        result["completed_at"] = Value::Null;
        if body["store"] != false {
            state["openai"]["responses"][&id]["response"] = result.clone();
        }
        event(&mut frames, "response.failed", json!({"response":result}));
        return Ok(WireResponse::sse(frames));
    }
    event(
        &mut frames,
        if truncated {
            "response.incomplete"
        } else {
            "response.completed"
        },
        json!({"response":result}),
    );
    Ok(WireResponse::sse(frames))
}

pub fn render(
    ctx: &RequestContext<'_>,
    body: &Value,
    state: &mut Value,
) -> Result<WireResponse, ApiError> {
    if !state.is_object() {
        *state = json!({});
    }
    if responses(ctx) {
        response(ctx, body, state)
    } else {
        chat(ctx, body, state)
    }
}

fn error_body(error: &ApiError) -> Value {
    let kind = match error.status {
        401 => "authentication_error",
        429 if error.code.contains("quota")
            || error.code.contains("credit")
            || error.code.contains("spend") =>
        {
            "insufficient_quota"
        }
        429 => "rate_limit_error",
        503 => "service_unavailable_error",
        500..=599 => "server_error",
        _ => "invalid_request_error",
    };
    json!({"error":{"message":error.message,"type":kind,"param":error.param,"code":error.code}})
}

pub fn error(error: &ApiError, request_id: &str) -> WireResponse {
    WireResponse {
        status: error.status,
        headers: BTreeMap::from([("x-request-id".into(), request_id.into())]),
        body: error_body(error),
        events: None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{Fixture, ModelProfile};
    use super::*;
    use axum::http::HeaderMap;

    fn profile(id: &str) -> ModelProfile {
        ModelProfile {
            id: id.into(),
            record: None,
            protocols: vec![Protocol::OpenaiChat, Protocol::OpenaiResponses],
            input_modalities: vec!["text".into(), "image".into()],
            output_modalities: vec!["text".into()],
            context_window: None,
            max_output_tokens: None,
            tool_call: Some(true),
            reasoning: Some(true),
            structured_output: Some(true),
            behavior: BTreeMap::new(),
            sources: vec![],
            notes: vec![],
        }
    }
    fn context<'a>(
        model: &'a ModelProfile,
        fixture: &'a Fixture,
        headers: &'a HeaderMap,
        protocol: Protocol,
        stream: bool,
    ) -> RequestContext<'a> {
        RequestContext {
            provider: "openai",
            protocol,
            model,
            fixture,
            request_id: "request_test",
            session_id: "test",
            turn: 1,
            seed: 1,
            headers,
            stream,
        }
    }
    fn request(protocol: Protocol) -> Value {
        if protocol == Protocol::OpenaiResponses {
            json!({"model":"gpt-4.1","input":"Hello","tools":[{"type":"function","name":"weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}]})
        } else {
            json!({"model":"gpt-4.1","messages":[{"role":"user","content":"Hello"}],"tools":[{"type":"function","function":{"name":"weather","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}}]})
        }
    }

    #[test]
    fn invalid_schemas_fail_before_a_fixture_selects_tools_or_structured_output() {
        let model = profile("gpt-4.1");
        let headers = HeaderMap::new();
        let fixture = Fixture::default();
        for protocol in [Protocol::OpenaiChat, Protocol::OpenaiResponses] {
            let ctx = context(&model, &fixture, &headers, protocol, false);
            let mut body = request(protocol);
            let parameters = if protocol == Protocol::OpenaiChat {
                &mut body["tools"][0]["function"]["parameters"]
            } else {
                &mut body["tools"][0]["parameters"]
            };
            *parameters = json!({"type":123});
            assert_eq!(validate(&ctx, &body, &json!({})).unwrap_err().status, 400);
            *body
                .pointer_mut(if protocol == Protocol::OpenaiChat {
                    "/tools/0/function/parameters"
                } else {
                    "/tools/0/parameters"
                })
                .unwrap() =
                json!({"type":"object","properties":{"x":{"type":"string","pattern":"^a+$"}}});
            validate(&ctx, &body, &json!({})).unwrap();
            let definition = json!({"type":"json_schema","name":"result","schema":{"type":123}});
            if protocol == Protocol::OpenaiResponses {
                body["text"] = json!({"format":definition});
            } else {
                body["response_format"] = json!({"type":"json_schema","json_schema":definition});
            }
            assert_eq!(validate(&ctx, &body, &json!({})).unwrap_err().status, 400);
        }
    }

    #[test]
    fn response_tool_result_uses_call_id_and_stored_response() {
        let model = profile("gpt-4.1");
        let headers = HeaderMap::new();
        let fixture = Fixture {
            scenario: Scenario::ToolCall,
            ..Fixture::default()
        };
        let ctx = context(&model, &fixture, &headers, Protocol::OpenaiResponses, false);
        let body = request(ctx.protocol);
        let mut state = json!({});
        validate(&ctx, &body, &state).unwrap();
        let first = render(&ctx, &body, &mut state).unwrap().body;
        let mut next = json!({"model":"gpt-4.1","previous_response_id":first["id"],"input":[{"type":"function_call_output","call_id":first["output"][0]["call_id"],"output":"sunny"}]});
        validate(&ctx, &next, &state).unwrap();
        next["input"][0]["call_id"] = first["output"][0]["id"].clone();
        assert!(validate(&ctx, &next, &state).is_err());
    }

    #[test]
    fn chat_parallel_fragments_reconstruct_each_call_and_usage_precedes_done() {
        let model = profile("gpt-4.1");
        let headers = HeaderMap::new();
        let fixture = Fixture {
            scenario: Scenario::ParallelTools,
            ..Fixture::default()
        };
        let ctx = context(&model, &fixture, &headers, Protocol::OpenaiChat, true);
        let mut body = request(ctx.protocol);
        body["stream"] = json!(true);
        body["stream_options"] = json!({"include_usage":true});
        let frames = render(&ctx, &body, &mut json!({})).unwrap().events.unwrap();
        assert_eq!(frames.last().unwrap().data, "[DONE]");
        assert!(frames.last().unwrap().event.is_none());
        let usage: Value = serde_json::from_str(&frames[frames.len() - 2].data).unwrap();
        assert_eq!(usage["choices"], json!([]));
        assert!(usage["usage"]["total_tokens"].as_u64().unwrap() > 0);
        let mut args: BTreeMap<u64, String> = BTreeMap::new();
        for frame in frames.iter().take(frames.len() - 1) {
            let value: Value = serde_json::from_str(&frame.data).unwrap();
            if let Some(calls) = value
                .pointer("/choices/0/delta/tool_calls")
                .and_then(Value::as_array)
            {
                for call in calls {
                    args.entry(call["index"].as_u64().unwrap())
                        .or_default()
                        .push_str(call["function"]["arguments"].as_str().unwrap());
                }
            }
        }
        assert_eq!(args.len(), 2);
        for value in args.values() {
            assert!(serde_json::from_str::<Value>(value).unwrap()["city"].is_string());
        }
    }

    #[test]
    fn responses_stream_has_contiguous_sequence_and_complete_terminal_object() {
        let model = profile("gpt-4.1");
        let headers = HeaderMap::new();
        let fixture = Fixture::default();
        let ctx = context(&model, &fixture, &headers, Protocol::OpenaiResponses, true);
        let frames = render(&ctx, &request(ctx.protocol), &mut json!({}))
            .unwrap()
            .events
            .unwrap();
        for (index, frame) in frames.iter().enumerate() {
            let value: Value = serde_json::from_str(&frame.data).unwrap();
            assert_eq!(value["sequence_number"], index);
            assert_eq!(value["type"].as_str(), frame.event.as_deref());
        }
        let last: Value = serde_json::from_str(&frames.last().unwrap().data).unwrap();
        assert_eq!(last["type"], "response.completed");
        assert_eq!(
            last["response"]["output"][0]["content"][0]["text"],
            fixture.text
        );
    }

    #[test]
    fn explicit_model_constraints_do_not_apply_to_other_providers() {
        let model = profile("gpt-6-astra");
        let headers = HeaderMap::new();
        let fixture = Fixture::default();
        let mut ctx = context(&model, &fixture, &headers, Protocol::OpenaiChat, false);
        let mut body = request(ctx.protocol);
        body["model"] = json!(model.id);
        assert!(validate(&ctx, &body, &json!({})).is_err());
        body.as_object_mut().unwrap().remove("tools");
        body["reasoning_effort"] = json!("none");
        assert!(validate(&ctx, &body, &json!({})).is_err());
        ctx.provider = "custom";
        assert!(validate(&ctx, &body, &json!({})).is_ok());
    }

    #[test]
    fn tool_history_rejects_duplicate_and_changed_results() {
        let model = profile("gpt-4.1");
        let headers = HeaderMap::new();
        let fixture = Fixture::default();
        let ctx = context(&model, &fixture, &headers, Protocol::OpenaiChat, false);
        let call =
            json!({"id":"call_1","type":"function","function":{"name":"weather","arguments":"{}"}});
        let mut body = json!({"model":"gpt-4.1","messages":[{"role":"assistant","tool_calls":[call]},{"role":"tool","tool_call_id":"call_1","content":"done"}]});
        assert!(validate(&ctx, &body, &json!({})).is_ok());
        body["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"tool","tool_call_id":"call_1","content":"duplicate"}));
        assert!(validate(&ctx, &body, &json!({})).is_err());
    }

    #[test]
    fn output_budgets_include_unicode_tools_refusals_and_reasoning() {
        let mut model = profile("gpt-4.1");
        model.max_output_tokens = Some(7);
        let headers = HeaderMap::new();
        for protocol in [Protocol::OpenaiChat, Protocol::OpenaiResponses] {
            for scenario in [
                Scenario::Text,
                Scenario::ToolCall,
                Scenario::ParallelTools,
                Scenario::Refusal,
                Scenario::Reasoning,
            ] {
                let fixture = Fixture {
                    scenario,
                    text: "世界abcdefghi".into(),
                    ..Fixture::default()
                };
                let ctx = context(&model, &fixture, &headers, protocol, false);
                let mut body = request(protocol);
                for explicit in [false, true] {
                    if explicit {
                        body[if responses(&ctx) {
                            "max_output_tokens"
                        } else {
                            "max_completion_tokens"
                        }] = json!(5);
                    }
                    let response = render(&ctx, &body, &mut json!({})).unwrap().body;
                    let used = response["usage"][if responses(&ctx) {
                        "output_tokens"
                    } else {
                        "completion_tokens"
                    }]
                    .as_u64()
                    .unwrap();
                    assert!(
                        used <= if explicit { 5 } else { 7 },
                        "{protocol:?} {scenario:?}: {response}"
                    );
                    if responses(&ctx) {
                        assert_eq!(response["status"], "incomplete");
                        assert_eq!(
                            response["incomplete_details"]["reason"],
                            "max_output_tokens"
                        );
                    } else {
                        assert_eq!(response["choices"][0]["finish_reason"], "length");
                    }
                }
            }
        }
    }

    #[test]
    fn response_stream_error_terminates_empty_and_tool_output() {
        let model = profile("gpt-4.1");
        let headers = HeaderMap::new();
        let fixture = Fixture {
            scenario: Scenario::StreamError,
            text: String::new(),
            ..Fixture::default()
        };
        let ctx = context(&model, &fixture, &headers, Protocol::OpenaiResponses, true);
        let mut body = request(ctx.protocol);
        for required in [false, true] {
            if required {
                body["tool_choice"] = json!("required");
            }
            let frames = render(&ctx, &body, &mut json!({})).unwrap().events.unwrap();
            assert_eq!(
                frames.last().unwrap().event.as_deref(),
                Some("response.failed")
            );
            assert!(
                !frames
                    .iter()
                    .any(|frame| frame.event.as_deref() == Some("response.completed"))
            );
        }
    }

    #[test]
    fn synthetic_logprob_bytes_preserve_unicode_and_match_output_usage() {
        let model = profile("gpt-4.1");
        let headers = HeaderMap::new();
        let fixture = Fixture {
            text: "A世界".into(),
            ..Fixture::default()
        };
        let ctx = context(&model, &fixture, &headers, Protocol::OpenaiChat, false);
        let mut body = request(ctx.protocol);
        body["logprobs"] = json!(true);
        body["top_logprobs"] = json!(2);
        let response = render(&ctx, &body, &mut json!({})).unwrap().body;
        let tokens = response["choices"][0]["logprobs"]["content"]
            .as_array()
            .unwrap();
        let bytes: Vec<u8> = tokens
            .iter()
            .flat_map(|token| {
                token["bytes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u8)
            })
            .collect();
        assert_eq!(String::from_utf8(bytes).unwrap(), fixture.text);
        assert_eq!(
            tokens.len() as u64,
            response["usage"]["completion_tokens"].as_u64().unwrap()
        );
        let fixture = Fixture {
            scenario: Scenario::Reasoning,
            text: String::new(),
            ..fixture
        };
        let ctx = context(&model, &fixture, &headers, Protocol::OpenaiChat, false);
        body["n"] = json!(2);
        let response = render(&ctx, &body, &mut json!({})).unwrap().body;
        assert_eq!(
            response["usage"]["completion_tokens"],
            2 * "Mock reasoning summary.".chars().count()
        );
        assert_eq!(
            response["usage"]["completion_tokens_details"]["reasoning_tokens"],
            response["usage"]["completion_tokens"]
        );
    }
}
