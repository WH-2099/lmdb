use serde_json::{Value, json};

use super::model::{ApiError, Frame, RequestContext, Scenario, WireResponse};

fn field<'a>(value: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    value.get(camel).or_else(|| value.get(snake))
}

fn invalid(path: &str, message: &str) -> ApiError {
    ApiError::invalid(path, message)
}

fn parts(content: &Value) -> &[Value] {
    content
        .get("parts")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn call(part: &Value) -> Option<&Value> {
    field(part, "functionCall", "function_call")
}

fn result(part: &Value) -> Option<&Value> {
    field(part, "functionResponse", "function_response")
}

fn signature(part: &Value) -> Option<&str> {
    field(part, "thoughtSignature", "thought_signature").and_then(Value::as_str)
}

fn config(body: &Value) -> &Value {
    field(body, "generationConfig", "generation_config").unwrap_or(&Value::Null)
}

fn thinking(body: &Value) -> &Value {
    field(config(body), "thinkingConfig", "thinking_config").unwrap_or(&Value::Null)
}

fn requires_signature(ctx: &RequestContext<'_>) -> bool {
    ctx.model
        .behavior
        .get("gemini_signature_required")
        .and_then(Value::as_bool)
        .unwrap_or(matches!(
            ctx.model.id.as_str(),
            "gemini-3-flash-preview"
                | "gemini-3.1-pro-preview"
                | "gemini-3.1-flash-lite"
                | "gemini-3.5-flash-lite"
                | "gemini-3.6-flash"
                | "gemini-3.8-flash"
                | "gemini-4-argon"
        ))
}

fn declarations(body: &Value) -> Vec<&Value> {
    body.get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            field(tool, "functionDeclarations", "function_declarations").and_then(Value::as_array)
        })
        .flatten()
        .collect()
}

fn schema(body: &Value) -> Option<&Value> {
    field(config(body), "responseJsonSchema", "response_json_schema")
        .or_else(|| field(config(body), "responseSchema", "response_schema"))
}

fn openapi_schema(schema: &Value) -> Value {
    let mut normalized = schema.clone();
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        normalized["type"] = json!(kind.to_ascii_lowercase());
        if schema.get("nullable") == Some(&Value::Bool(true)) {
            normalized["type"] = json!([kind.to_ascii_lowercase(), "null"]);
        }
    }
    for key in ["properties", "$defs", "definitions"] {
        if let Some(children) = schema.get(key).and_then(Value::as_object) {
            for (name, child) in children {
                normalized[key][name] = openapi_schema(child);
            }
        }
    }
    for key in ["items", "additionalProperties", "not"] {
        if let Some(child) = schema.get(key).filter(|child| child.is_object()) {
            normalized[key] = openapi_schema(child);
        }
    }
    for key in ["anyOf", "oneOf", "allOf", "prefixItems"] {
        if let Some(children) = schema.get(key).and_then(Value::as_array) {
            normalized[key] = Value::Array(children.iter().map(openapi_schema).collect());
        }
    }
    normalized
}

pub fn validate(ctx: &RequestContext<'_>, body: &Value, state: &Value) -> Result<(), ApiError> {
    if !body.is_object() {
        return Err(invalid("body", "The request body must be an object."));
    }
    for (camel, snake) in [
        ("cachedContent", "cached_content"),
        ("continuationToken", "continuation_token"),
    ] {
        if field(body, camel, snake).is_some() {
            return Err(ApiError::unsupported(format!(
                "Gemini {camel} lifecycle is outside this mock contract."
            )));
        }
    }
    if let Some(system) = field(body, "systemInstruction", "system_instruction") {
        if !system.is_object()
            || !system.get("parts").is_some_and(Value::is_array)
            || parts(system).is_empty()
        {
            return Err(invalid(
                "systemInstruction.parts",
                "System instructions require text parts.",
            ));
        }
        for part in parts(system) {
            validate_part(ctx, part)?;
            if !part.get("text").is_some_and(Value::is_string) {
                return Err(invalid(
                    "systemInstruction.parts",
                    "Gemini GenerateContent system instructions accept text only.",
                ));
            }
        }
    }
    let contents = body
        .get("contents")
        .and_then(Value::as_array)
        .filter(|contents| !contents.is_empty())
        .ok_or_else(|| invalid("contents", "A nonempty contents array is required."))?;
    for content in contents {
        if !content.is_object()
            || !content.get("parts").is_some_and(Value::is_array)
            || parts(content).is_empty()
        {
            return Err(invalid(
                "contents.parts",
                "Each content requires a nonempty parts array.",
            ));
        }
        if let Some(role) = content.get("role")
            && !matches!(role.as_str(), Some("user" | "model" | ""))
        {
            return Err(invalid(
                "contents.role",
                "Content role must be user or model.",
            ));
        }
        for part in parts(content) {
            validate_part(ctx, part)?;
            if call(part).is_some() && content.get("role").and_then(Value::as_str) != Some("model")
            {
                return Err(invalid(
                    "contents.role",
                    "Function calls belong to model content.",
                ));
            }
            if result(part).is_some()
                && content.get("role").and_then(Value::as_str) == Some("model")
            {
                return Err(invalid(
                    "contents.role",
                    "Function responses belong to user content.",
                ));
            }
        }
    }
    if let Some(tools) = body.get("tools") {
        let tools = tools
            .as_array()
            .ok_or_else(|| invalid("tools", "Tools must be an array."))?;
        if !tools.is_empty() && ctx.model.tool_call == Some(false) {
            return Err(invalid(
                "tools",
                "This model profile does not support tools.",
            ));
        }
        for tool in tools {
            let functions = field(tool, "functionDeclarations", "function_declarations")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    ApiError::unsupported("This Gemini mock supports functionDeclarations tools.")
                })?;
            for function in functions {
                if !function
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| !name.is_empty())
                {
                    return Err(invalid(
                        "tools.functionDeclarations.name",
                        "A function name is required.",
                    ));
                }
                for name in [
                    "parameters",
                    "parametersJsonSchema",
                    "parameters_json_schema",
                ] {
                    if function.get(name).is_some_and(|value| !value.is_object()) {
                        return Err(invalid(
                            "tools.functionDeclarations.parameters",
                            "Function parameters must be a schema object.",
                        ));
                    }
                }
                let parameters_json =
                    field(function, "parametersJsonSchema", "parameters_json_schema");
                if function.get("parameters").is_some() && parameters_json.is_some() {
                    return Err(invalid(
                        "tools.functionDeclarations.parameters",
                        "Use only one function parameters schema field.",
                    ));
                }
                if let Some(parameters) = parameters_json {
                    super::schema::validate(parameters)?;
                } else if let Some(parameters) = function.get("parameters") {
                    super::schema::validate(&openapi_schema(parameters))?;
                }
            }
        }
    }
    if let Some(tool_config) = field(body, "toolConfig", "tool_config") {
        let fc = field(
            tool_config,
            "functionCallingConfig",
            "function_calling_config",
        )
        .unwrap_or(&Value::Null);
        if let Some(mode) = fc.get("mode").and_then(Value::as_str)
            && !matches!(
                mode,
                "AUTO" | "ANY" | "NONE" | "VALIDATED" | "MODE_UNSPECIFIED"
            )
        {
            return Err(invalid(
                "toolConfig.functionCallingConfig.mode",
                "Unknown function calling mode.",
            ));
        }
        if let Some(names) = field(fc, "allowedFunctionNames", "allowed_function_names") {
            let names = names.as_array().ok_or_else(|| {
                invalid(
                    "toolConfig.allowedFunctionNames",
                    "Allowed function names must be an array.",
                )
            })?;
            if !matches!(
                fc.get("mode").and_then(Value::as_str),
                Some("ANY" | "VALIDATED")
            ) {
                return Err(invalid(
                    "toolConfig.allowedFunctionNames",
                    "Allowed function names require ANY or VALIDATED mode.",
                ));
            }
            let declared = declarations(body);
            for name in names {
                if !name.is_string()
                    || !declared
                        .iter()
                        .any(|function| function.get("name") == Some(name))
                {
                    return Err(invalid(
                        "toolConfig.allowedFunctionNames",
                        "Allowed functions must be declared.",
                    ));
                }
            }
        }
    }
    validate_generation(ctx, body)?;
    validate_history(ctx, contents, state)
}

