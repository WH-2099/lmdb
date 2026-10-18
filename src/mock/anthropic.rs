//! Native Messages contract, checked 2026-10-19 against platform.claude.com.
//! Sources: /docs/en/api/messages/create, /docs/en/build-with-claude/{thinking,streaming},
//! /docs/en/agents-and-tools/tool-use/handle-tool-calls, and /docs/en/api/errors.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::model::{ApiError, Frame, RequestContext, Scenario, WireResponse};

fn invalid(path: &str, message: &str) -> ApiError {
    ApiError::invalid(path, message)
}

fn string<'a>(value: &'a Value, path: &str) -> Result<&'a str, ApiError> {
    value
        .as_str()
        .ok_or_else(|| invalid(path, "Expected a string"))
}

fn nonempty<'a>(value: &'a Value, path: &str) -> Result<&'a str, ApiError> {
    let text = string(value, path)?;
    if text.is_empty() {
        return Err(invalid(path, "Must not be empty"));
    }
    Ok(text)
}

// Family membership is explicit: future IDs must come with profile settings.
fn family(id: &str) -> Option<u8> {
    match id {
        "claude-3-7-sonnet-20250219"
        | "claude-sonnet-4-20250514"
        | "claude-opus-4-20250514"
        | "claude-opus-4-1-20250805"
        | "claude-sonnet-4-5"
        | "claude-sonnet-4-5-20250929"
        | "claude-haiku-4-5"
        | "claude-haiku-4-5-20251001"
        | "claude-opus-4-5"
        | "claude-opus-4-5-20251101" => Some(0),
        "claude-opus-4-6" | "claude-sonnet-4-6" => Some(1),
        "claude-opus-4-7" | "claude-opus-4-8" => Some(2),
        "claude-opus-5" | "claude-haiku-5-5" => Some(3),
        "claude-sonnet-5" => Some(4),
        "claude-opus-5-5" | "claude-fable-5" | "claude-fable-5-1" | "claude-mythos-5"
        | "claude-mythos-5-1" => Some(5),
        "claude-sonnet-5-5" => Some(6),
        "claude-mythos-preview" => Some(7),
        _ => None,
    }
}

fn setting<'a>(ctx: &'a RequestContext<'_>, key: &str) -> Option<&'a Value> {
    ctx.model.behavior.get(key)
}

fn default_mode<'a>(ctx: &'a RequestContext<'_>) -> &'a str {
    setting(ctx, "anthropic_default_thinking")
        .and_then(Value::as_str)
        .unwrap_or(if matches!(family(&ctx.model.id), Some(3..=7)) {
            "adaptive"
        } else {
            "disabled"
        })
}

fn mode<'a>(ctx: &'a RequestContext<'_>, body: &'a Value) -> &'a str {
    body.pointer("/thinking/type")
        .and_then(Value::as_str)
        .unwrap_or_else(|| default_mode(ctx))
}

fn default_effort<'a>(ctx: &'a RequestContext<'_>) -> &'a str {
    setting(ctx, "anthropic_default_effort")
        .and_then(Value::as_str)
        .unwrap_or(
            if matches!(
                ctx.model.id.as_str(),
                "claude-opus-5-5" | "claude-haiku-5-5"
            ) {
                "medium"
            } else {
                "high"
            },
        )
}

fn allows_mode(ctx: &RequestContext<'_>, value: &str) -> Option<bool> {
    if let Some(values) = setting(ctx, "anthropic_thinking_modes").and_then(Value::as_array) {
        return Some(values.iter().any(|v| v.as_str() == Some(value)));
    }
    Some(match family(&ctx.model.id)? {
        0 => matches!(value, "enabled" | "disabled"),
        1 => matches!(value, "enabled" | "adaptive" | "disabled"),
        2..=4 => matches!(value, "adaptive" | "disabled"),
        5 => value == "adaptive",
        6 => matches!(value, "adaptive" | "between_tools"),
        7 => matches!(value, "adaptive" | "enabled"),
        _ => unreachable!(),
    })
}

fn per_message_effort(ctx: &RequestContext<'_>) -> bool {
    setting(ctx, "anthropic_per_message_effort")
        .and_then(Value::as_bool)
        .unwrap_or(matches!(
            ctx.model.id.as_str(),
            "claude-fable-5-1"
                | "claude-mythos-5-1"
                | "claude-opus-5"
                | "claude-opus-5-5"
                | "claude-sonnet-5-5"
                | "claude-haiku-5-5"
        ))
}

fn beta(ctx: &RequestContext<'_>, name: &str) -> bool {
    ctx.headers.get_all("anthropic-beta").iter().any(|header| {
        header
            .to_str()
            .is_ok_and(|header| header.split(',').any(|v| v.trim() == name))
    })
}

fn effort(ctx: &RequestContext<'_>, value: &Value) -> Result<(), ApiError> {
    let value = string(value, "output_config.effort")?;
    let allowed =
        if let Some(values) = setting(ctx, "anthropic_effort_levels").and_then(Value::as_array) {
            values.iter().any(|v| v.as_str() == Some(value))
        } else {
            match family(&ctx.model.id) {
                Some(0) => {
                    matches!(
                        ctx.model.id.as_str(),
                        "claude-opus-4-5" | "claude-opus-4-5-20251101"
                    ) && matches!(value, "low" | "medium" | "high")
                }
                Some(1 | 7) => matches!(value, "low" | "medium" | "high" | "max"),
                Some(_) => matches!(value, "low" | "medium" | "high" | "xhigh" | "max"),
                None => {
                    return Err(ApiError::unsupported(
                        "Configure anthropic_effort_levels for this model",
                    ));
                }
            }
        };
    if !allowed {
        return Err(invalid(
            "output_config.effort",
            "Effort level is not supported by this model",
        ));
    }
    Ok(())
}

fn image(value: &Value, path: &str) -> Result<(), ApiError> {
    let source = &value["source"];
    match source["type"].as_str() {
        Some("base64") => {
            if !matches!(
                source["media_type"].as_str(),
                Some("image/jpeg" | "image/png" | "image/gif" | "image/webp")
            ) {
                return Err(invalid(path, "Unsupported image media_type"));
            }
            let data = nonempty(&source["data"], path)?;
            if data.len() > 10 * 1024 * 1024 {
                return Err(invalid(
                    path,
                    "Base64 image exceeds the direct API 10 MB limit",
                ));
            }
            if !data
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
            {
                return Err(invalid(path, "Invalid base64 image data"));
            }
        }
        Some("url") => {
            let url = nonempty(&source["url"], path)?;
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(invalid(path, "Image URL must use HTTP or HTTPS"));
            }
        }
        Some("file") => {
            return Err(ApiError::unsupported(
                "Files API image references are not implemented",
            ));
        }
        _ => return Err(invalid(path, "Image source must be base64, url, or file")),
    }
    if value.get("transformations").is_some() {
        return Err(ApiError::unsupported(
            "Image transformations are not implemented",
        ));
    }
    Ok(())
}

