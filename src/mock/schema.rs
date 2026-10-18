use jsonschema::{Retrieve, Uri, ValidatorMap};
use serde_json::{Map, Value, json};

use super::model::ApiError;

const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 4096;
const MAX_BYTES: usize = 256 * 1024;
const MAX_ITEMS: usize = 64;
const MAX_TEXT: usize = 1024;
const MAX_STEPS: usize = 512;

struct NoRetrieval;

impl Retrieve for NoRetrieval {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Err(format!("External schema retrieval is disabled: {uri}").into())
    }
}

fn options() -> jsonschema::ValidationOptions<'static> {
    jsonschema::options()
        .with_retriever(NoRetrieval)
        .with_pattern_options(
            jsonschema::PatternOptions::fancy_regex()
                .backtrack_limit(100_000)
                .size_limit(1024 * 1024),
        )
}

fn compile(schema: &Value) -> Result<jsonschema::Validator, ApiError> {
    check_size(schema, "schema")?;
    check_references(schema, "#")?;
    check_work(schema, schema, &mut Vec::new(), &mut 8192, 0)?;
    options().build(schema).map_err(|error| {
        ApiError::invalid(
            "schema",
            format!("Invalid JSON Schema at {}: {error}", error.instance_path()),
        )
    })
}

/// Validate schema syntax, references, and work limits without generating a value.
pub fn validate(schema: &Value) -> Result<(), ApiError> {
    compile(schema).map(|_| ())
}

/// Produce a deterministic example, or validate an explicit fixture unchanged.
pub fn sample(schema: &Value, supplied: Option<&Value>) -> Result<Value, ApiError> {
    let validator = compile(schema)?;
    if let Some(value) = supplied {
        check_size(value, "mock_fixture")?;
        validator.validate(value).map_err(|error| {
            ApiError::invalid(
                "mock_fixture",
                format!(
                    "Fixture value at {} does not match schema {}: {error}",
                    error.instance_path(),
                    error.schema_path(),
                ),
            )
        })?;
        return Ok(value.clone());
    }

    let validators = options()
        .build_map(schema)
        .map_err(|error| ApiError::invalid("schema", format!("Invalid JSON Schema: {error}")))?;
    let value = Sampler {
        schema,
        validators,
        remaining: MAX_STEPS,
    }
    .generate("#", 0);
    if let Some(value) = value.filter(|value| validator.is_valid(value)) {
        return Ok(value);
    }
    Err(ApiError::unsupported(
        "Cannot generate a valid example within the mock schema limits; provide an explicit mock_fixture.output or mock_fixture.tool_arguments matching the schema.",
    ))
}

fn check_size(value: &Value, param: &str) -> Result<(), ApiError> {
    let mut stack = vec![(value, 0)];
    let mut nodes = 0;
    let mut bytes = 0;
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if depth > MAX_DEPTH || nodes > MAX_NODES {
            return Err(ApiError::invalid(
                param,
                "JSON exceeds the mock limit of 32 levels or 4096 values",
            ));
        }
        let children = match value {
            Value::Object(object) => object.len(),
            Value::Array(array) => array.len(),
            _ => 0,
        };
        if children > MAX_NODES - nodes || stack.len() > MAX_NODES - nodes - children {
            return Err(ApiError::invalid(
                param,
                "JSON exceeds the mock limit of 4096 values",
            ));
        }
        match value {
            Value::Object(object) => {
                bytes += object.keys().map(String::len).sum::<usize>();
                stack.extend(object.values().map(|value| (value, depth + 1)));
            }
            Value::Array(array) => stack.extend(array.iter().map(|value| (value, depth + 1))),
            Value::String(text) => bytes += text.len(),
            _ => {}
        }
        if bytes > MAX_BYTES || stack.len() > MAX_NODES {
            return Err(ApiError::invalid(
                param,
                "JSON exceeds the mock limit of 256 KiB of text or 4096 values",
            ));
        }
    }
    Ok(())
}