fn validate_part(ctx: &RequestContext<'_>, part: &Value) -> Result<(), ApiError> {
    if !part.is_object() {
        return Err(invalid("contents.parts", "Each part must be an object."));
    }
    let variants = [
        ("text", "text"),
        ("inlineData", "inline_data"),
        ("fileData", "file_data"),
        ("functionCall", "function_call"),
        ("functionResponse", "function_response"),
        ("executableCode", "executable_code"),
        ("codeExecutionResult", "code_execution_result"),
        ("toolCall", "tool_call"),
        ("toolResponse", "tool_response"),
    ];
    let count = variants
        .iter()
        .filter(|(camel, snake)| field(part, camel, snake).is_some())
        .count();
    if count != 1 {
        return Err(invalid(
            "contents.parts",
            "Each part must contain exactly one data variant.",
        ));
    }
    if let Some(text) = part.get("text")
        && !text.is_string()
    {
        return Err(invalid("contents.parts.text", "Text must be a string."));
    }
    if field(part, "thoughtSignature", "thought_signature").is_some_and(|value| !value.is_string())
    {
        return Err(invalid(
            "contents.parts.thoughtSignature",
            "Thought signatures must be strings.",
        ));
    }
    for (camel, snake) in [("inlineData", "inline_data"), ("fileData", "file_data")] {
        if let Some(media) = field(part, camel, snake) {
            if camel == "fileData"
                && field(media, "mimeType", "mime_type").is_none()
                && field(media, "fileUri", "file_uri")
                    .and_then(Value::as_str)
                    .is_some_and(|uri| !uri.is_empty())
            {
                continue;
            }
            let mime = field(media, "mimeType", "mime_type")
                .and_then(Value::as_str)
                .filter(|mime| mime.contains('/'))
                .ok_or_else(|| invalid("contents.parts.mimeType", "Media requires a MIME type."))?;
            let modality = if mime.starts_with("image/") {
                "image"
            } else if mime.starts_with("audio/") {
                "audio"
            } else if mime.starts_with("video/") {
                "video"
            } else if mime == "application/pdf" {
                "document"
            } else {
                "text"
            };
            if !ctx.model.input_modalities.is_empty()
                && !ctx
                    .model
                    .input_modalities
                    .iter()
                    .any(|value| value == modality || (modality == "document" && value == "pdf"))
            {
                return Err(invalid(
                    "contents.parts",
                    "This model profile does not support the supplied input modality.",
                ));
            }
            if camel == "inlineData" {
                let data = media.get("data").and_then(Value::as_str).ok_or_else(|| {
                    invalid(
                        "contents.parts.inlineData.data",
                        "Inline media requires base64 data.",
                    )
                })?;
                let payload = data.trim_end_matches('=');
                if data.is_empty()
                    || payload.len() % 4 == 1
                    || data.len() - payload.len() > 2
                    || data.contains('=') && data.len() % 4 != 0
                    || !payload
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"+/_-".contains(&byte))
                {
                    return Err(invalid(
                        "contents.parts.inlineData.data",
                        "Inline media must contain base64 data.",
                    ));
                }
            } else if !field(media, "fileUri", "file_uri")
                .and_then(Value::as_str)
                .is_some_and(|uri| !uri.is_empty())
            {
                return Err(invalid(
                    "contents.parts.fileData.fileUri",
                    "File data requires a URI.",
                ));
            }
        }
    }
    for (value, response) in [(call(part), false), (result(part), true)] {
        if let Some(value) = value {
            if !value.is_object()
                || !value
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| !name.is_empty())
            {
                return Err(invalid(
                    "contents.parts.function",
                    "Function calls and responses require a name.",
                ));
            }
            if value.get("id").is_some_and(|id| !id.is_string()) {
                return Err(invalid(
                    "contents.parts.function.id",
                    "Function identifiers must be strings.",
                ));
            }
            if response {
                if !value.get("response").is_some_and(Value::is_object) {
                    return Err(invalid(
                        "contents.parts.functionResponse.response",
                        "Function responses require an object result.",
                    ));
                }
            } else if value.get("args").is_some_and(|args| !args.is_object()) {
                return Err(invalid(
                    "contents.parts.functionCall.args",
                    "Function arguments must be an object.",
                ));
            }
        }
    }
    if variants[5..]
        .iter()
        .any(|(camel, snake)| field(part, camel, snake).is_some())
    {
        return Err(ApiError::unsupported(
            "Built-in Gemini tool execution is outside this mock contract.",
        ));
    }
    Ok(())
}