fn protected(block: &Value) -> bool {
    matches!(
        block["type"].as_str(),
        Some("thinking" | "redacted_thinking")
    )
}

fn validate_block(block: &Value, role: &str, path: &str, state: &Value) -> Result<(), ApiError> {
    match block["type"].as_str() {
        Some("text") => {
            string(&block["text"], path)?;
        }
        Some("image") => image(block, path)?,
        Some("tool_use") => {
            if role != "assistant" {
                return Err(invalid(path, "tool_use blocks require the assistant role"));
            }
            nonempty(&block["id"], path)?;
            nonempty(&block["name"], path)?;
            if !block["input"].is_object() {
                return Err(invalid(path, "tool_use.input must be an object"));
            }
        }
        Some("tool_result") => {
            if role != "user" {
                return Err(invalid(path, "tool_result blocks require the user role"));
            }
            nonempty(&block["tool_use_id"], path)?;
            if block.get("is_error").is_some_and(|v| !v.is_boolean()) {
                return Err(invalid(path, "tool_result.is_error must be a boolean"));
            }
            if let Some(content) = block.get("content") {
                if let Some(blocks) = content.as_array() {
                    for child in blocks {
                        match child["type"].as_str() {
                            Some("text") => {
                                string(&child["text"], path)?;
                            }
                            Some("image") => image(child, path)?,
                            Some("document" | "search_result") => {
                                return Err(ApiError::unsupported(
                                    "Document and search-result tool content is not implemented",
                                ));
                            }
                            _ => {
                                return Err(invalid(
                                    path,
                                    "Unsupported nested tool_result content type",
                                ));
                            }
                        }
                    }
                } else if !content.is_string() {
                    return Err(invalid(
                        path,
                        "tool_result.content must be a string or content array",
                    ));
                }
            }
        }
        Some("thinking" | "redacted_thinking") => {
            if role != "assistant" {
                return Err(invalid(path, "Thinking blocks require the assistant role"));
            }
            let key = if block["type"] == "thinking" {
                "signature"
            } else {
                "data"
            };
            let signature = nonempty(&block[key], path)?;
            if key == "signature" {
                string(&block["thinking"], path)?;
            }
            let original = state
                .pointer("/anthropic/protected")
                .and_then(Value::as_array)
                .and_then(|blocks| blocks.iter().find(|b| b[key].as_str() == Some(signature)));
            match original {
                Some(original) if same_protected(original, block) => {}
                Some(_) => return Err(invalid(path, "Thinking blocks cannot be modified")),
                None if signature.starts_with("mock_sig_")
                    || signature.starts_with("mock_redacted_") =>
                {
                    return Err(invalid(path, "Thinking blocks cannot be modified"));
                }
                None => {
                    return Err(ApiError::unsupported(
                        "Only thinking signatures emitted in this mock session can be verified",
                    ));
                }
            }
        }
        Some(
            "document"
            | "server_tool_use"
            | "web_search_tool_result"
            | "tool_addition"
            | "tool_removal",
        ) => {
            return Err(ApiError::unsupported(
                "This Anthropic content block is not implemented",
            ));
        }
        Some(_) => {
            return Err(ApiError::unsupported(
                "Unknown Anthropic content block type",
            ));
        }
        None => return Err(invalid(path, "Content blocks require a type")),
    }
    Ok(())
}

fn same_protected(original: &Value, candidate: &Value) -> bool {
    if original["type"] != candidate["type"] {
        return false;
    }
    if original["type"] == "thinking" {
        original["signature"] == candidate["signature"]
            && (original["thinking"] == "" || original["thinking"] == candidate["thinking"])
    } else {
        original["data"] == candidate["data"]
    }
}

fn content_blocks(content: &Value, path: &str) -> Result<Vec<Value>, ApiError> {
    if let Some(text) = content.as_str() {
        Ok(vec![json!({"type":"text","text":text})])
    } else {
        content
            .as_array()
            .filter(|blocks| !blocks.is_empty())
            .cloned()
            .ok_or_else(|| invalid(path, "Content must be a string or nonempty block array"))
    }
}

