//! OpenAI chat-completions ⇄ Ollama native `/api/chat` translation.
//!
//! Internally every request becomes an Ollama native chat body; replies come back as
//! [`crate::engine::Ev`] events and are re-encoded per API.

use serde_json::{json, Map, Value};

/// Text of a message `content` that may be a string or an OpenAI parts array.
pub fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p {
                Value::String(s) => Some(s.as_str()),
                _ if p["type"] == "text" => p["text"].as_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Base64 payloads of `data:` image parts (Ollama native takes raw base64 in `images`).
fn content_images(content: &Value) -> Vec<Value> {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["type"] == "image_url")
        .filter_map(|p| {
            let url = p["image_url"]["url"]
                .as_str()
                .or_else(|| p["image_url"].as_str())?;
            let (_, b64) = url.strip_prefix("data:")?.split_once(";base64,")?;
            Some(json!(b64))
        })
        .collect()
}

/// OpenAI `tool_calls` (arguments as a JSON string) → native (arguments as an object).
fn tool_calls_to_native(calls: &Value) -> Value {
    Value::Array(
        calls
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                let f = &c["function"];
                let args = match &f["arguments"] {
                    Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!({})),
                    Value::Null => json!({}),
                    other => other.clone(),
                };
                json!({"function": {"name": f["name"], "arguments": args}})
            })
            .collect(),
    )
}

/// Translates an OpenAI `/v1/chat/completions` body into an Ollama native chat body.
/// Returns (native body, stream?, include_usage?).
pub fn openai_to_native(req: &Value) -> Result<(Value, bool, bool), String> {
    let msgs = req["messages"]
        .as_array()
        .ok_or("`messages` must be an array")?;
    // tool_call_id → function name, so tool results can carry `tool_name`.
    let mut call_names = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(msgs.len());
    for m in msgs {
        let role = m["role"].as_str().unwrap_or("user");
        let role = if role == "developer" { "system" } else { role };
        let mut nm = Map::new();
        nm.insert("role".into(), json!(role));
        nm.insert("content".into(), json!(content_text(&m["content"])));
        let images = content_images(&m["content"]);
        if !images.is_empty() {
            nm.insert("images".into(), Value::Array(images));
        }
        if let Some(calls) = m["tool_calls"].as_array() {
            for c in calls {
                if let (Some(id), Some(name)) = (c["id"].as_str(), c["function"]["name"].as_str()) {
                    call_names.insert(id.to_string(), name.to_string());
                }
            }
            nm.insert("tool_calls".into(), tool_calls_to_native(&m["tool_calls"]));
        }
        if role == "tool" {
            let name = m["name"].as_str().map(str::to_string).or_else(|| {
                m["tool_call_id"]
                    .as_str()
                    .and_then(|id| call_names.get(id).cloned())
            });
            if let Some(name) = name {
                nm.insert("tool_name".into(), json!(name));
            }
        }
        out.push(Value::Object(nm));
    }

    let mut options = req["options"].as_object().cloned().unwrap_or_default();
    for (from, to) in [
        ("temperature", "temperature"),
        ("top_p", "top_p"),
        ("seed", "seed"),
        ("stop", "stop"),
        ("frequency_penalty", "frequency_penalty"),
        ("presence_penalty", "presence_penalty"),
        ("max_tokens", "num_predict"),
        ("max_completion_tokens", "num_predict"),
    ] {
        if !req[from].is_null() {
            let v = if from == "stop" && req[from].is_string() {
                json!([req[from]])
            } else {
                req[from].clone()
            };
            options.insert(to.into(), v);
        }
    }

    let mut body = Map::new();
    body.insert("messages".into(), Value::Array(out));
    if !options.is_empty() {
        body.insert("options".into(), Value::Object(options));
    }
    if let Some(tools) = req["tools"].as_array().filter(|t| !t.is_empty()) {
        body.insert("tools".into(), Value::Array(tools.clone()));
    }
    match req["response_format"]["type"].as_str() {
        Some("json_object") => {
            body.insert("format".into(), json!("json"));
        }
        Some("json_schema") => {
            body.insert(
                "format".into(),
                req["response_format"]["json_schema"]["schema"].clone(),
            );
        }
        _ => {}
    }
    if !req["think"].is_null() {
        body.insert("think".into(), req["think"].clone());
    }
    let stream = req["stream"].as_bool().unwrap_or(false);
    let include_usage = req["stream_options"]["include_usage"]
        .as_bool()
        .unwrap_or(false);
    Ok((Value::Object(body), stream, include_usage))
}

/// Native tool calls → OpenAI tool calls (arguments as JSON strings, with ids).
pub fn tool_calls_to_openai(calls: &[Value], id_base: &str, streaming: bool) -> Value {
    Value::Array(
        calls
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let f = &c["function"];
                let args = match &f["arguments"] {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                let mut v = json!({
                    "id": format!("call_{id_base}_{i}"),
                    "type": "function",
                    "function": {"name": f["name"], "arguments": args}
                });
                if streaming {
                    v["index"] = json!(i);
                }
                v
            })
            .collect(),
    )
}

/// Ollama `done_reason` → OpenAI `finish_reason`.
pub fn finish_reason(done_reason: &str, had_tool_calls: bool) -> &'static str {
    if had_tool_calls {
        "tool_calls"
    } else if done_reason == "length" {
        "length"
    } else {
        "stop"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_request_becomes_native() {
        let req = json!({
            "model": "amara",
            "stream": true,
            "stream_options": {"include_usage": true},
            "temperature": 0.2,
            "max_tokens": 50,
            "stop": "END",
            "messages": [
                {"role": "system", "content": "persona"},
                {"role": "user", "content": [{"type": "text", "text": "look"}, {"type": "image_url", "image_url": {"url": "data:image/png;base64,QUJD"}}]},
                {"role": "assistant", "content": null, "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "lights", "arguments": "{\"on\":true}"}}]},
                {"role": "tool", "tool_call_id": "c1", "content": "done"}
            ],
            "tools": [{"type": "function", "function": {"name": "lights", "parameters": {}}}]
        });
        let (b, stream, usage) = openai_to_native(&req).unwrap();
        assert!(stream && usage);
        let m = &b["messages"];
        assert_eq!(m[0], json!({"role": "system", "content": "persona"}));
        assert_eq!(m[1]["content"], "look");
        assert_eq!(m[1]["images"], json!(["QUJD"]));
        assert_eq!(m[2]["content"], "");
        assert_eq!(
            m[2]["tool_calls"][0]["function"]["arguments"],
            json!({"on": true})
        );
        assert_eq!(m[3]["tool_name"], "lights");
        assert_eq!(b["options"]["temperature"], json!(0.2));
        assert_eq!(b["options"]["num_predict"], json!(50));
        assert_eq!(b["options"]["stop"], json!(["END"]));
        assert_eq!(b["tools"][0]["function"]["name"], "lights");
        assert!(b.get("model").is_none());
    }

    #[test]
    fn tool_calls_back_to_openai() {
        let native = vec![json!({"function": {"name": "f", "arguments": {"a": 1}}})];
        let v = tool_calls_to_openai(&native, "x", true);
        assert_eq!(v[0]["function"]["arguments"], "{\"a\":1}");
        assert_eq!(v[0]["index"], 0);
        assert_eq!(v[0]["type"], "function");
        assert_eq!(finish_reason("stop", true), "tool_calls");
        assert_eq!(finish_reason("length", false), "length");
    }

    #[test]
    fn rejects_missing_messages() {
        assert!(openai_to_native(&json!({"model": "x"})).is_err());
    }
}