fn validate_generation(ctx: &RequestContext<'_>, body: &Value) -> Result<(), ApiError> {
    let config = config(body);
    if !config.is_null() && !config.is_object() {
        return Err(invalid(
            "generationConfig",
            "Generation config must be an object.",
        ));
    }
    if let Some(limit) = field(config, "maxOutputTokens", "max_output_tokens") {
        let limit = limit.as_u64().filter(|limit| *limit > 0).ok_or_else(|| {
            invalid(
                "generationConfig.maxOutputTokens",
                "The output token limit must be positive.",
            )
        })?;
        if ctx
            .model
            .max_output_tokens
            .is_some_and(|maximum| limit > maximum)
        {
            return Err(invalid(
                "generationConfig.maxOutputTokens",
                "The output token limit exceeds this model profile.",
            ));
        }
    }
    if let Some(stops) = field(config, "stopSequences", "stop_sequences") {
        let stops = stops.as_array().ok_or_else(|| {
            invalid(
                "generationConfig.stopSequences",
                "Stop sequences must be an array.",
            )
        })?;
        if stops.len() > 5
            || stops
                .iter()
                .any(|stop| !stop.is_string() || stop.as_str() == Some(""))
        {
            return Err(invalid(
                "generationConfig.stopSequences",
                "Supply at most five nonempty stop sequences.",
            ));
        }
    }
    if field(config, "candidateCount", "candidate_count")
        .is_some_and(|count| count.as_u64() != Some(1))
    {
        return Err(ApiError::unsupported(
            "This Gemini mock supports one response candidate per request.",
        ));
    }
    if let Some(modalities) = field(config, "responseModalities", "response_modalities") {
        let modalities = modalities.as_array().ok_or_else(|| {
            invalid(
                "generationConfig.responseModalities",
                "Response modalities must be an array.",
            )
        })?;
        if modalities
            .iter()
            .any(|modality| modality.as_str() != Some("TEXT"))
        {
            return Err(ApiError::unsupported(
                "Gemini media generation is outside this mock contract.",
            ));
        }
    }
    if field(config, "responseFormat", "response_format").is_some() {
        return Err(ApiError::unsupported(
            "Use responseMimeType and responseJsonSchema with this Gemini mock.",
        ));
    }
    let mime = field(config, "responseMimeType", "response_mime_type").and_then(Value::as_str);
    if let Some(mime) = mime
        && !matches!(mime, "text/plain" | "application/json" | "text/x.enum")
    {
        return Err(invalid(
            "generationConfig.responseMimeType",
            "Unsupported response MIME type.",
        ));
    }
    if schema(body).is_some() {
        if ctx.model.structured_output == Some(false) {
            return Err(invalid(
                "generationConfig.responseSchema",
                "This model profile does not support structured output.",
            ));
        }
        if !matches!(mime, Some("application/json" | "text/x.enum")) {
            return Err(invalid(
                "generationConfig.responseMimeType",
                "A response schema requires a compatible response MIME type.",
            ));
        }
        if field(config, "responseSchema", "response_schema").is_some()
            && field(config, "responseJsonSchema", "response_json_schema").is_some()
        {
            return Err(invalid(
                "generationConfig.responseSchema",
                "Use only one response schema field.",
            ));
        }
        if let Some(schema) = field(config, "responseJsonSchema", "response_json_schema") {
            super::schema::validate(schema)?;
        } else if let Some(schema) = field(config, "responseSchema", "response_schema") {
            if !schema.is_object() {
                return Err(invalid(
                    "generationConfig.responseSchema",
                    "The OpenAPI response schema must be an object.",
                ));
            }
            super::schema::validate(&openapi_schema(schema))?;
        }
    }
    let thinking = thinking(body);
    if thinking.is_null() {
        return Ok(());
    }
    if !thinking.is_object() {
        return Err(invalid(
            "generationConfig.thinkingConfig",
            "Thinking config must be an object.",
        ));
    }
    if ctx.model.reasoning == Some(false) {
        return Err(invalid(
            "generationConfig.thinkingConfig",
            "This model profile does not support thinking.",
        ));
    }
    if field(thinking, "includeThoughts", "include_thoughts")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(invalid(
            "generationConfig.thinkingConfig.includeThoughts",
            "Include thoughts must be a boolean.",
        ));
    }
    let budget = field(thinking, "thinkingBudget", "thinking_budget");
    let level = field(thinking, "thinkingLevel", "thinking_level");
    let id = ctx.model.id.as_str();
    if level.is_some()
        && matches!(
            id,
            "gemini-2.5-pro" | "gemini-2.5-flash" | "gemini-2.5-flash-lite"
        )
    {
        return Err(invalid(
            "generationConfig.thinkingConfig.thinkingLevel",
            "Gemini 2.5 requires thinkingBudget, not thinkingLevel.",
        ));
    }
    if id == "gemini-4-argon" && budget.is_some() && level.is_some() {
        return Err(invalid(
            "generationConfig.thinkingConfig",
            "Gemini 4 cannot combine thinkingBudget and thinkingLevel.",
        ));
    }
    if let Some(level) = level {
        let level = level
            .as_str()
            .ok_or_else(|| {
                invalid(
                    "generationConfig.thinkingConfig.thinkingLevel",
                    "Thinking level must be a string.",
                )
            })?
            .to_ascii_lowercase();
        let configured = ctx
            .model
            .behavior
            .get("gemini_thinking_levels")
            .and_then(Value::as_array);
        let allowed = if let Some(levels) = configured {
            levels.iter().any(|value| {
                value
                    .as_str()
                    .is_some_and(|value| value.eq_ignore_ascii_case(&level))
            })
        } else if matches!(
            id,
            "gemini-3.8-flash" | "gemini-4-argon" | "gemini-3.1-pro-preview"
        ) {
            matches!(level.as_str(), "low" | "medium" | "high")
        } else {
            matches!(level.as_str(), "minimal" | "low" | "medium" | "high")
        };
        if !allowed {
            return Err(invalid(
                "generationConfig.thinkingConfig.thinkingLevel",
                "This thinking level is unsupported by the model profile.",
            ));
        }
    }
    if let Some(budget) = budget {
        let budget = budget.as_i64().ok_or_else(|| {
            invalid(
                "generationConfig.thinkingConfig.thinkingBudget",
                "Thinking budget must be an integer.",
            )
        })?;
        let defaults = match id {
            "gemini-2.5-pro" => Some((128, 32768, false)),
            "gemini-2.5-flash" => Some((0, 24576, true)),
            "gemini-2.5-flash-lite" => Some((512, 24576, true)),
            _ => None,
        };
        let min = ctx
            .model
            .behavior
            .get("gemini_thinking_budget_min")
            .and_then(Value::as_i64)
            .or(defaults.map(|value| value.0));
        let max = ctx
            .model
            .behavior
            .get("gemini_thinking_budget_max")
            .and_then(Value::as_i64)
            .or(defaults.map(|value| value.1));
        let disable = ctx
            .model
            .behavior
            .get("gemini_thinking_can_disable")
            .and_then(Value::as_bool)
            .or(defaults.map(|value| value.2));
        if budget < -1
            || (budget == 0 && disable == Some(false))
            || (budget != -1
                && !(budget == 0 && disable == Some(true))
                && (min.is_some_and(|min| budget < min) || max.is_some_and(|max| budget > max)))
        {
            return Err(invalid(
                "generationConfig.thinkingConfig.thinkingBudget",
                "Thinking budget is outside the model profile's supported range.",
            ));
        }
    }
    Ok(())
}