fn validate_messages(
    ctx: &RequestContext<'_>,
    body: &Value,
    state: &Value,
) -> Result<(), ApiError> {
    let input = body["messages"]
        .as_array()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| invalid("messages", "Expected a nonempty message array"))?;
    let mut messages: Vec<Value> = Vec::new();
    for (index, message) in input.iter().enumerate() {
        let role = message["role"].as_str();
        if matches!(role, Some("user" | "assistant"))
            && messages
                .last()
                .is_some_and(|previous| previous["role"] == message["role"])
        {
            if message.get("output_config").is_some() {
                return Err(invalid(
                    "messages",
                    "Per-message output_config requires the system role",
                ));
            }
            let previous = messages.last_mut().unwrap();
            let mut blocks = content_blocks(&previous["content"], "messages.content")?;
            blocks.extend(content_blocks(
                &message["content"],
                &format!("messages.{index}.content"),
            )?);
            previous["content"] = json!(blocks);
        } else {
            messages.push(message.clone());
        }
    }
    let mut pending = BTreeSet::new();
    let mut seen_ids = BTreeSet::new();
    let mut image_count = 0;
    for (index, message) in messages.iter().enumerate() {
        let path = format!("messages.{index}");
        let role = string(&message["role"], &path)?;
        if role == "system" {
            if !pending.is_empty() {
                return Err(invalid(
                    &path,
                    "Tool results must immediately follow tool use",
                ));
            }
            if !beta(ctx, "mid-conversation-output-config-2026-07-01") {
                return Err(invalid(
                    &path,
                    "System messages with output_config require mid-conversation-output-config-2026-07-01",
                ));
            }
            if !per_message_effort(ctx) {
                return Err(invalid(
                    &path,
                    "This model does not support per-message effort",
                ));
            }
            if message["content"].as_array().is_none_or(|v| !v.is_empty()) {
                return Err(ApiError::unsupported(
                    "System messages with content require a separate unimplemented beta",
                ));
            }
            effort(ctx, &message["output_config"]["effort"])?;
            continue;
        }
        if !matches!(role, "user" | "assistant") {
            return Err(invalid(&path, "Expected user or assistant role"));
        }
        if message.get("output_config").is_some() {
            return Err(invalid(
                &path,
                "Per-message output_config requires the system role",
            ));
        }
        let content = &message["content"];
        if content.is_string() {
            if !pending.is_empty() {
                return Err(invalid(
                    &path,
                    "tool_use ids were found without tool_result blocks immediately after",
                ));
            }
            continue;
        }
        let blocks = content
            .as_array()
            .filter(|b| !b.is_empty())
            .ok_or_else(|| invalid(&path, "Content must be a string or nonempty block array"))?;
        let mut results = BTreeSet::new();
        let mut calls = BTreeSet::new();
        let mut saw_other = false;
        for (block_index, block) in blocks.iter().enumerate() {
            let block_path = format!("{path}.content.{block_index}");
            validate_block(block, role, &block_path, state)?;
            if block["type"] == "tool_result" {
                if saw_other {
                    return Err(invalid(&block_path, "tool_result blocks must come first"));
                }
                let id = block["tool_use_id"].as_str().unwrap();
                if !pending.contains(id) || !results.insert(id.to_owned()) {
                    return Err(invalid(&block_path, "Unknown or duplicate tool_use_id"));
                }
            } else {
                saw_other = true;
            }
            if block["type"] == "tool_use" {
                let id = block["id"].as_str().unwrap().to_owned();
                if !seen_ids.insert(id.clone()) {
                    return Err(invalid(&block_path, "Duplicate tool_use id"));
                }
                calls.insert(id);
            }
            if block["type"] == "image" {
                image_count += 1;
            }
            if let Some(nested) = block["content"].as_array() {
                image_count += nested.iter().filter(|b| b["type"] == "image").count();
            }
        }
        if pending != results {
            return Err(invalid(
                &path,
                "Every tool_use requires a result in the immediately following user message",
            ));
        }
        pending = calls;
    }
    if !pending.is_empty() {
        return Err(invalid(
            "messages",
            "Tool use requires a following user message with results",
        ));
    }
    let image_limit = if ctx.model.context_window == Some(200_000) {
        100
    } else {
        600
    };
    if image_count > image_limit {
        return Err(invalid("messages", "Too many image blocks for this model"));
    }
    if let Some(expected) = state.pointer("/anthropic/pending").filter(|v| !v.is_null()) {
        let ids = expected["ids"].as_array().unwrap();
        let assistant = messages
            .iter()
            .rev()
            .find(|m| m["role"] == "assistant")
            .and_then(|m| m["content"].as_array())
            .ok_or_else(|| {
                invalid(
                    "messages",
                    "Replay the assistant tool-use response before returning results",
                )
            })?;
        for id in ids {
            if !assistant
                .iter()
                .any(|b| b["type"] == "tool_use" && b["id"] == *id)
            {
                return Err(invalid(
                    "messages",
                    "Missing tool call from this mock session",
                ));
            }
        }
        let actual: Vec<_> = assistant.iter().filter(|b| protected(b)).collect();
        let original = expected["protected"].as_array().unwrap();
        if original.len() != actual.len()
            || original
                .iter()
                .zip(actual)
                .any(|(a, b)| !same_protected(a, b))
        {
            return Err(invalid(
                "messages",
                "Thinking blocks cannot be modified, reordered, or omitted from a tool-use turn",
            ));
        }
    }
    let final_role = messages.last().unwrap()["role"].as_str();
    if final_role == Some("assistant") {
        let allowed = setting(ctx, "anthropic_prefill")
            .and_then(Value::as_bool)
            .or_else(|| family(&ctx.model.id).map(|f| f == 0));
        match allowed {
            Some(true) if mode(ctx, body) == "disabled" => {}
            Some(_) => {
                return Err(invalid(
                    "messages",
                    "This model does not support assistant message prefill. The conversation must end with a user message.",
                ));
            }
            None => {
                return Err(ApiError::unsupported(
                    "Configure anthropic_prefill for this model",
                ));
            }
        }
    }
    Ok(())
}

pub fn validate(ctx: &RequestContext<'_>, body: &Value, state: &Value) -> Result<(), ApiError> {
    let version = ctx
        .headers
        .get("anthropic-version")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| invalid("anthropic-version", "anthropic-version header is required"))?;
    if version != "2023-06-01" {
        return Err(ApiError::unsupported(
            "Only anthropic-version 2023-06-01 is implemented",
        ));
    }
    if !body.is_object() {
        return Err(invalid("body", "Expected a JSON object"));
    }
    if string(&body["model"], "model")? != ctx.model.id {
        return Err(invalid(
            "model",
            "Model does not match the selected profile",
        ));
    }
    let max_tokens = body["max_tokens"]
        .as_u64()
        .ok_or_else(|| invalid("max_tokens", "Expected a nonnegative integer"))?;
    if ctx
        .model
        .max_output_tokens
        .is_some_and(|limit| max_tokens > limit)
    {
        return Err(invalid(
            "max_tokens",
            "Requested output exceeds this model's token limit",
        ));
    }
    if body.get("stream").is_some_and(|v| !v.is_boolean()) {
        return Err(invalid("stream", "Expected a boolean"));
    }
    for key in body.as_object().unwrap().keys() {
        if !matches!(
            key.as_str(),
            "model"
                | "max_tokens"
                | "messages"
                | "system"
                | "stream"
                | "tools"
                | "tool_choice"
                | "thinking"
                | "output_config"
                | "stop_sequences"
                | "temperature"
                | "top_p"
                | "top_k"
                | "metadata"
                | "cache_control"
        ) {
            return Err(ApiError::unsupported(format!(
                "Anthropic parameter {key} is not implemented"
            )));
        }
    }
    if let Some(system) = body.get("system") {
        if let Some(blocks) = system.as_array() {
            for block in blocks {
                if block["type"] != "text" {
                    return Err(invalid(
                        "system",
                        "Top-level system content supports text blocks only",
                    ));
                }
                string(&block["text"], "system.text")?;
            }
        } else {
            string(system, "system")?;
        }
    }
    if let Some(sequences) = body.get("stop_sequences") {
        let sequences = sequences
            .as_array()
            .ok_or_else(|| invalid("stop_sequences", "Expected an array"))?;
        for sequence in sequences {
            string(sequence, "stop_sequences")?;
        }
    }
    validate_thinking(ctx, body, max_tokens)?;
    validate_tools(ctx, body)?;
    if let Some(config) = body.get("output_config") {
        if !config.is_object() {
            return Err(invalid("output_config", "Expected an object"));
        }
        if let Some(value) = config.get("effort") {
            effort(ctx, value)?;
        }
        if let Some(format) = config.get("format") {
            if format["type"] != "json_schema" {
                return Err(ApiError::unsupported(
                    "Only json_schema structured output is implemented",
                ));
            }
            if !format["schema"].is_object() && !format["schema"].is_boolean() {
                return Err(invalid(
                    "output_config.format.schema",
                    "Expected a JSON Schema",
                ));
            }
            super::schema::validate(&format["schema"])?;
            if ctx.model.structured_output == Some(false) {
                return Err(invalid(
                    "output_config.format",
                    "This model does not support structured output",
                ));
            }
            if citations_enabled(&body["messages"]) {
                return Err(invalid(
                    "output_config.format",
                    "Citations cannot be combined with JSON output",
                ));
            }
        }
    }
    validate_messages(ctx, body, state)
}

