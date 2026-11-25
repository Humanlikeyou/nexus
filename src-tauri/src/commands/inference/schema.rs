//! A small JSON Schema checker for model output and tool arguments: no new dependency.
//!
//! Supported keywords: `type` (string or list), `enum`, `const`, `properties`, `required`,
//! `additionalProperties` (bool or schema), `items`, `minItems`, `maxItems`, `minLength`,
//! `maxLength`, `minimum`, `maximum`, `anyOf`. A schema that uses anything else is refused by
//! [`check_supported`] up front, so a keyword is never silently ignored.
//!
//! Error text names the JSON path and the rule only, never the value (it may be personal data).

use serde_json::Value;

const SUPPORTED: [&str; 19] = [
    "type",
    "enum",
    "const",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "minItems",
    "maxItems",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "anyOf",
    // Annotations, no effect on validity.
    "title",
    "description",
    "default",
    "examples",
    "$schema",
];
const MAX_DEPTH: usize = 32;

/// Refuses a schema with a keyword this checker doesn't enforce.
pub(crate) fn check_supported(schema: &Value) -> Result<(), String> {
    walk_supported(schema, "$", 0)
}

fn walk_supported(schema: &Value, path: &str, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("{path}: schema too deep"));
    }
    let Some(obj) = schema.as_object() else {
        return match schema {
            Value::Bool(_) => Ok(()),
            _ => Err(format!("{path}: schema must be an object")),
        };
    };
    for key in obj.keys() {
        if !SUPPORTED.contains(&key.as_str()) {
            return Err(format!("{path}: unsupported schema keyword `{key}`"));
        }
    }
    if let Some(props) = obj.get("properties").and_then(Value::as_object) {
        for (k, s) in props {
            walk_supported(s, &format!("{path}.{k}"), depth + 1)?;
        }
    }
    if let Some(s) = obj.get("additionalProperties").filter(|v| v.is_object()) {
        walk_supported(s, &format!("{path}.*"), depth + 1)?;
    }
    if let Some(s) = obj.get("items") {
        walk_supported(s, &format!("{path}[]"), depth + 1)?;
    }
    if let Some(list) = obj.get("anyOf").and_then(Value::as_array) {
        for s in list {
            walk_supported(s, path, depth + 1)?;
        }
    }
    Ok(())
}

/// Ok when `value` satisfies `schema`; else the first failure as `path: rule`.
pub(crate) fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    check(schema, value, "$", 0)
}

fn type_matches(t: &str, v: &Value) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        "number" => v.is_number(),
        "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
        _ => false,
    }
}

fn check(schema: &Value, v: &Value, path: &str, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("{path}: too deep"));
    }
    let obj = match schema {
        Value::Bool(true) => return Ok(()),
        Value::Bool(false) => return Err(format!("{path}: not allowed")),
        Value::Object(o) => o,
        _ => return Err(format!("{path}: bad schema")),
    };
    if let Some(t) = obj.get("type") {
        let ok = match t {
            Value::String(s) => type_matches(s, v),
            Value::Array(list) => list
                .iter()
                .filter_map(Value::as_str)
                .any(|s| type_matches(s, v)),
            _ => false,
        };
        if !ok {
            return Err(format!("{path}: wrong type"));
        }
    }
    if let Some(list) = obj.get("enum").and_then(Value::as_array) {
        if !list.contains(v) {
            return Err(format!("{path}: not one of the allowed values"));
        }
    }
    if let Some(c) = obj.get("const") {
        if c != v {
            return Err(format!("{path}: not the required value"));
        }
    }
    if let Some(list) = obj.get("anyOf").and_then(Value::as_array) {
        if !list.iter().any(|s| check(s, v, path, depth + 1).is_ok()) {
            return Err(format!("{path}: matches none of anyOf"));
        }
    }
    if let Some(s) = v.as_str() {
        let n = s.chars().count() as u64;
        if obj
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|m| n < m)
        {
            return Err(format!("{path}: too short"));
        }
        if obj
            .get("maxLength")
            .and_then(Value::as_u64)
            .is_some_and(|m| n > m)
        {
            return Err(format!("{path}: too long"));
        }
    }
    if let Some(x) = v.as_f64() {
        if obj
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|m| x < m)
        {
            return Err(format!("{path}: below minimum"));
        }
        if obj
            .get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|m| x > m)
        {
            return Err(format!("{path}: above maximum"));
        }
    }
    if let Some(items) = v.as_array() {
        let n = items.len() as u64;
        if obj
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| n < m)
        {
            return Err(format!("{path}: too few items"));
        }
        if obj
            .get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| n > m)
        {
            return Err(format!("{path}: too many items"));
        }
        if let Some(s) = obj.get("items") {
            for (i, item) in items.iter().enumerate() {
                check(s, item, &format!("{path}[{i}]"), depth + 1)?;
            }
        }
    }
    if let Some(map) = v.as_object() {
        if let Some(req) = obj.get("required").and_then(Value::as_array) {
            for k in req.iter().filter_map(Value::as_str) {
                if !map.contains_key(k) {
                    return Err(format!("{path}.{k}: required"));
                }
            }
        }
        let props = obj.get("properties").and_then(Value::as_object);
        for (k, val) in map {
            let sub = format!("{path}.{k}");
            match props.and_then(|p| p.get(k)) {
                Some(s) => check(s, val, &sub, depth + 1)?,
                None => match obj.get("additionalProperties") {
                    Some(Value::Bool(false)) => return Err(format!("{sub}: not allowed")),
                    Some(s @ Value::Object(_)) => check(s, val, &sub, depth + 1)?,
                    _ => {}
                },
            }
        }
    }
    Ok(())
}