fn validate_history(
    ctx: &RequestContext<'_>,
    contents: &[Value],
    state: &Value,
) -> Result<(), ApiError> {
    let turn_start = contents
        .iter()
        .rposition(|content| {
            content.get("role").and_then(Value::as_str) != Some("model")
                && parts(content).iter().all(|part| result(part).is_none())
        })
        .unwrap_or(0);
    for (index, content) in contents.iter().enumerate() {
        let calls: Vec<_> = parts(content).iter().filter_map(call).collect();
        let results: Vec<_> = parts(content).iter().filter_map(result).collect();
        if !results.is_empty() {
            let prior: Vec<_> = index
                .checked_sub(1)
                .map(|index| parts(&contents[index]).iter().filter_map(call).collect())
                .unwrap_or_default();
            if prior.len() != results.len()
                || prior.iter().zip(&results).any(|(call, result)| {
                    call.get("name") != result.get("name")
                        || call
                            .get("id")
                            .is_some_and(|id| result.get("id") != Some(id))
                })
            {
                return Err(invalid(
                    "contents.functionResponse",
                    "Function responses must follow all parallel calls together, matching their names, order, and issued IDs.",
                ));
            }
        }
        if calls.is_empty() || index < turn_start {
            continue;
        }
        let first_part = parts(content)
            .iter()
            .find(|part| call(part).is_some())
            .expect("nonempty calls");
        if requires_signature(ctx) && signature(first_part).is_none_or(str::is_empty) {
            return Err(invalid(
                "contents.thoughtSignature",
                "The first functionCall in this step is missing a thoughtSignature.",
            ));
        }
        let next_results = contents
            .get(index + 1)
            .map(|content| parts(content).iter().filter_map(result).count())
            .unwrap_or(0);
        if next_results != calls.len() {
            return Err(invalid(
                "contents.functionResponse",
                "Return the complete group of function responses after the model's function calls.",
            ));
        }
        let groups = state.get("gemini_issued").and_then(Value::as_array);
        if let Some(sig) = signature(first_part)
            && sig.starts_with("bW9jay1nZW1pbmkt")
            && !groups.is_some_and(|groups| {
                groups
                    .iter()
                    .any(|group| group["signature"].as_str() == Some(sig))
            })
        {
            return Err(invalid(
                "contents.thoughtSignature",
                "This synthetic thought signature was not issued in the current mock session.",
            ));
        }
        if let Some(groups) = groups {
            for group in groups {
                let issued = group["calls"].as_array().expect("stored calls array");
                let correlated = signature(first_part)
                    .is_some_and(|sig| group["signature"].as_str() == Some(sig))
                    || calls.iter().any(|call| {
                        call.get("id").is_some_and(|id| {
                            issued.iter().any(|issued| issued.get("id") == Some(id))
                        })
                    })
                    || calls.iter().all(|call| call.get("id").is_none())
                        && issued.len() == calls.len()
                        && issued.iter().zip(&calls).all(|(left, right)| {
                            left.get("name") == right.get("name")
                                && left.get("args") == right.get("args")
                        });
                let sig = signature(first_part);
                let skip_signature = matches!(
                    sig,
                    Some(
                        "context_engineering_is_the_way_to_go" | "skip_thought_signature_validator"
                    )
                );
                if correlated
                    && (issued.len() != calls.len()
                        || issued
                            .iter()
                            .zip(&calls)
                            .any(|(left, right)| left != *right)
                        || !skip_signature && sig.is_some() && group["signature"].as_str() != sig)
                {
                    return Err(invalid(
                        "contents.thoughtSignature",
                        "Preserve this mock's issued function calls and thought signature exactly in their original group.",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn mock_signature(ctx: &RequestContext<'_>) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let raw = format!(
        "mock-gemini-{}-{}-{}",
        ctx.seed, ctx.session_id, ctx.request_id
    );
    let mut encoded = String::new();
    for chunk in raw.as_bytes().chunks(3) {
        let n = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | chunk.get(2).copied().unwrap_or(0) as u32;
        encoded.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        encoded.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

// One Unicode scalar is one synthetic output token; signatures are protocol metadata.
fn part_tokens(part: &Value) -> u64 {
    if let Some(text) = part.get("text").and_then(Value::as_str) {
        text.chars().count() as u64
    } else {
        call(part)
            .map(|call| call.to_string().chars().count() as u64)
            .unwrap_or(0)
    }
}

fn limit_output(ctx: &RequestContext<'_>, body: &Value, parts: &mut Vec<Value>) -> bool {
    let Some(mut remaining) = field(config(body), "maxOutputTokens", "max_output_tokens")
        .and_then(Value::as_u64)
        .or(ctx.model.max_output_tokens)
    else {
        return false;
    };
    let mut truncated = false;
    let mut kept = Vec::new();
    for mut part in parts.drain(..) {
        let tokens = part_tokens(&part);
        if tokens > remaining {
            if let Some(text) = part.get("text").and_then(Value::as_str)
                && remaining > 0
            {
                part["text"] = json!(text.chars().take(remaining as usize).collect::<String>());
                kept.push(part);
            }
            // A native functionCall must remain an object, so an incomplete call is omitted.
            truncated = true;
            break;
        }
        remaining -= tokens;
        kept.push(part);
    }
    *parts = kept;
    truncated
}

fn response(ctx: &RequestContext<'_>, parts: Vec<Value>, finish: &str) -> Value {
    let mut output_tokens = 0;
    let mut thought_tokens = 0;
    for part in &parts {
        if part.get("thought") == Some(&Value::Bool(true)) {
            thought_tokens += part_tokens(part);
        } else {
            output_tokens += part_tokens(part);
        }
    }
    json!({"candidates":[{"index":0,"content":{"role":"model","parts":parts},"finishReason":finish}],
        "usageMetadata":{"promptTokenCount":12,"candidatesTokenCount":output_tokens,"thoughtsTokenCount":thought_tokens,"totalTokenCount":12 + output_tokens + thought_tokens},
        "modelVersion":ctx.model.id,"responseId":ctx.request_id})
}

pub fn render(
    ctx: &RequestContext<'_>,
    body: &Value,
    state: &mut Value,
) -> Result<WireResponse, ApiError> {
    let scenario = ctx.fixture.scenario;
    if scenario == Scenario::Refusal {
        let value = json!({"promptFeedback":{"blockReason":"SAFETY","safetyRatings":[{"category":"HARM_CATEGORY_DANGEROUS_CONTENT","probability":"HIGH","blocked":true}]},"usageMetadata":{"promptTokenCount":12,"totalTokenCount":12},"modelVersion":ctx.model.id,"responseId":ctx.request_id});
        return Ok(if ctx.stream {
            WireResponse::sse(vec![Frame::data(value)])
        } else {
            WireResponse::json(value)
        });
    }
    let latest_results = body["contents"]
        .as_array()
        .and_then(|contents| contents.last())
        .is_some_and(|content| parts(content).iter().any(|part| result(part).is_some()));
    let tool_config = field(body, "toolConfig", "tool_config")
        .and_then(|tool| field(tool, "functionCallingConfig", "function_calling_config"));
    let mode = tool_config
        .and_then(|config| config.get("mode"))
        .and_then(Value::as_str);
    let use_tools = mode == Some("ANY")
        || !latest_results
            && mode != Some("NONE")
            && matches!(scenario, Scenario::ToolCall | Scenario::ParallelTools);
    let thoughts = scenario == Scenario::Reasoning
        || field(thinking(body), "includeThoughts", "include_thoughts").and_then(Value::as_bool)
            == Some(true);
    let thinking_enabled = (ctx.model.reasoning == Some(true) || requires_signature(ctx))
        && (requires_signature(ctx)
            || field(thinking(body), "thinkingBudget", "thinking_budget").and_then(Value::as_i64)
                != Some(0));
    if scenario == Scenario::Reasoning && !thinking_enabled {
        return Err(invalid(
            "generationConfig.thinkingConfig",
            "The reasoning fixture requires thinking enabled on this model.",
        ));
    }
    let mut output_parts = Vec::new();
    let mut finish = "STOP";
    if use_tools {
        let allowed = tool_config
            .and_then(|config| field(config, "allowedFunctionNames", "allowed_function_names"))
            .and_then(Value::as_array);
        let functions: Vec<_> = declarations(body)
            .into_iter()
            .filter(|function| allowed.is_none_or(|names| names.contains(&function["name"])))
            .collect();
        if functions.is_empty() {
            return Err(invalid(
                "tools",
                "This fixture requires at least one function declaration.",
            ));
        }
        let count = if scenario == Scenario::ParallelTools {
            2
        } else {
            1
        };
        let sig = mock_signature(ctx);
        let signed = requires_signature(ctx) || thinking_enabled;
        for index in 0..count {
            let function = functions[index % functions.len()];
            let empty = json!({"type":"object","properties":{}});
            let parameters = field(function, "parametersJsonSchema", "parameters_json_schema")
                .cloned()
                .unwrap_or_else(|| openapi_schema(function.get("parameters").unwrap_or(&empty)));
            let args = super::schema::sample(&parameters, ctx.fixture.tool_arguments.as_ref())?;
            if !args.is_object() {
                return Err(invalid(
                    "tools.functionDeclarations.parameters",
                    "Function parameters must generate an object.",
                ));
            }
            let call = json!({"id":format!("call_{}_{}",ctx.request_id,index),"name":function["name"],"args":args});
            let mut part = json!({"functionCall":call});
            if index == 0 && signed {
                part["thoughtSignature"] = json!(sig);
            }
            output_parts.push(part);
        }
    } else if scenario == Scenario::MalformedToolArguments {
        finish = "MALFORMED_FUNCTION_CALL";
    } else if scenario != Scenario::Empty {
        if thoughts && thinking_enabled {
            output_parts.push(json!({"thought":true,"text":"Mock thought summary: checking the supplied request."}));
        }
        let mut text = ctx.fixture.text.clone();
        let mime =
            field(config(body), "responseMimeType", "response_mime_type").and_then(Value::as_str);
        if scenario == Scenario::Structured
            || schema(body).is_some()
            || mime == Some("application/json")
        {
            let empty = json!({"type":"object","properties":{"ready":{"type":"boolean"}},"required":["ready"]});
            let schema = field(config(body), "responseJsonSchema", "response_json_schema")
                .cloned()
                .unwrap_or_else(|| openapi_schema(schema(body).unwrap_or(&empty)));
            let value = super::schema::sample(&schema, ctx.fixture.output.as_ref())?;
            text = if mime == Some("text/x.enum") {
                value.as_str().unwrap_or("").to_owned()
            } else {
                value.to_string()
            };
        }
        if let Some(stops) =
            field(config(body), "stopSequences", "stop_sequences").and_then(Value::as_array)
            && let Some(index) = stops
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|stop| text.find(stop))
                .min()
        {
            text.truncate(index);
        }
        if scenario == Scenario::Truncated {
            finish = "MAX_TOKENS";
            text = text.chars().take(8).collect();
        }
        let mut part = json!({"text":text});
        if requires_signature(ctx) && thinking_enabled {
            part["thoughtSignature"] = json!(mock_signature(ctx));
        }
        output_parts.push(part);
    }
    if limit_output(ctx, body, &mut output_parts) {
        finish = "MAX_TOKENS";
    }
    let issued: Vec<_> = output_parts.iter().filter_map(call).cloned().collect();
    if !issued.is_empty() {
        if !state.is_object() {
            *state = json!({});
        }
        let groups = state
            .as_object_mut()
            .unwrap()
            .entry("gemini_issued")
            .or_insert_with(|| json!([]));
        let sig = output_parts.iter().find_map(signature);
        groups
            .as_array_mut()
            .unwrap()
            .push(json!({"calls":issued,"signature":sig}));
    }
    let value = response(ctx, output_parts.clone(), finish);
    if !ctx.stream {
        return Ok(WireResponse::json(value));
    }
    let mut events = Vec::new();
    if scenario == Scenario::UnknownEvent {
        events.push(Frame::data(
            json!({"mockFutureExtension":{"version":1},"responseId":ctx.request_id}),
        ));
    }
    for part in output_parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            let chars: Vec<_> = text.chars().collect();
            for chunk in chars.chunks(9) {
                let mut chunk_part = json!({"text":chunk.iter().collect::<String>()});
                if part.get("thought") == Some(&Value::Bool(true)) {
                    chunk_part["thought"] = json!(true);
                }
                events.push(Frame::data(json!({"candidates":[{"index":0,"content":{"role":"model","parts":[chunk_part]}}],"modelVersion":ctx.model.id,"responseId":ctx.request_id})));
            }
            if let Some(sig) = signature(&part) {
                events.push(Frame::data(json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"","thoughtSignature":sig}]}}],"modelVersion":ctx.model.id,"responseId":ctx.request_id})));
            }
        } else {
            events.push(Frame::data(json!({"candidates":[{"index":0,"content":{"role":"model","parts":[part]}}],"modelVersion":ctx.model.id,"responseId":ctx.request_id})));
        }
    }
    if scenario == Scenario::StreamError {
        events.push(Frame::data(
            error(
                &ApiError::new(500, "INTERNAL", "Synthetic Gemini stream failure."),
                ctx.request_id,
            )
            .body,
        ));
    } else {
        events.push(Frame::data(json!({"candidates":[{"index":0,"finishReason":finish}],"usageMetadata":value["usageMetadata"],"modelVersion":ctx.model.id,"responseId":ctx.request_id})));
    }
    Ok(WireResponse::sse(events))
}