fn child(path: &str, key: &str) -> String {
    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn check_references(schema: &Value, path: &str) -> Result<(), ApiError> {
    let Some(object) = schema.as_object() else {
        return Ok(());
    };
    if path != "#" && (object.contains_key("$id") || object.contains_key("id")) {
        return Err(ApiError::invalid(
            "schema",
            format!(
                "Subschema identifiers at {path} are unsupported; keep references relative to the root schema",
            ),
        ));
    }
    for keyword in ["$ref", "$dynamicRef", "$recursiveRef"] {
        if let Some(reference) = object.get(keyword).and_then(Value::as_str) {
            if !reference.is_empty() && !reference.starts_with('#') {
                return Err(ApiError::invalid(
                    "schema",
                    format!(
                        "External reference at {path}/{keyword} is disabled; use a local #/$defs reference",
                    ),
                ));
            }
            if reference_path(reference).is_none() {
                return Err(ApiError::invalid(
                    "schema",
                    format!(
                        "Reference at {path}/{keyword} must be a local JSON Pointer such as #/$defs/entry",
                    ),
                ));
            }
        }
    }
    for (schema, path) in subschemas(schema, path) {
        check_references(schema, &path)?;
    }
    Ok(())
}

fn subschemas<'a>(schema: &'a Value, path: &str) -> Vec<(&'a Value, String)> {
    let mut children = Vec::new();
    let Some(object) = schema.as_object() else {
        return children;
    };
    // Only schema positions are inspected: const/enum can contain ordinary data named "$ref".
    for (key, value) in object {
        let path = child(path, key);
        match key.as_str() {
            "$defs" | "definitions" | "properties" | "patternProperties" | "dependentSchemas"
            | "dependencies" => {
                if let Some(schemas) = value.as_object() {
                    for (name, schema) in schemas {
                        children.push((schema, child(&path, name)));
                    }
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                if let Some(schemas) = value.as_array() {
                    for (index, schema) in schemas.iter().enumerate() {
                        children.push((schema, child(&path, &index.to_string())));
                    }
                }
            }
            "items" if value.is_array() => {
                for (index, schema) in value.as_array().unwrap().iter().enumerate() {
                    children.push((schema, child(&path, &index.to_string())));
                }
            }
            "items"
            | "additionalItems"
            | "additionalProperties"
            | "contains"
            | "propertyNames"
            | "not"
            | "if"
            | "then"
            | "else"
            | "unevaluatedItems"
            | "unevaluatedProperties"
            | "contentSchema" => {
                children.push((value, path));
            }
            _ => {}
        }
    }
    children
}

fn reference_path(reference: &str) -> Option<String> {
    let fragment = if reference.is_empty() {
        ""
    } else {
        reference.strip_prefix('#')?
    };
    let mut bytes = Vec::with_capacity(fragment.len());
    let mut input = fragment.bytes();
    while let Some(byte) = input.next() {
        bytes.push(if byte == b'%' {
            let high = (input.next()? as char).to_digit(16)?;
            let low = (input.next()? as char).to_digit(16)?;
            (high * 16 + low) as u8
        } else {
            byte
        });
    }
    let pointer = String::from_utf8(bytes).ok()?;
    (pointer.is_empty() || pointer.starts_with('/')).then(|| format!("#{pointer}"))
}

fn check_work(
    root: &Value,
    schema: &Value,
    references: &mut Vec<String>,
    remaining: &mut usize,
    depth: usize,
) -> Result<(), ApiError> {
    if depth > 128 || *remaining == 0 {
        return Err(ApiError::invalid(
            "schema",
            "Schema reference expansion exceeds the mock validation work limit",
        ));
    }
    *remaining -= 1;
    for keyword in ["$ref", "$dynamicRef", "$recursiveRef"] {
        if let Some(path) = schema
            .get(keyword)
            .and_then(Value::as_str)
            .and_then(reference_path)
        {
            // Fixtures and generated values have bounded depth, so recursive instance descent is finite.
            if references
                .iter()
                .filter(|previous| **previous == path)
                .count()
                >= MAX_DEPTH
            {
                continue;
            }
            if let Some(target) = root.pointer(&path[1..]) {
                check_references(target, &path)?;
                references.push(path);
                check_work(root, target, references, remaining, depth + 1)?;
                references.pop();
            }
        }
    }
    for (schema, _) in subschemas(schema, "#") {
        check_work(root, schema, references, remaining, depth + 1)?;
    }
    Ok(())
}

struct Sampler<'a> {
    schema: &'a Value,
    validators: ValidatorMap,
    remaining: usize,
}