/// The JSON value in a model reply: the whole trimmed text, or the body of one ```json fence.
pub(crate) fn extract_json(content: &str) -> Option<Value> {
    let t = content.trim();
    if let Ok(v) = serde_json::from_str(t) {
        return Some(v);
    }
    let start = t.find("```")?;
    let after = &t[start + 3..];
    let after = after.strip_prefix("json").unwrap_or(after);
    let end = after.find("```")?;
    serde_json::from_str(after[..end].trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn day() -> Value {
        json!({"type": "object",
               "properties": {"mood": {"type": "string", "enum": ["good", "ok", "bad"]},
                              "score": {"type": "integer", "minimum": 0, "maximum": 10},
                              "tags": {"type": "array", "items": {"type": "string", "maxLength": 5}, "maxItems": 2}},
               "required": ["mood", "score"], "additionalProperties": false})
    }

    #[test]
    fn valid_and_each_kind_of_invalid() {
        assert!(validate(&day(), &json!({"mood": "ok", "score": 5})).is_ok());
        let bad = [
            (json!({"mood": "neutral", "score": 5}), "$.mood: not one of"),
            (json!({"mood": "ok"}), "$.score: required"),
            (json!({"mood": "ok", "score": 11}), "$.score: above maximum"),
            (json!({"mood": "ok", "score": 2.5}), "$.score: wrong type"),
            (
                json!({"mood": "ok", "score": 1, "x": 1}),
                "$.x: not allowed",
            ),
            (
                json!({"mood": "ok", "score": 1, "tags": ["a", "b", "c"]}),
                "too many items",
            ),
            (
                json!({"mood": "ok", "score": 1, "tags": ["toolong"]}),
                "$.tags[0]: too long",
            ),
            (json!("text"), "$: wrong type"),
        ];
        for (v, want) in bad {
            let e = validate(&day(), &v).unwrap_err();
            assert!(e.contains(want), "{e} should contain {want}");
        }
    }

    #[test]
    fn errors_never_echo_the_value() {
        let e = validate(&day(), &json!({"mood": "my secret diagnosis", "score": 1})).unwrap_err();
        assert!(!e.contains("secret"));
    }

    #[test]
    fn unsupported_keywords_are_refused_not_ignored() {
        assert!(check_supported(&day()).is_ok());
        let e = check_supported(&json!({"type": "string", "pattern": "^a"})).unwrap_err();
        assert!(e.contains("pattern"));
        let e = check_supported(&json!({"type": "object", "properties": {"a": {"oneOf": []}}}))
            .unwrap_err();
        assert!(e.contains("$.a") && e.contains("oneOf"));
    }

    #[test]
    fn json_is_found_bare_or_in_one_fence() {
        assert_eq!(extract_json(" {\"a\":1} "), Some(json!({"a": 1})));
        assert_eq!(
            extract_json("Here:\n```json\n{\"a\": 2}\n```\n"),
            Some(json!({"a": 2}))
        );
        assert_eq!(extract_json("no json here"), None);
        assert_eq!(extract_json("```json\n{broken\n```"), None);
    }

    #[test]
    fn integer_bounds_and_any_of() {
        let s = json!({"anyOf": [{"type": "integer"}, {"type": "null"}]});
        assert!(validate(&s, &json!(3)).is_ok());
        assert!(validate(&s, &Value::Null).is_ok());
        assert!(validate(&s, &json!("3")).is_err());
    }
}