pub fn error(error: &ApiError, request_id: &str) -> WireResponse {
    let status = match error.status {
        400 | 422 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        402 | 429 => "RESOURCE_EXHAUSTED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        408 | 504 => "DEADLINE_EXCEEDED",
        409 => "ABORTED",
        413 => "INVALID_ARGUMENT",
        499 => "CANCELLED",
        501 => "UNIMPLEMENTED",
        503 => "UNAVAILABLE",
        _ => "INTERNAL",
    };
    let mut value = json!({"error":{"code":error.status,"message":error.message,"status":status}});
    if let Some(param) = &error.param {
        value["error"]["details"] = json!([{"@type":"type.googleapis.com/google.rpc.BadRequest","fieldViolations":[{"field":param,"description":error.message}]}]);
    }
    let mut response = WireResponse::json(value);
    response.status = error.status;
    response
        .headers
        .insert("x-request-id".into(), request_id.into());
    response
}

#[cfg(test)]
mod tests {
    use super::super::model::{Fixture, ModelProfile, Protocol};
    use super::*;
    use axum::http::HeaderMap;

    fn model(id: &str) -> ModelProfile {
        ModelProfile {
            id: id.into(),
            record: None,
            protocols: vec![Protocol::GeminiGenerate],
            input_modalities: vec!["text".into(), "image".into()],
            output_modalities: vec!["text".into()],
            context_window: None,
            max_output_tokens: Some(65536),
            tool_call: Some(true),
            reasoning: Some(true),
            structured_output: Some(true),
            behavior: Default::default(),
            sources: vec![],
            notes: vec![],
        }
    }
    fn context<'a>(
        model: &'a ModelProfile,
        fixture: &'a Fixture,
        headers: &'a HeaderMap,
        stream: bool,
    ) -> RequestContext<'a> {
        RequestContext {
            provider: "google",
            protocol: Protocol::GeminiGenerate,
            model,
            fixture,
            request_id: "req_1",
            session_id: "s1",
            turn: 0,
            seed: 7,
            headers,
            stream,
        }
    }
    fn request() -> Value {
        json!({"contents":[{"role":"user","parts":[{"text":"Hello"}]}]})
    }
    fn tools_request() -> Value {
        let mut body = request();
        body["tools"] = json!([{"functionDeclarations":[{"name":"weather","parameters":{"type":"object","properties":{}}}]}]);
        body
    }
    fn tool_history(body: &mut Value, response: &Value) {
        let content = response["candidates"][0]["content"].clone();
        let replies: Vec<_> = parts(&content).iter().filter_map(call).map(|call| json!({"functionResponse":{"name":call["name"],"id":call["id"],"response":{"ready":true}}})).collect();
        body["contents"].as_array_mut().unwrap().push(content);
        body["contents"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","parts":replies}));
    }
    #[test]
    fn user_images_and_system_text_have_different_constraints() {
        let model = model("gemini-2.5-flash");
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = request();
        body["contents"][0]["parts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}}));
        assert!(validate(&ctx, &body, &Value::Null).is_ok());
        body["systemInstruction"] =
            json!({"parts":[{"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}}]});
        assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
        body["systemInstruction"] = json!({"role":"system","parts":[{"text":"Be concise"}]});
        assert!(validate(&ctx, &body, &Value::Null).is_ok());
    }
    #[test]
    fn thinking_settings_follow_exact_model_profiles() {
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let mut body = request();
        body["generationConfig"] = json!({"thinkingConfig":{"thinkingBudget":0}});
        assert!(
            validate(
                &context(&model("gemini-2.5-flash"), &fixture, &headers, false),
                &body,
                &Value::Null
            )
            .is_ok()
        );
        assert!(
            validate(
                &context(&model("gemini-2.5-pro"), &fixture, &headers, false),
                &body,
                &Value::Null
            )
            .is_err()
        );
        body["generationConfig"] = json!({"thinkingConfig":{"thinkingLevel":"LOW"}});
        assert!(
            validate(
                &context(&model("gemini-2.5-flash"), &fixture, &headers, false),
                &body,
                &Value::Null
            )
            .is_err()
        );
        assert!(
            validate(
                &context(&model("gemini-3-flash-preview"), &fixture, &headers, false),
                &body,
                &Value::Null
            )
            .is_ok()
        );
        body["generationConfig"] =
            json!({"thinkingConfig":{"thinkingLevel":"LOW","thinkingBudget":1024}});
        assert!(
            validate(
                &context(&model("gemini-4-argon"), &fixture, &headers, false),
                &body,
                &Value::Null
            )
            .is_err()
        );
    }
    #[test]
    fn parallel_calls_require_grouped_results_and_current_turn_signatures() {
        let model = model("gemini-3-flash-preview");
        let fixture = Fixture {
            scenario: Scenario::ParallelTools,
            ..Default::default()
        };
        let headers = HeaderMap::new();
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = tools_request();
        let mut state = Value::Null;
        let response = render(&ctx, &body, &mut state).unwrap();
        tool_history(&mut body, &response.body);
        assert!(signature(&body["contents"][1]["parts"][0]).is_some());
        assert!(signature(&body["contents"][1]["parts"][1]).is_none());
        assert!(validate(&ctx, &body, &state).is_ok());
        assert!(validate(&ctx, &body, &Value::Null).is_err());
        let mut imported = body.clone();
        imported["contents"][1]["parts"][0]["thoughtSignature"] =
            json!("external-provider-signature");
        assert!(validate(&ctx, &imported, &Value::Null).is_ok());
        let mut changed = body.clone();
        changed["contents"][1]["parts"][0]["thoughtSignature"] = json!("changed");
        assert!(validate(&ctx, &changed, &state).is_err());
        let mut missing = body.clone();
        missing["contents"][1]["parts"][0]
            .as_object_mut()
            .unwrap()
            .remove("thoughtSignature");
        assert!(validate(&ctx, &missing, &state).is_err());
        let mut mixed = missing.clone();
        mixed["contents"][2]["parts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"text":"Also explain the result"}));
        assert!(validate(&ctx, &mixed, &state).is_err());
        missing["contents"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","parts":[{"text":"Start a new turn"}]}));
        assert!(validate(&ctx, &missing, &state).is_ok());
        let mut interleaved = body.clone();
        interleaved["contents"] = json!([body["contents"][0],{"role":"model","parts":[body["contents"][1]["parts"][0]]},{"role":"user","parts":[body["contents"][2]["parts"][0]]},{"role":"model","parts":[body["contents"][1]["parts"][1]]},{"role":"user","parts":[body["contents"][2]["parts"][1]]}]);
        assert!(validate(&ctx, &interleaved, &state).is_err());
        let mut wrong_id = body.clone();
        wrong_id["contents"][2]["parts"][0]["functionResponse"]["id"] = json!("wrong");
        assert!(validate(&ctx, &wrong_id, &state).is_err());
    }
    #[test]
    fn older_model_allows_omitting_its_issued_signature() {
        let model = model("gemini-2.5-flash");
        let fixture = Fixture {
            scenario: Scenario::ToolCall,
            ..Default::default()
        };
        let headers = HeaderMap::new();
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = tools_request();
        let mut state = Value::Null;
        let response = render(&ctx, &body, &mut state).unwrap();
        tool_history(&mut body, &response.body);
        body["contents"][1]["parts"][0]
            .as_object_mut()
            .unwrap()
            .remove("thoughtSignature");
        assert!(validate(&ctx, &body, &state).is_ok());
    }

    #[test]
    fn forced_tool_mode_continues_calling_after_function_results() {
        let model = model("gemini-3-flash-preview");
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let ctx = context(&model, &fixture, &headers, false);
        let mut body = tools_request();
        body["toolConfig"] = json!({"functionCallingConfig":{"mode":"ANY"}});
        let mut state = Value::Null;
        let first = render(&ctx, &body, &mut state).unwrap();
        tool_history(&mut body, &first.body);
        assert!(validate(&ctx, &body, &state).is_ok());
        let mut next = context(&model, &fixture, &headers, false);
        next.request_id = "req_2";
        let response = render(&next, &body, &mut state).unwrap();
        let content = &response.body["candidates"][0]["content"];
        assert_eq!(parts(content).iter().filter_map(call).count(), 1);
        assert_eq!(call(&parts(content)[0]).unwrap()["id"], "call_req_2_0");
        assert!(parts(content).iter().all(|part| part.get("text").is_none()));
    }

    #[test]
    fn output_limits_apply_to_json_and_sse_with_matching_synthetic_usage() {
        let mut model = model("gemini-3-flash-preview");
        model.max_output_tokens = Some(2);
        let fixture = Fixture {
            text: "世界🙂abc".into(),
            ..Default::default()
        };
        let headers = HeaderMap::new();
        for stream in [false, true] {
            for requested in [None, Some(1)] {
                let ctx = context(&model, &fixture, &headers, stream);
                let mut body = request();
                if let Some(limit) = requested {
                    body["generationConfig"] = json!({"maxOutputTokens":limit});
                }
                assert!(validate(&ctx, &body, &Value::Null).is_ok());
                let response = render(&ctx, &body, &mut Value::Null).unwrap();
                let values: Vec<Value> = response
                    .events
                    .map(|events| {
                        events
                            .into_iter()
                            .map(|event| serde_json::from_str(&event.data).unwrap())
                            .collect()
                    })
                    .unwrap_or_else(|| vec![response.body]);
                let visible: String = values
                    .iter()
                    .flat_map(|value| parts(&value["candidates"][0]["content"]).iter())
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect();
                assert_eq!(visible, if requested.is_some() { "世" } else { "世界" });
                let last = values.last().unwrap();
                assert_eq!(last["candidates"][0]["finishReason"], "MAX_TOKENS");
                assert_eq!(
                    last["usageMetadata"]["candidatesTokenCount"],
                    requested.unwrap_or(2)
                );
                assert_eq!(last["usageMetadata"]["thoughtsTokenCount"], 0);
                assert_eq!(
                    last["usageMetadata"]["totalTokenCount"],
                    12 + requested.unwrap_or(2)
                );
            }
        }
        let ctx = context(&model, &fixture, &headers, false);
        for invalid_limit in [json!(0), json!(-1), json!(1.5), json!("1"), json!(3)] {
            let mut body = request();
            body["generationConfig"] = json!({"maxOutputTokens":invalid_limit});
            assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
        }
    }

    #[test]
    fn budget_includes_thoughts_and_never_remembers_unemitted_calls() {
        let model = model("gemini-3-flash-preview");
        let headers = HeaderMap::new();
        let reasoning = Fixture {
            scenario: Scenario::Reasoning,
            ..Default::default()
        };
        let mut body = request();
        body["generationConfig"] = json!({"maxOutputTokens":1});
        let response = render(
            &context(&model, &reasoning, &headers, false),
            &body,
            &mut Value::Null,
        )
        .unwrap();
        assert_eq!(response.body["candidates"][0]["finishReason"], "MAX_TOKENS");
        assert_eq!(
            response.body["candidates"][0]["content"]["parts"],
            json!([{"thought":true,"text":"M"}])
        );
        assert_eq!(response.body["usageMetadata"]["thoughtsTokenCount"], 1);
        assert_eq!(response.body["usageMetadata"]["candidatesTokenCount"], 0);

        let tools = Fixture {
            scenario: Scenario::ToolCall,
            ..Default::default()
        };
        let mut body = tools_request();
        body["generationConfig"] = json!({"maxOutputTokens":1});
        let mut state = Value::Null;
        let response =
            render(&context(&model, &tools, &headers, false), &body, &mut state).unwrap();
        assert_eq!(response.body["candidates"][0]["finishReason"], "MAX_TOKENS");
        assert_eq!(
            response.body["candidates"][0]["content"]["parts"],
            json!([])
        );
        assert_eq!(response.body["usageMetadata"]["candidatesTokenCount"], 0);
        assert!(state.get("gemini_issued").is_none());
    }

    #[test]
    fn openapi_schema_normalizes_types_without_changing_user_defaults() {
        let schema = json!({"type":"OBJECT","properties":{"kind":{"type":"STRING","nullable":true}},"default":{"type":"OBJECT"}});
        let normalized = openapi_schema(&schema);
        assert_eq!(normalized["type"], "object");
        assert_eq!(
            normalized["properties"]["kind"]["type"],
            json!(["string", "null"])
        );
        assert_eq!(normalized["default"]["type"], "OBJECT");
    }

    #[test]
    fn schemas_are_validated_before_choosing_text_or_tool_output() {
        let model = model("gemini-3-flash-preview");
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let ctx = context(&model, &fixture, &headers, false);
        for schema_field in ["parameters", "parametersJsonSchema"] {
            let mut body = tools_request();
            body["tools"][0]["functionDeclarations"][0]
                .as_object_mut()
                .unwrap()
                .remove("parameters");
            body["tools"][0]["functionDeclarations"][0][schema_field] = json!({"type":123});
            assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
        }
        for schema_field in ["responseSchema", "responseJsonSchema"] {
            let mut body = tools_request();
            body["toolConfig"] = json!({"functionCallingConfig":{"mode":"ANY"}});
            body["generationConfig"] = json!({"responseMimeType":"application/json"});
            body["generationConfig"][schema_field] = json!({"type":123});
            assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
        }
        let mut body = tools_request();
        body["tools"][0]["functionDeclarations"][0]["parameters"] =
            json!({"type":"OBJECT","properties":{"name":{"type":"STRING"}}});
        assert!(validate(&ctx, &body, &Value::Null).is_ok());
        body["generationConfig"] =
            json!({"responseMimeType":"application/json","responseSchema":false});
        assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
        body.as_object_mut().unwrap().remove("generationConfig");
        body["tools"][0]["functionDeclarations"][0]["parametersJsonSchema"] =
            json!({"type":"object"});
        assert_eq!(validate(&ctx, &body, &Value::Null).unwrap_err().status, 400);
    }

    #[test]
    fn streaming_preserves_empty_text_signature_and_native_finish() {
        let model = model("gemini-3-flash-preview");
        let fixture = Fixture::default();
        let headers = HeaderMap::new();
        let ctx = context(&model, &fixture, &headers, true);
        let response = render(&ctx, &request(), &mut Value::Null).unwrap();
        let frames = response.events.unwrap();
        assert!(
            frames
                .iter()
                .all(|frame| frame.event.is_none() && frame.data != "[DONE]")
        );
        let values: Vec<Value> = frames
            .iter()
            .map(|frame| serde_json::from_str(&frame.data).unwrap())
            .collect();
        assert!(values.iter().any(
            |value| value["candidates"][0]["content"]["parts"][0]["text"] == ""
                && value["candidates"][0]["content"]["parts"][0]["thoughtSignature"].is_string()
        ));
        assert_eq!(
            values.last().unwrap()["candidates"][0]["finishReason"],
            "STOP"
        );
    }
    #[test]
    fn blocked_prompt_has_no_candidates_and_402_is_distinct_from_429() {
        let model = model("gemini-2.5-flash");
        let fixture = Fixture {
            scenario: Scenario::Refusal,
            ..Default::default()
        };
        let headers = HeaderMap::new();
        let response = render(
            &context(&model, &fixture, &headers, false),
            &request(),
            &mut Value::Null,
        )
        .unwrap();
        assert!(response.body.get("candidates").is_none());
        for status in [402, 429] {
            let response = error(
                &ApiError::new(status, "quota", "Synthetic quota fixture"),
                "req",
            );
            assert_eq!(response.status, status);
            assert_eq!(response.body["error"]["status"], "RESOURCE_EXHAUSTED");
        }
    }
}