impl Sampler<'_> {
    fn valid(&self, path: &str, value: &Value) -> bool {
        generated_size(value)
            && self
                .validators
                .get(path)
                .is_some_and(|validator| validator.is_valid(value))
    }

    fn generate(&mut self, path: &str, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH || self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let schema = self.schema.pointer(path.strip_prefix('#')?)?;
        if let Some(value) = schema.get("const").filter(|value| self.valid(path, value)) {
            return Some(value.clone());
        }
        if let Some(values) = schema.get("enum").and_then(Value::as_array) {
            return values.iter().find(|value| self.valid(path, value)).cloned();
        }

        let mut candidates = self.simple(schema, path, depth);
        let base = candidates.first().cloned();
        if let Some(reference) = schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(reference_path)
            && let Some(value) = self.generate(&reference, depth + 1)
        {
            candidates.push(value.clone());
            if let Some(base) = &base {
                candidates.push(merge(base.clone(), value));
            }
        }
        for keyword in ["anyOf", "oneOf", "allOf"] {
            if let Some(branches) = schema.get(keyword).and_then(Value::as_array) {
                let mut combined = base.clone().unwrap_or(Value::Null);
                for index in 0..branches.len() {
                    let branch = child(&child(path, keyword), &index.to_string());
                    if let Some(value) = self.generate(&branch, depth + 1) {
                        if self.valid(path, &value) {
                            return Some(value);
                        }
                        let merged = merge(base.clone().unwrap_or(Value::Null), value.clone());
                        if self.valid(path, &merged) {
                            return Some(merged);
                        }
                        if keyword == "allOf" {
                            combined = merge(combined, value);
                        }
                    }
                }
                if keyword == "allOf" {
                    candidates.push(combined);
                }
            }
        }
        candidates.into_iter().find(|value| self.valid(path, value))
    }

    fn simple(&mut self, schema: &Value, path: &str, depth: usize) -> Vec<Value> {
        let types: Vec<&str> = match schema.get("type") {
            Some(Value::String(kind)) => vec![kind],
            Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
            _ if schema.get("properties").is_some() || schema.get("required").is_some() => {
                vec!["object"]
            }
            _ if schema.get("items").is_some() || schema.get("prefixItems").is_some() => {
                vec!["array"]
            }
            _ => vec!["null", "object", "array", "string", "integer", "boolean"],
        };
        let mut values = Vec::new();
        for kind in types {
            match kind {
                "null" => values.push(Value::Null),
                "boolean" => values.extend([json!(false), json!(true)]),
                "object" => {
                    if let Some(value) = self.object(schema, path, depth) {
                        values.push(value);
                    }
                }
                "array" => {
                    if let Some(value) = self.array(schema, path, depth) {
                        values.push(value);
                    }
                    values.push(json!([]));
                }
                "string" => {
                    let min = schema.get("minLength").and_then(Value::as_u64).unwrap_or(0);
                    let max = schema
                        .get("maxLength")
                        .and_then(Value::as_u64)
                        .unwrap_or(MAX_TEXT as u64);
                    if min <= max && min <= MAX_TEXT as u64 {
                        let len = 4.max(min as usize).min(max.min(MAX_TEXT as u64) as usize);
                        let mut value: String = "mock".chars().take(len).collect();
                        value.extend(std::iter::repeat_n('x', len.saturating_sub(4)));
                        values.push(Value::String(value));
                    }
                }
                "integer" | "number" => values.extend(numbers(schema, kind == "integer")),
                _ => {}
            }
        }
        values
    }

    fn object(&mut self, schema: &Value, path: &str, depth: usize) -> Option<Value> {
        let properties = schema.get("properties").and_then(Value::as_object);
        let required: Vec<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let max = schema
            .get("maxProperties")
            .and_then(Value::as_u64)
            .unwrap_or(MAX_ITEMS as u64)
            .min(MAX_ITEMS as u64) as usize;
        let min = schema
            .get("minProperties")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if required.len() > max || min > max as u64 {
            return None;
        }
        let mut object = Map::new();
        for key in &required {
            let value = self.property(schema, properties, path, key, depth)?;
            object.insert((*key).to_owned(), value);
        }
        if let Some(properties) = properties {
            for key in properties.keys() {
                if object.len() >= max {
                    break;
                }
                if !object.contains_key(key)
                    && let Some(value) = self.property(schema, Some(properties), path, key, depth)
                {
                    object.insert(key.clone(), value);
                }
            }
        }
        let mut index = 0;
        while object.len() < min as usize {
            let key = format!("mock_{index}");
            index += 1;
            if object.contains_key(&key) {
                continue;
            }
            let value = self.property(schema, properties, path, &key, depth)?;
            object.insert(key, value);
        }
        Some(Value::Object(object))
    }

    fn property(
        &mut self,
        schema: &Value,
        properties: Option<&Map<String, Value>>,
        path: &str,
        key: &str,
        depth: usize,
    ) -> Option<Value> {
        if properties.is_some_and(|properties| properties.contains_key(key)) {
            self.generate(&child(&child(path, "properties"), key), depth + 1)
        } else if schema.get("additionalProperties").is_some() {
            self.generate(&child(path, "additionalProperties"), depth + 1)
        } else {
            Some(Value::Null)
        }
    }

    fn array(&mut self, schema: &Value, path: &str, depth: usize) -> Option<Value> {
        let prefix = schema
            .get("prefixItems")
            .and_then(Value::as_array)
            .or_else(|| schema.get("items").and_then(Value::as_array));
        let min = schema.get("minItems").and_then(Value::as_u64).unwrap_or(0);
        let max = schema
            .get("maxItems")
            .and_then(Value::as_u64)
            .unwrap_or(MAX_ITEMS as u64)
            .min(MAX_ITEMS as u64);
        if min > max {
            return None;
        }
        let count = min
            .max(prefix.map_or(1, |values| values.len() as u64))
            .min(max) as usize;
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let item_path = if prefix.is_some_and(|prefix| index < prefix.len()) {
                Some(child(
                    &child(
                        path,
                        if schema.get("prefixItems").is_some() {
                            "prefixItems"
                        } else {
                            "items"
                        },
                    ),
                    &index.to_string(),
                ))
            } else if schema.get("items").is_some_and(|items| !items.is_array()) {
                Some(child(path, "items"))
            } else if schema.get("additionalItems").is_some() {
                Some(child(path, "additionalItems"))
            } else {
                None
            };
            values.push(match item_path {
                Some(path) => self.generate(&path, depth + 1)?,
                None => Value::Null,
            });
        }
        Some(Value::Array(values))
    }
}

