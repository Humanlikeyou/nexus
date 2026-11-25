//! Redaction before anything reaches the ledger: secrets by key name and by shape, declared
//! personal fields, and long text. Applied to parameters, expected state, responses and errors.

use serde_json::{Map, Value};

const SECRET_KEY_PARTS: [&str; 12] = [
    "key",
    "token",
    "secret",
    "password",
    "passwd",
    "credential",
    "authorization",
    "auth",
    "cookie",
    "session",
    "bearer",
    "private",
];
/// Parameter names that only *contain* a secret word but aren't secrets.
const SAFE_KEYS: [&str; 4] = ["idempotency_key", "keyboard", "monkey", "keynote"];
const MAX_TEXT_CHARS: usize = 500;
pub(crate) const REDACTED: &str = "[redacted]";
pub(crate) const PERSONAL: &str = "[personal]";

fn secret_key(k: &str) -> bool {
    let low = k.to_ascii_lowercase();
    !SAFE_KEYS.contains(&low.as_str()) && SECRET_KEY_PARTS.iter().any(|p| low.contains(p))
}

/// A value that looks like a credential, whatever its key.
fn secret_shape(s: &str) -> bool {
    let t = s.trim();
    let low = t.to_ascii_lowercase();
    low.starts_with("bearer ")
        || low.starts_with("basic ")
        || ["sk-", "sk_", "ghp_", "gho_", "xox", "akia", "eyj"]
            .iter()
            .any(|p| low.starts_with(p) && t.len() >= 20)
        || (t.len() >= 32
            && !t.contains(char::is_whitespace)
            && t.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_=+/.".contains(c))
            && t.chars().any(|c| c.is_ascii_digit())
            && t.chars().any(|c| c.is_ascii_alphabetic())
            && !t.contains('/'))
}

/// Text for the ledger: secret-shaped words removed, then capped.
pub(crate) fn text(s: &str) -> String {
    let scrubbed: Vec<String> = s
        .split(' ')
        .map(|w| {
            if secret_shape(w) {
                REDACTED.to_string()
            } else {
                w.to_string()
            }
        })
        .collect();
    let joined = scrubbed.join(" ");
    let low = joined.to_ascii_lowercase();
    // "Bearer <token>" split across words.
    let joined = if let Some(i) = low.find("bearer ") {
        format!("{}{REDACTED}", &joined[..i])
    } else {
        joined
    };
    let n = joined.chars().count();
    if n <= MAX_TEXT_CHARS {
        joined
    } else {
        let cut: String = joined.chars().take(MAX_TEXT_CHARS).collect();
        format!("{cut}… ({n} chars)")
    }
}

/// A redacted copy of `v`. `personal` names fields (at any depth) whose values are personal data.
pub(crate) fn value(v: &Value, personal: &[&str]) -> Value {
    walk(v, personal, 0)
}

fn walk(v: &Value, personal: &[&str], depth: usize) -> Value {
    if depth > 16 {
        return Value::String(REDACTED.into());
    }
    match v {
        Value::Object(o) => {
            let mut out = Map::new();
            for (k, val) in o {
                let red = if secret_key(k) {
                    Value::String(REDACTED.into())
                } else if personal.contains(&k.as_str()) {
                    Value::String(PERSONAL.into())
                } else {
                    walk(val, personal, depth + 1)
                };
                out.insert(k.clone(), red);
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(
            a.iter()
                .take(200)
                .map(|x| walk(x, personal, depth + 1))
                .collect(),
        ),
        Value::String(s) => Value::String(if secret_shape(s) {
            REDACTED.into()
        } else {
            text(s)
        }),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn secrets_by_name_and_by_shape_and_personal_fields_are_removed() {
        let v = json!({
            "api_key": "abc", "Authorization": "x", "nested": {"refresh_token": "y", "note": "ok"},
            "header": "Bearer abcdefghijklmnop", "raw": "sk-ant-FAKE_ANTHROPIC_NOT_REAL_aaaaaaaa",
            "hexy": "4f9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a", "path": "/Users/t/Documents/a b.md",
            "diagnosis": "private health text", "volume": 40, "idempotency_key": "k1",
            "list": ["fine", "ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
        });
        let r = value(&v, &["diagnosis"]);
        let s = r.to_string();
        for leak in [
            "abc\"",
            "\"y\"",
            "abcdefghijklmnop",
            "FAKE_ANTHROPIC",
            "4f9a8b7c",
            "private health",
            "ghp_",
        ] {
            assert!(!s.contains(leak), "{leak} leaked: {s}");
        }
        assert_eq!(r["nested"]["note"], "ok");
        assert_eq!(r["path"], "/Users/t/Documents/a b.md");
        assert_eq!(r["volume"], 40);
        assert_eq!(r["idempotency_key"], "k1");
        assert_eq!(r["diagnosis"], PERSONAL);
    }

    #[test]
    fn error_text_is_scrubbed_and_capped() {
        let t = text("upstream said Bearer sk-live-FAKE_LIVE_NOT_REAL_aaaa failed");
        assert!(!t.contains("FAKE_LIVE") && t.starts_with("upstream said "));
        let long = "word ".repeat(400);
        assert!(text(&long).ends_with("(2000 chars)"));
    }
}