fn citations_enabled(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            (map.get("type").is_some_and(|v| v == "document")
                && map.get("citations").is_some_and(|v| v["enabled"] == true))
                || map.get("content").is_some_and(citations_enabled)
        }
        Value::Array(values) => values.iter().any(citations_enabled),
        _ => false,
    }
}

fn validate_thinking(
    ctx: &RequestContext<'_>,
    body: &Value,
    max_tokens: u64,
) -> Result<(), ApiError> {
    let selected = mode(ctx, body);
    if let Some(thinking) = body.get("thinking") {
        let selected = string(&thinking["type"], "thinking.type")?;
        match allows_mode(ctx, selected) {
            Some(true) => {}
            Some(false) => {
                return Err(invalid(
                    "thinking.type",
                    "Thinking mode is not supported by this model",
                ));
            }
            None => {
                return Err(ApiError::unsupported(
                    "Configure anthropic_thinking_modes for this model",
                ));
            }
        }
        if selected == "enabled" {
            let budget = thinking["budget_tokens"]
                .as_u64()
                .ok_or_else(|| invalid("thinking.budget_tokens", "Expected an integer"))?;
            let interleaved = matches!(
                ctx.model.id.as_str(),
                "claude-sonnet-4-6"
                    | "claude-sonnet-4-5"
                    | "claude-sonnet-4-5-20250929"
                    | "claude-opus-4-5"
                    | "claude-opus-4-5-20251101"
                    | "claude-sonnet-4-20250514"
                    | "claude-opus-4-20250514"
                    | "claude-opus-4-1-20250805"
            ) && beta(ctx, "interleaved-thinking-2025-05-14");
            if budget < 1024 || (budget >= max_tokens && !interleaved) {
                return Err(invalid(
                    "thinking.budget_tokens",
                    "Budget must be at least 1024 and below max_tokens, except with supported interleaved thinking",
                ));
            }
        } else if thinking.get("budget_tokens").is_some() {
            return Err(invalid(
                "thinking.budget_tokens",
                "A budget requires enabled thinking",
            ));
        }
        if let Some(display) = thinking.get("display")
            && !display.is_null()
            && !matches!(display.as_str(), Some("summarized" | "omitted"))
        {
            return Err(ApiError::unsupported(
                "Only summarized and omitted thinking display are implemented",
            ));
        }
        if thinking.get("block_binding").is_some() {
            return Err(ApiError::unsupported(
                "Thinking prefix-binding controls are not implemented",
            ));
        }
    }
    let effort = body
        .pointer("/output_config/effort")
        .and_then(Value::as_str)
        .unwrap_or_else(|| default_effort(ctx));
    if matches!(family(&ctx.model.id), Some(3 | 6))
        && matches!(selected, "disabled" | "between_tools")
        && matches!(effort, "xhigh" | "max")
    {
        return Err(invalid(
            "thinking.type",
            "This thinking mode requires high effort or below",
        ));
    }
    if let Some(messages) = body["messages"].as_array()
        && matches!(selected, "disabled" | "between_tools")
        && matches!(
            ctx.model.id.as_str(),
            "claude-sonnet-5-5" | "claude-haiku-5-5"
        )
        && messages
            .iter()
            .filter_map(|m| m.pointer("/output_config/effort").and_then(Value::as_str))
            .any(|level| level != effort)
    {
        return Err(invalid(
            "output_config.effort",
            "Effort cannot change mid-conversation with this thinking mode",
        ));
    }
    let modern = matches!(family(&ctx.model.id), Some(2..=7));
    for key in ["temperature", "top_p", "top_k"] {
        if let Some(value) = body.get(key) {
            let number = value
                .as_f64()
                .ok_or_else(|| invalid(key, "Expected a number"))?;
            if key != "top_k" && !(0.0..=1.0).contains(&number) {
                return Err(invalid(key, "Expected a number between 0 and 1"));
            }
            if key == "top_k" && value.as_u64().is_none() {
                return Err(invalid(key, "Expected a nonnegative integer"));
            }
            if modern
                && (key == "top_k"
                    || (key == "temperature" && number != 1.0)
                    || (key == "top_p" && number < 0.99))
            {
                return Err(invalid(
                    key,
                    "Sampling parameter is not supported by this model",
                ));
            }
            // The current generated reference and thinking guide disagree at top_p > .99.
            if modern && key == "top_p" && number > 0.99 {
                return Err(ApiError::unsupported(
                    "top_p above 0.99 needs a provider probe to resolve conflicting documentation",
                ));
            }
            if !modern && selected != "disabled" && (key != "top_p" || number < 0.95) {
                return Err(invalid(
                    key,
                    "Sampling setting is incompatible with thinking",
                ));
            }
            if family(&ctx.model.id).is_none() {
                return Err(ApiError::unsupported(
                    "Sampling behavior is not configured for this model",
                ));
            }
        }
    }
    if modern && body.get("temperature").is_some() && body.get("top_p").is_some() {
        return Err(invalid(
            "temperature",
            "temperature and top_p cannot both be specified",
        ));
    }
    Ok(())
}