fn numbers(schema: &Value, integer: bool) -> Vec<Value> {
    let lower = schema
        .get("exclusiveMinimum")
        .and_then(Value::as_f64)
        .or_else(|| schema.get("minimum").and_then(Value::as_f64));
    let upper = schema
        .get("exclusiveMaximum")
        .and_then(Value::as_f64)
        .or_else(|| schema.get("maximum").and_then(Value::as_f64));
    let step = schema
        .get("multipleOf")
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let mut values = vec![0.0, step, -step];
    if let Some(lower) = lower {
        values.extend([lower, lower + step]);
    }
    if let Some(upper) = upper {
        values.extend([upper, upper - step]);
    }
    if let (Some(lower), Some(upper)) = (lower, upper) {
        values.push(lower / 2.0 + upper / 2.0);
    }
    let multiple = schema.get("multipleOf").is_some();
    values
        .into_iter()
        .flat_map(|value| {
            if multiple || integer {
                vec![(value / step).floor() * step, (value / step).ceil() * step]
            } else {
                vec![value]
            }
        })
        .filter_map(|value| {
            if integer && value.fract() == 0.0 {
                if value >= i64::MIN as f64 && value < i64::MAX as f64 {
                    return Some(Value::from(value as i64));
                }
                if value >= 0.0 && value < u64::MAX as f64 {
                    return Some(Value::from(value as u64));
                }
            }
            serde_json::Number::from_f64(value).map(Value::Number)
        })
        .collect()
}

fn merge(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Object(mut left), Value::Object(right)) => {
            for (key, value) in right {
                let value = match left.remove(&key) {
                    Some(previous) => merge(previous, value),
                    None => value,
                };
                left.insert(key, value);
            }
            Value::Object(left)
        }
        (left, Value::Null) => left,
        (_, right) => right,
    }
}

