//! Gemma sometimes writes a tool call as plain text, which Ollama then returns as `content`:
//! `mcp__x__y{topic:<|"|>a card<|"|>}<tool_call|>`, with or without `<|tool_call>call:` in front.
//! [`recover_stream`] turns such a reply into real tool calls.
//!
//! A reply converts only when it begins with a whole call to a tool this request offered, so a
//! normal reply that mentions the syntax is unchanged. Prose after the calls stays as `content`
//! beside `tool_calls`: Hermes keeps that as commentary, and as the answer if the next model turn
//! comes back empty. Text is held only while it could still be a call.

use crate::engine::{Ev, EvStream};
use futures::StreamExt;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

const QUOTE: &str = "<|\"|>";
const OPEN: &str = "<|tool_call>";
const CALL: &str = "call:";
const CLOSE: &str = "<tool_call|>";
/// A longer held reply goes out as text.
const MAX_HELD_BYTES: usize = 64 * 1024;
const MAX_DEPTH: usize = 16;

/// The function names in a request's `tools`.
pub fn offered_names(body: &Value) -> HashSet<String> {
    body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
        .collect()
}

/// Could `text` (the reply so far) still become a call to one of `names`?
fn may_be_call(text: &str, names: &HashSet<String>) -> bool {
    let t = text.trim_start();
    names.iter().any(|name| {
        [
            format!("{OPEN}{CALL}{name}{{"),
            format!("{OPEN}{name}{{"),
            format!("{CALL}{name}{{"),
            format!("{name}{{"),
        ]
        .iter()
        .any(|form| t.starts_with(form.as_str()) || form.starts_with(t))
    })
}

/// The leading calls to `names` as native tool calls, plus the prose after them. None unless
/// `text` begins with a whole call.
pub fn parse_leading_calls(text: &str, names: &HashSet<String>) -> Option<(Vec<Value>, String)> {
    let mut p = Parser { rest: text };
    let mut calls = Vec::new();
    loop {
        p.skip_ws();
        let before = p.rest;
        match p.call(names) {
            Some(call) => calls.push(call),
            None if calls.is_empty() => return None,
            None => {
                // A broken or cut-off call after the good ones is not prose to show.
                let syntax = [QUOTE, OPEN, CLOSE].iter().any(|t| before.contains(t));
                let prose = if syntax { "" } else { before };
                return Some((calls, prose.trim().to_string()));
            }
        }
    }
}

struct Parser<'a> {
    rest: &'a str,
}