fn validate_tools(ctx: &RequestContext<'_>, body: &Value) -> Result<(), ApiError> {
    let mut names = BTreeSet::new();
    if let Some(tools) = body.get("tools") {
        let tools = tools
            .as_array()
            .ok_or_else(|| invalid("tools", "Expected an array"))?;
        if !tools.is_empty() && ctx.model.tool_call == Some(false) {
            return Err(invalid("tools", "This model does not support tools"));
        }
        for tool in tools {
            if tool
                .get("type")
                .is_some_and(|kind| !kind.is_null() && kind != "custom")
            {
                return Err(ApiError::unsupported(
                    "Server tools and toolsets are not implemented",
                ));
            }
            let name = nonempty(&tool["name"], "tools.name")?;
            if !names.insert(name) {
                return Err(invalid("tools", "Duplicate tool name"));
            }
            if tool["input_schema"]["type"] != "object" {
                return Err(invalid(
                    "tools.input_schema",
                    "Tool input_schema must describe an object",
                ));
            }
            super::schema::validate(&tool["input_schema"])?;
            if tool.get("strict").is_some_and(|v| !v.is_boolean()) {
                return Err(invalid("tools.strict", "Expected a boolean"));
            }
        }
    }
    if let Some(choice) = body.get("tool_choice") {
        let choice_type = string(&choice["type"], "tool_choice.type")?;
        if !matches!(choice_type, "auto" | "none" | "any" | "tool") {
            return Err(invalid("tool_choice.type", "Unknown tool choice"));
        }
        if choice
            .get("disable_parallel_tool_use")
            .is_some_and(|v| !v.is_boolean())
        {
            return Err(invalid(
                "tool_choice.disable_parallel_tool_use",
                "Expected a boolean",
            ));
        }
        if choice_type == "tool" && !names.contains(nonempty(&choice["name"], "tool_choice.name")?)
        {
            return Err(invalid("tool_choice.name", "Selected tool is not defined"));
        }
        if matches!(choice_type, "any" | "tool") {
            if names.is_empty() {
                return Err(invalid("tools", "Forced tool use requires tools"));
            }
            if mode(ctx, body) == "enabled" {
                return Err(invalid(
                    "tool_choice",
                    "Forced tool use is incompatible with manual thinking",
                ));
            }
            let allowed = setting(ctx, "anthropic_forced_tools")
                .and_then(Value::as_bool)
                .or_else(|| {
                    family(&ctx.model.id).map(|_| {
                        !matches!(
                            ctx.model.id.as_str(),
                            "claude-opus-5-5"
                                | "claude-sonnet-5-5"
                                | "claude-fable-5-1"
                                | "claude-mythos-5-1"
                        )
                    })
                });
            match allowed {
                Some(true) => {}
                Some(false) => {
                    return Err(invalid(
                        "tool_choice",
                        "Forced tool use is not supported by this model",
                    ));
                }
                None => {
                    return Err(ApiError::unsupported(
                        "Configure anthropic_forced_tools for this model",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn opaque(ctx: &RequestContext<'_>, label: &str) -> String {
    let mut hash = Sha256::new();
    for value in [
        ctx.session_id,
        &ctx.seed.to_string(),
        &ctx.turn.to_string(),
        label,
    ] {
        hash.update(value.as_bytes());
        hash.update([0]);
    }
    format!("{:x}", hash.finalize())
}

pub fn render(
    ctx: &RequestContext<'_>,
    body: &Value,
    state: &mut Value,
) -> Result<WireResponse, ApiError> {
    let scenario = ctx.fixture.scenario;
    let choice = body
        .pointer("/tool_choice/type")
        .and_then(Value::as_str)
        .unwrap_or("auto");
    let continuation = state
        .pointer("/anthropic/pending")
        .is_some_and(|v| !v.is_null())
        || body["messages"]
            .as_array()
            .and_then(|messages| messages.last())
            .and_then(|message| message["content"].as_array())
            .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "tool_result"));
    let wants_tool = matches!(choice, "any" | "tool")
        || (!continuation
            && matches!(
                scenario,
                Scenario::ToolCall | Scenario::ParallelTools | Scenario::MalformedToolArguments
            ));
    if wants_tool && choice == "none" {
        return Err(ApiError::unsupported(
            "The selected tool fixture conflicts with tool_choice none",
        ));
    }
    if scenario == Scenario::StreamError && !ctx.stream {
        return Err(ApiError::unsupported("stream_error requires stream:true"));
    }
    let mut blocks = Vec::new();
    let selected = mode(ctx, body);
    let selected = if continuation
        && state["anthropic"]["pending"]["thinking_mode"]
            .as_str()
            .is_some_and(|previous| previous != selected)
    {
        "disabled"
    } else {
        selected
    };
    let max_tokens = body["max_tokens"]
        .as_u64()
        .unwrap_or(0)
        .min(ctx.model.max_output_tokens.unwrap_or(u64::MAX));
    let reasoning = max_tokens > 0
        && scenario != Scenario::Empty
        && !matches!(choice, "any" | "tool")
        && (selected == "enabled"
            || (selected == "adaptive"
                && (scenario == Scenario::Reasoning
                    || wants_tool
                    || body.get("thinking").is_some())));
    if scenario == Scenario::Reasoning && selected == "disabled" {
        return Err(ApiError::unsupported(
            "The reasoning fixture requires a supported thinking mode",
        ));
    }
    if reasoning {
        let display = body
            .pointer("/thinking/display")
            .and_then(Value::as_str)
            .or_else(|| setting(ctx, "anthropic_default_thinking_display").and_then(Value::as_str))
            .unwrap_or(if matches!(family(&ctx.model.id), Some(3..=7)) {
                "omitted"
            } else {
                "summarized"
            });
        blocks.push(json!({
            "type":"thinking",
            "thinking": if display == "omitted" { "" } else { "Mock reasoning summary: checking the supplied inputs." },
            "signature":format!("mock_sig_{}", opaque(ctx, "thinking"))
        }));
        if setting(ctx, "anthropic_redacted_thinking").and_then(Value::as_bool) == Some(true) {
            blocks.push(json!({"type":"redacted_thinking",
                "data":format!("mock_redacted_{}", opaque(ctx, "redacted"))}));
        }
    }
    let mut stop = "end_turn";
    let mut sequence = Value::Null;
    if max_tokens == 0 {
        stop = "max_tokens";
    } else if wants_tool {
        let tools = body["tools"]
            .as_array()
            .filter(|tools| !tools.is_empty())
            .ok_or_else(|| {
                ApiError::unsupported("Tool fixtures require at least one client tool")
            })?;
        let selected_tools: Vec<_> = if choice == "tool" {
            tools
                .iter()
                .filter(|tool| tool["name"] == body["tool_choice"]["name"])
                .collect()
        } else {
            let count = if scenario == Scenario::ParallelTools
                && body["tool_choice"]["disable_parallel_tool_use"] != true
            {
                2
            } else {
                1
            };
            tools.iter().cycle().take(count).collect()
        };
        for (index, tool) in selected_tools.into_iter().enumerate() {
            let input = if scenario == Scenario::MalformedToolArguments {
                json!({"__mock_invalid_arguments":true})
            } else {
                super::schema::sample(&tool["input_schema"], ctx.fixture.tool_arguments.as_ref())?
            };
            if !input.is_object() {
                return Err(ApiError::unsupported(
                    "Anthropic tool arguments must be an object",
                ));
            }
            blocks.push(json!({"type":"tool_use","id":format!("toolu_{}_{}", &opaque(ctx, "tool")[..20], index),
                "name":tool["name"],"input":input}));
        }
        stop = "tool_use";
    } else if scenario != Scenario::Empty {
        let mut text = ctx.fixture.text.clone();
        if scenario == Scenario::Structured || body.pointer("/output_config/format").is_some() {
            let schema = body
                .pointer("/output_config/format/schema")
                .cloned()
                .unwrap_or(json!({"type":"object"}));
            text = super::schema::sample(&schema, ctx.fixture.output.as_ref())?.to_string();
        }
        if scenario == Scenario::Refusal {
            stop = "refusal";
            text = "I cannot help with that request.".into();
        } else if scenario == Scenario::Truncated {
            stop = "max_tokens";
            text = text
                .chars()
                .take(text.chars().count().div_ceil(2))
                .collect();
        } else if scenario == Scenario::StopSequence {
            let first = body["stop_sequences"]
                .as_array()
                .and_then(|v| v.first())
                .ok_or_else(|| {
                    ApiError::unsupported("stop_sequence fixture requires stop_sequences")
                })?;
            sequence = first.clone();
            stop = "stop_sequence";
            if let Some(position) = text.find(first.as_str().unwrap_or("")) {
                text.truncate(position);
            }
        } else if scenario == Scenario::ContextOverflow {
            stop = "model_context_window_exceeded";
        }
        blocks.push(json!({"type":"text","text":text}));
    }
    let sequences = body["stop_sequences"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let (output_tokens, clipped, matched) = limit_output(&mut blocks, sequences, max_tokens);
    if clipped {
        stop = "max_tokens";
        sequence = Value::Null;
    } else if let Some(matched) = matched {
        stop = "stop_sequence";
        sequence = json!(matched);
    }
    let usage = json!({"input_tokens":16,"cache_creation_input_tokens":0,
        "cache_read_input_tokens":0,"output_tokens":output_tokens});
    let message = json!({"id":format!("msg_{}", &opaque(ctx, "message")[..24]),"type":"message",
        "role":"assistant","model":ctx.model.id,"content":blocks,"stop_reason":stop,
        "stop_sequence":sequence,"stop_details":null,"usage":usage});
    let mut response = if ctx.stream {
        stream(ctx, &message)
    } else {
        WireResponse::json(message.clone())
    };
    response
        .headers
        .insert("request-id".into(), ctx.request_id.into());
    if !state.is_object() {
        *state = json!({});
    }
    if !state["anthropic"].is_object() {
        state["anthropic"] = json!({"protected":[]});
    }
    if !state["anthropic"]["protected"].is_array() {
        state["anthropic"]["protected"] = json!([]);
    }
    let protected_blocks: Vec<_> = blocks.iter().filter(|b| protected(b)).cloned().collect();
    state["anthropic"]["protected"]
        .as_array_mut()
        .unwrap()
        .extend(protected_blocks.clone());
    state["anthropic"]["pending"] = if stop == "tool_use" {
        json!({"ids":blocks.iter().filter(|b| b["type"] == "tool_use").map(|b| b["id"].clone()).collect::<Vec<_>>(),
            "protected":protected_blocks,"thinking_mode":selected})
    } else {
        Value::Null
    };
    Ok(response)
}

fn limit_output(
    blocks: &mut Vec<Value>,
    sequences: &[Value],
    budget: u64,
) -> (u64, bool, Option<String>) {
    // One Unicode scalar is one synthetic output token; opaque protection metadata is excluded.
    let mut used = 0;
    for index in 0..blocks.len() {
        let remaining = budget.saturating_sub(used);
        let block = &mut blocks[index];
        let key = match block["type"].as_str() {
            Some("text") => "text",
            Some("thinking") => "thinking",
            Some("tool_use") => {
                let count = block["input"].to_string().chars().count() as u64;
                if count > remaining {
                    // A native JSON tool input is an object, so an incomplete fixture is dropped.
                    blocks.truncate(index);
                    return (used, true, None);
                }
                used += count;
                continue;
            }
            _ => continue,
        };
        let text = block[key].as_str().unwrap_or("");
        let matched = if key == "text" {
            sequences
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|sequence| text.find(sequence).map(|position| (position, sequence)))
                .min_by_key(|(position, _)| *position)
        } else {
            None
        };
        let visible = &text[..matched.map_or(text.len(), |(position, _)| position)];
        let count = visible.chars().count() as u64;
        if count > remaining {
            block[key] = json!(visible.chars().take(remaining as usize).collect::<String>());
            blocks.truncate(index + 1);
            return (budget, true, None);
        }
        used += count;
        if let Some((_, sequence)) = matched {
            let sequence = sequence.to_owned();
            block[key] = json!(visible);
            blocks.truncate(index + 1);
            return (used, false, Some(sequence));
        }
    }
    (used, false, None)
}

fn chunks(text: &str) -> Vec<String> {
    let chars: Vec<_> = text.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars
        .chunks(8)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn stream(ctx: &RequestContext<'_>, message: &Value) -> WireResponse {
    let mut start = message.clone();
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    start["stop_sequence"] = Value::Null;
    start["usage"]["output_tokens"] = json!(0);
    let mut events = vec![Frame::named(
        "message_start",
        json!({"type":"message_start","message":start}),
    )];
    events.push(Frame::named("ping", json!({"type":"ping"})));
    if ctx.fixture.scenario == Scenario::UnknownEvent {
        events.push(Frame::named(
            "mock_future_event",
            json!({"type":"mock_future_event","value":true}),
        ));
    }
    for (index, block) in message["content"].as_array().unwrap().iter().enumerate() {
        let mut initial = block.clone();
        let kind = block["type"].as_str().unwrap();
        match kind {
            "text" => initial["text"] = json!(""),
            "thinking" => {
                initial["thinking"] = json!("");
                initial["signature"] = json!("");
            }
            "tool_use" => initial["input"] = json!({}),
            _ => {}
        }
        events.push(Frame::named(
            "content_block_start",
            json!({"type":"content_block_start","index":index,"content_block":initial}),
        ));
        let deltas: Vec<Value> = match kind {
            "text" => chunks(block["text"].as_str().unwrap())
                .into_iter()
                .map(|text| json!({"type":"text_delta","text":text}))
                .collect(),
            "thinking" => chunks(block["thinking"].as_str().unwrap())
                .into_iter()
                .map(|text| json!({"type":"thinking_delta","thinking":text}))
                .chain(std::iter::once(
                    json!({"type":"signature_delta","signature":block["signature"]}),
                ))
                .collect(),
            "tool_use" => {
                let arguments = if ctx.fixture.scenario == Scenario::MalformedToolArguments {
                    "{\"mock\":".into()
                } else {
                    block["input"].to_string()
                };
                std::iter::once(json!({"type":"input_json_delta","partial_json":""}))
                    .chain(
                        chunks(&arguments)
                            .into_iter()
                            .map(|text| json!({"type":"input_json_delta","partial_json":text})),
                    )
                    .collect()
            }
            _ => Vec::new(),
        };
        for delta in deltas {
            events.push(Frame::named(
                "content_block_delta",
                json!({"type":"content_block_delta","index":index,"delta":delta}),
            ));
            if ctx.fixture.scenario == Scenario::StreamError && kind == "text" {
                events.push(Frame::named("error", json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}})));
                return WireResponse::sse(events);
            }
        }
        events.push(Frame::named(
            "content_block_stop",
            json!({"type":"content_block_stop","index":index}),
        ));
    }
    events.push(Frame::named("message_delta", json!({"type":"message_delta",
        "delta":{"stop_reason":message["stop_reason"],"stop_sequence":message["stop_sequence"],"stop_details":message["stop_details"]},
        "usage":{"output_tokens":message["usage"]["output_tokens"]}})));
    events.push(Frame::named("message_stop", json!({"type":"message_stop"})));
    WireResponse::sse(events)
}

pub fn error(error: &ApiError, request_id: &str) -> WireResponse {
    let error_type = match error.status {
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        413 => "request_too_large",
        429 => "rate_limit_error",
        500 => "api_error",
        504 => "timeout_error",
        529 => "overloaded_error",
        _ => &error.code,
    };
    let message = match &error.param {
        Some(param) => format!("{param}: {}", error.message),
        None => error.message.clone(),
    };
    let mut response = WireResponse::json(json!({"type":"error",
        "error":{"type":error_type,"message":message},"request_id":request_id}));
    response.status = error.status;
    response
        .headers
        .insert("request-id".into(), request_id.into());
    response
}

#[cfg(test)]
mod tests {
    use super::super::model::{Fixture, ModelProfile, Protocol};
    use super::*;
    use axum::http::HeaderMap;

    fn profile(id: &str) -> ModelProfile {
        serde_json::from_value(json!({"id":id,"protocols":["anthropic_messages"],
            "tool_call":true,"reasoning":true,"structured_output":true}))
        .unwrap()
    }
    fn context<'a>(
        model: &'a ModelProfile,
        fixture: &'a Fixture,
        headers: &'a HeaderMap,
        stream: bool,
    ) -> RequestContext<'a> {
        RequestContext {
            provider: "anthropic",
            protocol: Protocol::AnthropicMessages,
            model,
            fixture,
            request_id: "req_test",
            session_id: "test",
            turn: 0,
            seed: 7,
            headers,
            stream,
        }
    }
    fn headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
        headers
    }
    fn request(id: &str) -> Value {
        json!({"model":id,"max_tokens":2048,"messages":[{"role":"user","content":"Check weather"}],
            "tools":[{"name":"weather","input_schema":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}}]})
    }

    #[test]
    fn thinking_tool_roundtrip_rejects_dropped_and_modified_blocks() {
        let mut model = profile("claude-sonnet-4-6");
        model
            .behavior
            .insert("anthropic_redacted_thinking".into(), json!(true));
        let fixture = Fixture {
            scenario: Scenario::ToolCall,
            ..Fixture::default()
        };
        let headers = headers();
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = request(&model.id);
        body["thinking"] = json!({"type":"enabled","budget_tokens":1024});
        let mut state = json!({});
        validate(&ctx, &body, &state).unwrap();
        let response = render(&ctx, &body, &mut state).unwrap();
        let content = response.body["content"].clone();
        let id = content[2]["id"].clone();
        body["messages"].as_array_mut().unwrap().extend([
            json!({"role":"assistant","content":content}),
            json!({"role":"user","content":[{"type":"tool_result","tool_use_id":id,"is_error":true}]}),
        ]);
        validate(&ctx, &body, &state).unwrap();
        let mut damaged = body.clone();
        damaged["messages"][1]["content"][0]["thinking"] = json!("edited");
        assert_eq!(validate(&ctx, &damaged, &state).unwrap_err().status, 400);
        damaged = body.clone();
        damaged["messages"][1]["content"][0]["signature"] = json!("mock_sig_tampered");
        assert_eq!(validate(&ctx, &damaged, &state).unwrap_err().status, 400);
        damaged = body.clone();
        damaged["messages"][1]["content"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert_eq!(validate(&ctx, &damaged, &state).unwrap_err().status, 400);
        damaged = body.clone();
        damaged["messages"][1]["content"]
            .as_array_mut()
            .unwrap()
            .remove(0);
        assert_eq!(validate(&ctx, &damaged, &state).unwrap_err().status, 400);
        let resumed = render(&ctx, &body, &mut state).unwrap();
        assert_eq!(resumed.body["stop_reason"], "end_turn");
        assert!(state["anthropic"]["pending"].is_null());
    }

    #[test]
    fn tool_results_require_adjacency_unique_ids_and_prefix_order() {
        let model = profile("claude-sonnet-4-6");
        let fixture = Fixture::default();
        let headers = headers();
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = request(&model.id);
        body["messages"] = json!([
            {"role":"assistant","content":[{"type":"tool_use","id":"one","name":"weather","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"one"},{"type":"text","text":"continue"}]}
        ]);
        validate(&ctx, &body, &Value::Null).unwrap();
        body["messages"][1]["content"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
        body["messages"][1]["content"] = json!([{"type":"tool_result","tool_use_id":"unknown"}]);
        assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
        body["messages"][1]["content"] = json!([{"type":"tool_result","tool_use_id":"one"},{"type":"tool_result","tool_use_id":"one"}]);
        assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
    }

    #[test]
    fn consecutive_roles_merge_before_tool_result_matching() {
        let model = profile("claude-sonnet-4-6");
        let fixture = Fixture {
            scenario: Scenario::ToolCall,
            ..Fixture::default()
        };
        let headers = headers();
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = request(&model.id);
        body["messages"] = json!([
            {"role":"assistant","content":[{"type":"tool_use","id":"one","name":"weather","input":{}}]},
            {"role":"assistant","content":[{"type":"tool_use","id":"two","name":"weather","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"one"}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"two"}]}
        ]);
        validate(&ctx, &body, &Value::Null).unwrap();
        let response = render(&ctx, &body, &mut Value::Null).unwrap();
        assert_eq!(response.body["stop_reason"], "end_turn");
    }

    #[test]
    fn streaming_tool_arguments_reassemble_and_thinking_signature_precedes_stop() {
        let model = profile("claude-sonnet-4-6");
        let fixture = Fixture {
            scenario: Scenario::ToolCall,
            ..Fixture::default()
        };
        let headers = headers();
        let ctx = context(&model, &fixture, &headers, true);
        let mut body = request(&model.id);
        body["thinking"] = json!({"type":"adaptive"});
        let response = render(&ctx, &body, &mut Value::Null).unwrap();
        let events = response.events.unwrap();
        assert_eq!(
            events.first().unwrap().event.as_deref(),
            Some("message_start")
        );
        assert_eq!(
            events.last().unwrap().event.as_deref(),
            Some("message_stop")
        );
        let data: Vec<Value> = events
            .iter()
            .map(|e| serde_json::from_str(&e.data).unwrap())
            .collect();
        let arguments: String = data
            .iter()
            .filter_map(|d| d.pointer("/delta/partial_json").and_then(Value::as_str))
            .collect();
        assert!(
            serde_json::from_str::<Value>(&arguments)
                .unwrap()
                .is_object()
        );
        let signature = data
            .iter()
            .position(|d| d.pointer("/delta/type") == Some(&json!("signature_delta")))
            .unwrap();
        assert_eq!(data[signature + 1]["type"], "content_block_stop");
        assert!(!events.iter().any(|e| e.data == "[DONE]"));
    }

    #[test]
    fn new_models_reject_manual_thinking_forced_tools_and_prefill() {
        let model = profile("claude-opus-5-5");
        let fixture = Fixture::default();
        let headers = headers();
        let ctx = context(&model, &fixture, &headers, false);
        let body = request(&model.id);
        validate(&ctx, &body, &Value::Null).unwrap();
        for (key, value) in [
            ("thinking", json!({"type":"enabled","budget_tokens":1024})),
            ("thinking", json!({"type":"disabled"})),
            ("tool_choice", json!({"type":"any"})),
            (
                "messages",
                json!([{"role":"assistant","content":"Answer:"}]),
            ),
        ] {
            let mut invalid_body = body.clone();
            invalid_body[key] = value;
            assert_eq!(
                validate(&ctx, &invalid_body, &Value::Null)
                    .unwrap_err()
                    .status,
                400
            );
        }
    }

    #[test]
    fn normal_caps_and_stop_sequences_match_in_json_and_sse() {
        let model = profile("claude-opus-5-5");
        let headers = headers();
        for (text, cap, stops, expected, reason, sequence) in [
            (
                "Mock response: ready ✓ 世界",
                1,
                json!([]),
                "M",
                "max_tokens",
                Value::Null,
            ),
            (
                "Mock response: ready ✓ 世界",
                2048,
                json!(["ready"]),
                "Mock response: ",
                "stop_sequence",
                json!("ready"),
            ),
            (
                "Mock response: ready ✓ 世界",
                4,
                json!(["ready"]),
                "Mock",
                "max_tokens",
                Value::Null,
            ),
            (
                "Mock response: ready ✓ 世界",
                2048,
                json!(["世界", "ready"]),
                "Mock response: ",
                "stop_sequence",
                json!("ready"),
            ),
            ("✓世界abc", 2, json!([]), "✓世", "max_tokens", Value::Null),
            ("✓世界", 3, json!([]), "✓世界", "end_turn", Value::Null),
        ] {
            let fixture = Fixture {
                text: text.into(),
                ..Fixture::default()
            };
            for streaming in [false, true] {
                let ctx = context(&model, &fixture, &headers, streaming);
                let mut body = request(&model.id);
                body["max_tokens"] = json!(cap);
                body["stop_sequences"] = stops.clone();
                validate(&ctx, &body, &Value::Null).unwrap();
                let response = render(&ctx, &body, &mut Value::Null).unwrap();
                if let Some(events) = response.events {
                    let data: Vec<Value> = events
                        .iter()
                        .map(|event| serde_json::from_str(&event.data).unwrap())
                        .collect();
                    let actual: String = data
                        .iter()
                        .filter_map(|event| event.pointer("/delta/text").and_then(Value::as_str))
                        .collect();
                    let end = data
                        .iter()
                        .find(|event| event["type"] == "message_delta")
                        .unwrap();
                    assert_eq!(actual, expected);
                    assert_eq!(end["delta"]["stop_reason"], reason);
                    assert_eq!(end["delta"]["stop_sequence"], sequence);
                    assert_eq!(end["usage"]["output_tokens"], expected.chars().count());
                    assert_eq!(
                        events.last().unwrap().event.as_deref(),
                        Some("message_stop")
                    );
                } else {
                    assert_eq!(response.body["content"][0]["text"], expected);
                    assert_eq!(response.body["stop_reason"], reason);
                    assert_eq!(response.body["stop_sequence"], sequence);
                    assert_eq!(
                        response.body["usage"]["output_tokens"],
                        expected.chars().count()
                    );
                }
            }
        }
    }

    #[test]
    fn normal_text_validates_all_schemas_without_sampling_unused_tools() {
        let model = profile("claude-opus-5-5");
        let headers = headers();
        let fixture = Fixture::default();
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = request(&model.id);
        let malformed = json!({"type":"object","properties":{"x":{"type":123}}});
        body["tools"][0]["input_schema"] = malformed.clone();
        assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
        let pattern = json!({"type":"object","properties":{"x":{"type":"string","pattern":"^[A-Z]{17}$"}},"required":["x"]});
        body["tools"][0]["input_schema"] = pattern.clone();
        validate(&ctx, &body, &Value::Null).unwrap();
        assert_eq!(
            render(&ctx, &body, &mut Value::Null).unwrap().body["content"][0]["text"],
            fixture.text
        );
        body["output_config"] = json!({"format":{"type":"json_schema","schema":malformed}});
        assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
        body["output_config"]["format"]["schema"] = pattern;
        validate(&ctx, &body, &Value::Null).unwrap();
    }

    #[test]
    fn budget_includes_thinking_and_never_registers_an_unemitted_tool() {
        let model = profile("claude-sonnet-4-6");
        let headers = headers();
        let fixture = Fixture {
            scenario: Scenario::Reasoning,
            ..Fixture::default()
        };
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = request(&model.id);
        body["thinking"] = json!({"type":"adaptive"});
        body["max_tokens"] = json!(5);
        validate(&ctx, &body, &Value::Null).unwrap();
        let response = render(&ctx, &body, &mut Value::Null).unwrap();
        assert_eq!(response.body["content"].as_array().unwrap().len(), 1);
        assert_eq!(response.body["content"][0]["thinking"], "Mock ");
        assert_eq!(response.body["usage"]["output_tokens"], 5);
        assert_eq!(response.body["stop_reason"], "max_tokens");

        let fixture = Fixture {
            scenario: Scenario::ToolCall,
            ..Fixture::default()
        };
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = request(&model.id);
        body["max_tokens"] = json!(1);
        let mut state = Value::Null;
        let response = render(&ctx, &body, &mut state).unwrap();
        assert_eq!(response.body["content"], json!([]));
        assert_eq!(response.body["usage"]["output_tokens"], 0);
        assert_eq!(response.body["stop_reason"], "max_tokens");
        assert!(state["anthropic"]["pending"].is_null());
        body["max_tokens"] = json!(2048);
        validate(&ctx, &body, &state).unwrap();
        assert_eq!(
            render(&ctx, &body, &mut state).unwrap().body["stop_reason"],
            "tool_use"
        );
    }

    #[test]
    fn stream_error_ends_with_native_error_without_success_terminator() {
        let model = profile("claude-sonnet-4-6");
        let fixture = Fixture {
            scenario: Scenario::StreamError,
            ..Fixture::default()
        };
        let headers = headers();
        let ctx = context(&model, &fixture, &headers, true);
        let events = render(&ctx, &request(&model.id), &mut Value::Null)
            .unwrap()
            .events
            .unwrap();
        assert_eq!(events.last().unwrap().event.as_deref(), Some("error"));
        assert!(
            !events
                .iter()
                .any(|e| e.event.as_deref() == Some("message_stop"))
        );
    }
}