fn generated_size(value: &Value) -> bool {
    if check_size(value, "schema").is_err() {
        return false;
    }
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            Value::String(text) if text.chars().count() > MAX_TEXT => return false,
            Value::Array(array) => {
                if array.len() > MAX_ITEMS {
                    return false;
                }
                stack.extend(array);
            }
            Value::Object(object) => {
                if object.len() > MAX_ITEMS {
                    return false;
                }
                stack.extend(object.values());
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generated(schema: Value) -> Value {
        let value = sample(&schema, None).unwrap();
        assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&value));
        value
    }

    #[test]
    fn nested_objects_references_enums_and_array_bounds() {
        let value = generated(json!({
            "type": "object", "additionalProperties": false,
            "$defs": {"entry": {"type": "object", "required": ["state", "count"],
                "properties": {"state": {"enum": ["ready", "done"]}, "count": {"type": "integer", "minimum": 3}}}},
            "required": ["entries", "empty"],
            "properties": {
                "entries": {"type": "array", "minItems": 2, "maxItems": 2, "items": {"$ref": "#/$defs/entry"}},
                "empty": {"type": "array", "maxItems": 0}
            }
        }));
        assert_eq!(value["entries"].as_array().unwrap().len(), 2);
        assert_eq!(value["entries"][0]["state"], "ready");
        assert_eq!(value["entries"][0]["count"].as_i64(), Some(3));
        assert_eq!(value["empty"], json!([]));
    }

    #[test]
    fn unions_intersections_and_nullable_recursion() {
        for keyword in ["anyOf", "oneOf"] {
            let schema = json!({keyword: [
                {"type": "string", "pattern": "^z{10}$"},
                {"type": "integer", "minimum": 7, "maximum": 7}
            ]});
            assert_eq!(generated(schema).as_f64(), Some(7.0));
        }
        assert_eq!(
            generated(json!({"allOf": [
                {"type": "object", "properties": {"id": {"const": 7}}, "required": ["id"]},
                {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}
            ]})),
            json!({"id": 7, "name": "mock"})
        );
        generated(json!({"$defs": {"node": {"anyOf": [
            {"type": "null"}, {"type": "object", "required": ["next"], "properties": {"next": {"$ref": "#/$defs/node"}}}
        ]}}, "$ref": "#/$defs/node"}));
    }

    #[test]
    fn fixtures_are_validated_against_all_constraints() {
        let schema = json!({"type": "object", "required": ["code"], "additionalProperties": false,
            "properties": {"code": {"type": "string", "pattern": "^SKU-[0-9]{8}$"}}});
        let valid = json!({"code": "SKU-12345678"});
        assert_eq!(sample(&schema, Some(&valid)).unwrap(), valid);
        for invalid in [
            json!({}),
            json!({"code": "wrong"}),
            json!({"code": "SKU-12345678", "extra": true}),
        ] {
            let error = sample(&schema, Some(&invalid)).unwrap_err();
            assert_eq!(error.status, 400);
            assert_eq!(error.param.as_deref(), Some("mock_fixture"));
        }
        let error = sample(&schema, None).unwrap_err();
        assert_eq!(error.status, 501);
        assert!(error.message.contains("mock_fixture.output"));
    }

    #[test]
    fn malformed_schemas_and_external_references_are_rejected() {
        for schema in [
            json!({"type": "unknown"}),
            json!({"type": 123}),
            json!({"required": "bad"}),
            json!({"$ref": "file:///etc/passwd"}),
            json!({"$ref": "https://example.com/schema"}),
            json!({"$ref": "#/$defs/missing"}),
            json!({"properties": {"value": {"$dynamicRef": "https://example.com/schema"}}}),
        ] {
            assert!(validate(&schema).is_err(), "{schema}");
            let error = sample(&schema, Some(&Value::Null)).unwrap_err();
            assert_eq!(error.status, 400, "{schema}");
            assert_eq!(error.param.as_deref(), Some("schema"));
        }
        assert!(
            validate(&json!({"$defs": {"entry": {"type": "string"}}, "$ref": "#/$defs/entry"}))
                .is_ok()
        );
        assert_eq!(
            generated(json!({"const": {"$ref": "https://example.com/data"}})),
            json!({"$ref": "https://example.com/data"})
        );
    }

    #[test]
    fn generation_is_bounded_and_never_returns_an_invalid_example() {
        for schema in [
            json!(false),
            json!({"type": "array", "minItems": 1000000000}),
            json!({"type": "string", "minLength": 1000000000}),
            json!({"type": "object", "required": ["next"], "properties": {"next": {"$ref": "#"}}}),
            json!({"oneOf": [{"type": "integer"}, {"type": "number"}]}),
        ] {
            assert_eq!(sample(&schema, None).unwrap_err().status, 501);
        }
        let mut schema = json!({"type": "string"});
        for _ in 0..40 {
            schema = json!({"items": schema});
        }
        let error = sample(&schema, None).unwrap_err();
        assert_eq!(error.param.as_deref(), Some("schema"));
    }

    #[test]
    fn escaped_local_references_and_validation_work_limits() {
        assert_eq!(
            generated(json!({
                "$defs": {"value with space": {"type": "string", "minLength": 6}},
                "$ref": "#/$defs/value%20with%20space"
            })),
            json!("mockxx")
        );
        let mut definitions = Map::new();
        definitions.insert("d0".into(), json!({"type": "null"}));
        for index in 1..30 {
            let reference = format!("#/$defs/d{}", index - 1);
            definitions.insert(
                format!("d{index}"),
                json!({"allOf": [
                    {"$ref": reference}, {"$ref": reference}
                ]}),
            );
        }
        let schema = json!({"$defs": definitions, "$ref": "#/$defs/d29"});
        let error = sample(&schema, Some(&Value::Null)).unwrap_err();
        assert_eq!(error.status, 400);
        assert_eq!(error.param.as_deref(), Some("schema"));
        assert!(error.message.contains("work limit"));
    }
}