impl Parser<'_> {
    /// One call at the start of the input, with its optional markers.
    fn call(&mut self, names: &HashSet<String>) -> Option<Value> {
        self.eat(OPEN);
        self.eat(CALL);
        let brace = self.rest.find('{')?;
        let name = &self.rest[..brace];
        if !names.contains(name) {
            return None;
        }
        self.rest = &self.rest[brace + 1..];
        let arguments = self.object(0)?;
        self.skip_ws();
        self.eat(CLOSE);
        Some(json!({"function": {"name": name, "arguments": arguments}}))
    }

    fn skip_ws(&mut self) {
        self.rest = self.rest.trim_start();
    }

    fn eat(&mut self, token: &str) -> bool {
        match self.rest.strip_prefix(token) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    /// After a `<|"|>`: the text up to the closing one.
    fn quoted(&mut self) -> Option<String> {
        let end = self.rest.find(QUOTE)?;
        let text = self.rest[..end].to_string();
        self.rest = &self.rest[end + QUOTE.len()..];
        Some(text)
    }

    /// A JSON string at the start of the input.
    fn json_string(&mut self) -> Option<String> {
        let mut values = serde_json::Deserializer::from_str(self.rest).into_iter::<String>();
        let text = values.next()?.ok()?;
        let used = values.byte_offset();
        self.rest = &self.rest[used..];
        Some(text)
    }

    /// After a `{`: members up to and including the matching `}`.
    fn object(&mut self, depth: usize) -> Option<Value> {
        let mut out = Map::new();
        self.skip_ws();
        if self.eat("}") {
            return Some(Value::Object(out));
        }
        loop {
            self.skip_ws();
            let key = if self.eat(QUOTE) {
                self.quoted()?
            } else if self.rest.starts_with('"') {
                self.json_string()?
            } else {
                let end = self.rest.find(':')?;
                let key = self.rest[..end].trim();
                let plain = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
                if key.is_empty() || !key.chars().all(plain) {
                    return None;
                }
                self.rest = &self.rest[end..];
                key.to_string()
            };
            self.skip_ws();
            if !self.eat(":") {
                return None;
            }
            out.insert(key, self.value(depth)?);
            self.skip_ws();
            if self.eat("}") {
                return Some(Value::Object(out));
            }
            if !self.eat(",") {
                return None;
            }
        }
    }

    /// After a `[`: items up to and including the matching `]`.
    fn array(&mut self, depth: usize) -> Option<Value> {
        let mut out = Vec::new();
        self.skip_ws();
        if self.eat("]") {
            return Some(Value::Array(out));
        }
        loop {
            out.push(self.value(depth)?);
            self.skip_ws();
            if self.eat("]") {
                return Some(Value::Array(out));
            }
            if !self.eat(",") {
                return None;
            }
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth >= MAX_DEPTH {
            return None;
        }
        self.skip_ws();
        if self.eat(QUOTE) {
            return self.quoted().map(Value::String);
        }
        if self.rest.starts_with('"') {
            return self.json_string().map(Value::String);
        }
        if self.eat("{") {
            return self.object(depth + 1);
        }
        if self.eat("[") {
            return self.array(depth + 1);
        }
        // A number, true, false, null, or an unquoted word.
        let end = self.rest.find([',', '}', ']'])?;
        let word = self.rest[..end].trim();
        if word.is_empty() {
            return None;
        }
        self.rest = &self.rest[end..];
        Some(serde_json::from_str(word).unwrap_or_else(|_| json!(word)))
    }
}

fn text_delta(content: String) -> Ev {
    Ev::Delta {
        content,
        tool_calls: vec![],
    }
}

/// The reply as real tool calls if it was Gemma call syntax, as text otherwise.
fn settle(held: String, names: &HashSet<String>) -> Option<Ev> {
    if held.is_empty() {
        return None;
    }
    Some(match parse_leading_calls(&held, names) {
        Some((tool_calls, content)) => Ev::Delta {
            content,
            tool_calls,
        },
        None => text_delta(held),
    })
}

/// Passes `events` through, recovering a tool call written as text. Ordinary replies are released
/// at the first delta that cannot begin a call to one of `names`.
pub fn recover_stream(events: EvStream, names: HashSet<String>) -> EvStream {
    if names.is_empty() {
        return events;
    }
    async_stream::stream! {
        let mut held = String::new();
        let mut holding = true;
        futures::pin_mut!(events);
        while let Some(ev) = events.next().await {
            match ev {
                Ok(Ev::Delta { content, tool_calls }) if holding => {
                    held.push_str(&content);
                    if !tool_calls.is_empty() {
                        holding = false;
                        yield Ok(Ev::Delta { content: std::mem::take(&mut held), tool_calls });
                    } else if held.len() > MAX_HELD_BYTES || !may_be_call(&held, &names) {
                        holding = false;
                        yield Ok(text_delta(std::mem::take(&mut held)));
                    }
                }
                Ok(done @ Ev::Done { .. }) => {
                    if let Some(ev) = settle(std::mem::take(&mut held), &names) {
                        yield Ok(ev);
                    }
                    yield Ok(done);
                }
                Err(e) => {
                    held.clear();
                    yield Err(e);
                }
                other => yield other,
            }
        }
        // A stream that ends without Done still must not show call syntax.
        if let Some(ev) = settle(held, &names) {
            yield Ok(ev);
        }
    }
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MUTE: &str = "mcp__followups__mute_topic";
    const DONE: &str = "mcp__followups__item_done";

    fn names() -> HashSet<String> {
        [MUTE, DONE, "lights"].map(String::from).into()
    }

    /// All of `text` must be calls.
    fn parse_calls(text: &str, names: &HashSet<String>) -> Option<Vec<Value>> {
        let (calls, prose) = parse_leading_calls(text, names)?;
        assert_eq!(prose, "", "{text}");
        Some(calls)
    }

    fn call(c: &Value) -> (&str, &Value) {
        (
            c["function"]["name"].as_str().unwrap(),
            &c["function"]["arguments"],
        )
    }

    #[test]
    fn exact_leaked_strings_become_calls() {
        let one = "mcp__followups__mute_topic{topic:<|\"|>gym membership card<|\"|>}<tool_call|>";
        let calls = parse_calls(one, &names()).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            call(&calls[0]),
            (MUTE, &json!({"topic": "gym membership card"}))
        );

        for gap in [" ", "\n"] {
            let two = format!(
                "mcp__followups__item_done{{item:<|\"|>x<|\"|>,note:<|\"|>y<|\"|>}}{gap}mcp__followups__mute_topic{{topic:<|\"|>x<|\"|>}}"
            );
            let calls = parse_calls(&two, &names()).unwrap();
            assert_eq!(calls.len(), 2);
            assert_eq!(call(&calls[0]), (DONE, &json!({"item": "x", "note": "y"})));
            assert_eq!(call(&calls[1]), (MUTE, &json!({"topic": "x"})));
        }
    }

    #[test]
    fn markers_and_value_types() {
        let text = "<|tool_call>call:lights{note:<|\"|>a, b } c: d<|\"|>,level:3,on:true,off:null,\
                    room:kitchen,tags:[<|\"|>x,y<|\"|>,2],deep:{a:{b:1.5}}}<tool_call|>\
                    <|tool_call>call:lights{}<tool_call|>";
        let calls = parse_calls(text, &names()).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(
            call(&calls[0]).1,
            &json!({"note": "a, b } c: d", "level": 3, "on": true, "off": null,
                    "room": "kitchen", "tags": ["x,y", 2], "deep": {"a": {"b": 1.5}}})
        );
        assert_eq!(call(&calls[1]).1, &json!({}));
        let json_args = parse_calls("lights{\"on\": \"a, \\\"b\\\"\"}", &names()).unwrap();
        assert_eq!(call(&json_args[0]).1, &json!({"on": "a, \"b\""}));
    }

    #[test]
    fn leading_calls_keep_the_prose_after_them() {
        let text = "mcp__followups__item_done{item:<|\"|>bike repair receipt<|\"|>,note:<|\"|>sorted<|\"|>} All good on that front.";
        let (calls, prose) = parse_leading_calls(text, &names()).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            call(&calls[0]),
            (
                DONE,
                &json!({"item": "bike repair receipt", "note": "sorted"})
            )
        );
        assert_eq!(prose, "All good on that front.");

        let two = "lights{a:1}<tool_call|> lights{a:2}\n\nDone. Mute {this} too?";
        let (calls, prose) = parse_leading_calls(two, &names()).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(prose, "Done. Mute {this} too?");

        let (calls, prose) =
            parse_leading_calls("lights{a:1}, then web_search{}", &names()).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(prose, ", then web_search{}");

        // A cut-off second call must not reach the user.
        let (calls, prose) =
            parse_leading_calls("lights{a:1} lights{a:<|\"|>cut off", &names()).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(prose, "");
    }

    #[test]
    fn everything_else_stays_text() {
        for text in [
            "",
            "Done.",
            // Not an offered tool.
            "web_search{query:<|\"|>x<|\"|>}",
            "mcp__other__mute_topic{topic:<|\"|>x<|\"|>}<tool_call|>",
            // A mention, not a call.
            "You could call mcp__followups__mute_topic{topic:<|\"|>x<|\"|>} for that.",
            // Broken syntax.
            "lights{note:<|\"|>unclosed}",
            "lights{note:<|\"|>a<|\"|>",
            "lights{no colon}",
            "lights{a b:1}",
            "lights",
        ] {
            assert_eq!(parse_leading_calls(text, &names()), None, "{text}");
        }
        let deep = format!("lights{{a:{}", "[".repeat(100_000));
        assert_eq!(parse_leading_calls(&deep, &names()), None);
    }

    #[test]
    fn prefix_check_is_short_for_normal_text() {
        assert!(may_be_call("", &names()));
        assert!(may_be_call("  mcp__foll", &names()));
        assert!(may_be_call("<|tool", &names()));
        assert!(may_be_call("call:li", &names()));
        assert!(may_be_call("lights{on:", &names()));
        assert!(!may_be_call("Done", &names()));
        assert!(!may_be_call("lights are on", &names()));
        assert!(!may_be_call("call me", &names()));
    }

    fn done() -> Ev {
        Ev::Done {
            done_reason: "stop".into(),
            prompt_tokens: 1,
            completion_tokens: 2,
        }
    }

    async fn run(events: Vec<Result<Ev, String>>) -> Vec<Result<Ev, String>> {
        recover_stream(futures::stream::iter(events).boxed(), names())
            .collect()
            .await
    }

    fn chunks(parts: &[&str]) -> Vec<Result<Ev, String>> {
        let mut v: Vec<_> = parts
            .iter()
            .map(|p| Ok(text_delta(p.to_string())))
            .collect();
        v.push(Ok(done()));
        v
    }

    #[tokio::test]
    async fn split_chunks_become_one_tool_call_delta() {
        let out = run(chunks(&[
            "mcp__follow",
            "ups__mute_topic{topic:<|",
            "\"|>gym, card<|\"|>}",
            "<tool_call|>",
        ]))
        .await;
        assert_eq!(out.len(), 2);
        let Ok(Ev::Delta {
            content,
            tool_calls,
        }) = &out[0]
        else {
            panic!("{out:?}")
        };
        assert_eq!(content, "");
        assert_eq!(call(&tool_calls[0]), (MUTE, &json!({"topic": "gym, card"})));
        assert_eq!(out[1], Ok(done()));
    }

    #[tokio::test]
    async fn ordinary_text_is_not_held() {
        let out = run(chunks(&["Done", ".", " Talk soon."])).await;
        let expected = ["Done", ".", " Talk soon."].map(|t| Ok(text_delta(t.to_string())));
        assert_eq!(&out[..3], &expected);
        assert_eq!(out[3], Ok(done()));

        // Held for a moment, then released whole.
        let out = run(chunks(&["li", "ghts are on", " now"])).await;
        assert_eq!(out[0], Ok(text_delta("lights are on".into())));
        assert_eq!(out[1], Ok(text_delta(" now".into())));
    }

    #[tokio::test]
    async fn trailing_prose_is_held_with_the_call_and_kept() {
        let out = run(chunks(&["lights{on:true}", " All good", " on that front."])).await;
        let Ok(Ev::Delta {
            content,
            tool_calls,
        }) = &out[0]
        else {
            panic!("{out:?}")
        };
        assert_eq!(content, "All good on that front.");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(out[1], Ok(done()));
    }

    #[tokio::test]
    async fn a_broken_call_passes_unchanged_at_the_end() {
        let text = "lights{on is the call I would use";
        let out = run(chunks(&["lights{on", " is the call I would use"])).await;
        assert_eq!(out, vec![Ok(text_delta(text.to_string())), Ok(done())]);
    }

    #[tokio::test]
    async fn native_tool_calls_and_empty_tools_pass_through() {
        let native = vec![json!({"function": {"name": "lights", "arguments": {}}})];
        let delta = Ev::Delta {
            content: String::new(),
            tool_calls: native,
        };
        let out = run(vec![Ok(delta.clone()), Ok(done())]).await;
        assert_eq!(out, vec![Ok(delta), Ok(done())]);

        let text = "mcp__followups__mute_topic{topic:<|\"|>x<|\"|>}";
        let events = futures::stream::iter(chunks(&[text])).boxed();
        let out: Vec<_> = recover_stream(events, HashSet::new()).collect().await;
        assert_eq!(out[0], Ok(text_delta(text.to_string())));
    }

    #[tokio::test]
    async fn errors_and_truncated_streams_never_show_syntax() {
        let held = "mcp__followups__mute_topic{topic:<|\"|>x";
        let out = run(vec![Ok(text_delta(held.into())), Err("boom".into())]).await;
        assert_eq!(out, vec![Err("boom".to_string())]);

        let whole = "mcp__followups__mute_topic{topic:<|\"|>x<|\"|>}";
        let out = run(vec![Ok(text_delta(whole.into()))]).await;
        assert!(matches!(&out[0], Ok(Ev::Delta { tool_calls, .. }) if tool_calls.len() == 1));
    }

    #[tokio::test]
    async fn an_oversized_reply_goes_out_as_text() {
        let long = format!("lights{{a:<|\"|>{}", "x".repeat(MAX_HELD_BYTES));
        let out = run(chunks(&[&long, "tail"])).await;
        assert_eq!(out[0], Ok(text_delta(long)));
        assert_eq!(out[1], Ok(text_delta("tail".into())));
    }

    #[test]
    fn offered_names_come_from_the_request() {
        let body = json!({"tools": [{"type": "function", "function": {"name": "lights"}}, {}]});
        assert_eq!(offered_names(&body), HashSet::from(["lights".to_string()]));
        assert!(offered_names(&json!({})).is_empty());
    }
}
