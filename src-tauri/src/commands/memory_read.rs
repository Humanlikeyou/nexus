//! Settings > Memory (read side): `memory_list` returns Amara's Letta core-memory blocks.
//!
//! Rules this module keeps:
//! - GET only. There is no code path here that builds any other HTTP method (a test drives the
//!   whole command against a fake server and asserts every request line starts with `GET `).
//! - Loopback only: the base URL is the constant below. No host, port or path comes from input
//!   or the environment. Redirects and system proxies are off.
//! - The Letta password lives in the Keychain (`letta_server_password`, set through `secret_set`),
//!   is sent only as `Authorization: Bearer`, is zeroized after use, and is never logged or put in
//!   an error. Errors are fixed codes (`letta_*`); no response body or URL ever reaches them.
//! - Caps: agents, blocks, items, characters per value, bytes per response, time per request and
//!   per call.
//! - Like the other loopback reads (n8n, system map) this ignores NEXUS_OUTBOUND_DISABLED: that
//!   switch blocks sends that leave the Mac, and this read never does.
//!
//! API (Letta 0.16.x REST): `GET /v1/agents/?limit=N` then `GET /v1/agents/{id}/core-memory/blocks?limit=N`.
//! Checked against the 0.16.4 server source in ~/projects/terrance-integrations/MemGPT; the live 0.16.8
//! container needs the password for every route (an unauthenticated call returns 401), so the
//! exact 0.16.8 response was not observed from here.

use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use zeroize::Zeroizing;

use super::keychain::{service_for, Keychain, KeychainError, MacKeychain};

pub(crate) const LETTA_BASE: &str = "http://127.0.0.1:8283";
/// Keychain item name (service `com.taisun.nexus.letta_server_password`).
pub(crate) const LETTA_SECRET: &str = "letta_server_password";

const MAX_AGENTS: usize = 10;
const MAX_BLOCKS_PER_AGENT: usize = 20;
const MAX_ITEMS: usize = 50;
pub(crate) const MAX_VALUE_CHARS: usize = 2000;
pub(crate) const MAX_BODY_BYTES: usize = 512 * 1024;
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItem {
    pub id: String,
    pub label: String,
    /// Truncated to `MAX_VALUE_CHARS` characters.
    pub value: String,
    pub agent: String,
    pub updated_at: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryError {
    NoKey,
    KeyError,
    Off,
    Unauthorized,
    BadResponse,
}

impl MemoryError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::NoKey => "letta_no_key",
            Self::KeyError => "letta_key_error",
            Self::Off => "letta_off",
            Self::Unauthorized => "letta_unauthorized",
            Self::BadResponse => "letta_bad_response",
        }
    }
}

/// Longest password accepted from the Keychain.
const MAX_KEY_BYTES: usize = 256;

/// Reads the password from the Keychain. The error carries no Keychain text.
pub(crate) fn read_key(kc: &dyn Keychain) -> Result<Zeroizing<String>, MemoryError> {
    match kc.get(&service_for(LETTA_SECRET)) {
        Ok(bytes) => {
            let bytes = Zeroizing::new(bytes);
            let s = Zeroizing::new(
                std::str::from_utf8(&bytes)
                    .map_err(|_| MemoryError::KeyError)?
                    .trim()
                    .to_string(),
            );
            if s.is_empty() {
                Err(MemoryError::NoKey)
            } else if s.len() > MAX_KEY_BYTES || !s.bytes().all(|b| b.is_ascii_graphic()) {
                // A newline, space or non-ASCII byte can't go in a Bearer header; say "key error", not "Letta is off".
                Err(MemoryError::KeyError)
            } else {
                Ok(s)
            }
        }
        Err(KeychainError::NotFound) => Err(MemoryError::NoKey),
        Err(KeychainError::Other(_)) => Err(MemoryError::KeyError),
    }
}

fn clip(s: &str) -> (String, bool) {
    match s.char_indices().nth(MAX_VALUE_CHARS) {
        Some((i, _)) => (s[..i].to_string(), true),
        None => (s.to_string(), false),
    }
}

/// Letta ids look like `agent-<uuid>`. Anything else never goes into a URL path.
pub(crate) fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `GET /v1/agents/` body -> (id, name), capped at `MAX_AGENTS`.
pub(crate) fn parse_agents(v: &Value) -> Result<Vec<(String, String)>, MemoryError> {
    let arr = v.as_array().ok_or(MemoryError::BadResponse)?;
    Ok(arr
        .iter()
        .filter_map(|a| {
            let id = a.get("id")?.as_str()?;
            if !safe_id(id) {
                return None;
            }
            let name = a.get("name").and_then(Value::as_str).unwrap_or("Agent");
            Some((id.to_string(), clip(name).0))
        })
        .take(MAX_AGENTS)
        .collect())
}

/// `GET /v1/agents/{id}/core-memory/blocks` body -> items for that agent, capped at `MAX_BLOCKS_PER_AGENT`.
pub(crate) fn parse_blocks(v: &Value, agent: &str) -> Result<Vec<MemoryItem>, MemoryError> {
    let arr = v.as_array().ok_or(MemoryError::BadResponse)?;
    Ok(arr
        .iter()
        .filter_map(|b| {
            let id = b.get("id")?.as_str()?;
            let value = b.get("value")?.as_str()?;
            let label = b.get("label").and_then(Value::as_str).unwrap_or("block");
            let (value, truncated) = clip(value);
            Some(MemoryItem {
                id: id.to_string(),
                label: clip(label).0,
                value,
                agent: agent.to_string(),
                updated_at: b
                    .get("updated_at")
                    .and_then(Value::as_str)
                    .map(|s| clip(s).0),
                truncated,
            })
        })
        .take(MAX_BLOCKS_PER_AGENT)
        .collect())
}

/// Loopback client: no redirects, no system proxy. Shared with `memory_write`.
pub(crate) fn build_client() -> Result<reqwest::Client, MemoryError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .map_err(|_| MemoryError::Off)
}

/// The only HTTP call in this module: GET with the bearer, capped body, no redirects, no proxy.
async fn get_json(client: &reqwest::Client, url: &str, key: &str) -> Result<Value, MemoryError> {
    let resp = client
        .get(url)
        .bearer_auth(key)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|_| MemoryError::Off)?;
    read_json_body(resp).await
}

/// Status check + capped body read + JSON parse. Shared with `memory_write`.
pub(crate) async fn read_json_body(mut resp: reqwest::Response) -> Result<Value, MemoryError> {
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(MemoryError::Unauthorized);
    }
    if !status.is_success() {
        return Err(MemoryError::BadResponse);
    }
    if resp
        .content_length()
        .is_some_and(|n| n > MAX_BODY_BYTES as u64)
    {
        return Err(MemoryError::BadResponse);
    }
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|_| MemoryError::Off)? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(MemoryError::BadResponse);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| MemoryError::BadResponse)
}

pub(crate) async fn list_from(base: &str, key: &str) -> Result<Vec<MemoryItem>, MemoryError> {
    let client = build_client()?;
    let agents = parse_agents(
        &get_json(
            &client,
            &format!("{base}/v1/agents/?limit={MAX_AGENTS}"),
            key,
        )
        .await?,
    )?;
    let mut items = Vec::new();
    for (id, name) in agents {
        if items.len() >= MAX_ITEMS {
            break;
        }
        let url = format!("{base}/v1/agents/{id}/core-memory/blocks?limit={MAX_BLOCKS_PER_AGENT}");
        items.extend(parse_blocks(&get_json(&client, &url, key).await?, &name)?);
    }
    items.truncate(MAX_ITEMS);
    Ok(items)
}

/// Read-only. The error is one of the fixed `letta_*` codes.
#[tauri::command]
pub async fn memory_list() -> Result<Vec<MemoryItem>, String> {
    let key = tokio::task::spawn_blocking(|| read_key(&MacKeychain))
        .await
        .map_err(|_| MemoryError::KeyError.code().to_string())?
        .map_err(|e| e.code().to_string())?;
    tokio::time::timeout(CALL_TIMEOUT, list_from(LETTA_BASE, &key))
        .await
        .map_err(|_| MemoryError::Off.code().to_string())?
        .map_err(|e| e.code().to_string())
}

#[cfg(test)]
mod tests {
    use super::super::keychain::{set_with, FakeKeychain};
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const SENTINEL: &str = "SENTINEL-letta-pw-do-not-leak";

    type Handler = Arc<dyn Fn(&str, &str) -> (u16, String) + Send + Sync>;

    /// Fake Letta on an ephemeral loopback port. Records each request line and bearer header.
    /// `handler(request_line, authorization_header) -> (status, body)`.
    async fn fake(handler: Handler) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let (handler, log) = (handler.clone(), log.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let mut n = 0;
                    while n < buf.len() {
                        let Ok(r) = sock.read(&mut buf[n..]).await else {
                            return;
                        };
                        if r == 0 {
                            break;
                        }
                        n += r;
                        if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    let line = head.lines().next().unwrap_or("").to_string();
                    let auth = head
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))
                        .unwrap_or("")
                        .to_string();
                    log.lock().unwrap().push(line.clone());
                    let (status, body) = handler(&line, &auth);
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (base, seen)
    }

    fn happy() -> Handler {
        Arc::new(|line, auth| {
            if !auth.ends_with(&format!("Bearer {SENTINEL}")) {
                return (401, "{\"detail\":\"Unauthorized\"}".into());
            }
            if line.starts_with("GET /v1/agents/?") {
                (
                    200,
                    json!([{"id":"agent-1","name":"Amara"},{"id":"../evil","name":"x"}])
                        .to_string(),
                )
            } else if line.starts_with("GET /v1/agents/agent-1/core-memory/blocks") {
                (
                    200,
                    json!([
                        {"id":"block-1","label":"human","value":"Lives in Tokyo","updated_at":"2026-11-01T00:00:00Z"},
                        {"id":"block-2","label":"persona","value":"x".repeat(MAX_VALUE_CHARS + 50)},
                        {"id":"block-3","label":"broken"}
                    ])
                    .to_string(),
                )
            } else {
                (404, "{}".into())
            }
        })
    }

    #[test]
    fn parses_agents_and_blocks_and_caps_values() {
        let agents = parse_agents(&json!([
            {"id":"agent-1","name":"Amara"},
            {"id":"bad id/..","name":"x"},
            {"name":"no id"}
        ]))
        .unwrap();
        assert_eq!(agents, vec![("agent-1".to_string(), "Amara".to_string())]);
        let many: Vec<Value> = (0..40)
            .map(|i| json!({"id":format!("agent-{i}"),"name":"n"}))
            .collect();
        assert_eq!(parse_agents(&Value::Array(many)).unwrap().len(), MAX_AGENTS);
        assert_eq!(
            parse_agents(&json!({"detail":"x"})),
            Err(MemoryError::BadResponse)
        );

        let blocks = parse_blocks(
            &json!([
                {"id":"b1","label":"human","value":"héllo","updated_at":"2026-11-01T00:00:00Z"},
                {"id":"b2","label":"persona","value":"é".repeat(MAX_VALUE_CHARS + 1)},
                {"id":"b3","value":"no label"},
                {"id":"b4"}
            ]),
            "Amara",
        )
        .unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].value, "héllo");
        assert_eq!(
            blocks[0].updated_at.as_deref(),
            Some("2026-11-01T00:00:00Z")
        );
        assert!(!blocks[0].truncated);
        assert_eq!(blocks[1].value.chars().count(), MAX_VALUE_CHARS);
        assert!(blocks[1].truncated);
        assert_eq!(blocks[2].label, "block");
        assert_eq!(blocks[0].agent, "Amara");
        let many: Vec<Value> = (0..60)
            .map(|i| json!({"id":format!("b{i}"),"value":"v"}))
            .collect();
        assert_eq!(
            parse_blocks(&Value::Array(many), "A").unwrap().len(),
            MAX_BLOCKS_PER_AGENT
        );
        assert_eq!(
            parse_blocks(&json!("x"), "A"),
            Err(MemoryError::BadResponse)
        );
    }

    #[test]
    fn error_codes_are_fixed_strings() {
        assert_eq!(MemoryError::NoKey.code(), "letta_no_key");
        assert_eq!(MemoryError::KeyError.code(), "letta_key_error");
        assert_eq!(MemoryError::Off.code(), "letta_off");
        assert_eq!(MemoryError::Unauthorized.code(), "letta_unauthorized");
        assert_eq!(MemoryError::BadResponse.code(), "letta_bad_response");
    }

    #[test]
    fn key_comes_from_the_keychain_and_missing_means_no_key() {
        let kc = FakeKeychain::default();
        assert_eq!(read_key(&kc).unwrap_err(), MemoryError::NoKey);
        set_with(&kc, LETTA_SECRET, format!("  {SENTINEL}\n")).unwrap();
        assert_eq!(&**read_key(&kc).as_ref().unwrap(), SENTINEL);
    }

    #[test]
    fn a_malformed_key_is_a_key_error_not_letta_off() {
        let kc = FakeKeychain::default();
        for bad in [
            "pass word".to_string(),
            "pa\nss".to_string(),
            "pässword".to_string(),
            "x".repeat(257),
        ] {
            set_with(&kc, LETTA_SECRET, bad).unwrap();
            assert_eq!(read_key(&kc).unwrap_err(), MemoryError::KeyError);
        }
    }

    #[tokio::test]
    async fn lists_blocks_over_get_only_with_the_bearer() {
        let (base, seen) = fake(happy()).await;
        let items = list_from(&base, SENTINEL).await.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].agent, "Amara");
        assert_eq!(items[0].label, "human");
        assert!(items[1].truncated);
        let lines = seen.lock().unwrap().clone();
        // the unsafe agent id ("../evil") never became a request
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines.iter().all(|l| l.starts_with("GET ")),
            "non-GET request: {lines:?}"
        );
    }

    #[tokio::test]
    async fn wrong_password_is_unauthorized_and_leaks_nothing() {
        let (base, _) = fake(happy()).await;
        let e = list_from(&base, "wrong").await.unwrap_err();
        assert_eq!(e, MemoryError::Unauthorized);
        assert!(!format!("{e:?}{}", e.code()).contains("wrong"));
    }

    #[tokio::test]
    async fn closed_port_is_off() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        drop(l);
        assert_eq!(list_from(&base, "k").await.unwrap_err(), MemoryError::Off);
    }

    #[tokio::test]
    async fn bad_status_bad_json_and_oversize_are_bad_response() {
        let (b, _) = fake(Arc::new(|_, _| (500, "secret body".into()))).await;
        assert_eq!(
            list_from(&b, "k").await.unwrap_err(),
            MemoryError::BadResponse
        );
        let (b, _) = fake(Arc::new(|_, _| (200, "not json".into()))).await;
        assert_eq!(
            list_from(&b, "k").await.unwrap_err(),
            MemoryError::BadResponse
        );
        let (b, _) = fake(Arc::new(|_, _| (200, "{\"detail\":1}".into()))).await;
        assert_eq!(
            list_from(&b, "k").await.unwrap_err(),
            MemoryError::BadResponse
        );
        let big = format!("[\"{}\"]", "a".repeat(MAX_BODY_BYTES + 10));
        let (b, _) = fake(Arc::new(move |_, _| (200, big.clone()))).await;
        assert_eq!(
            list_from(&b, "k").await.unwrap_err(),
            MemoryError::BadResponse
        );
    }

    #[tokio::test]
    async fn total_items_are_capped() {
        let (base, _) = fake(Arc::new(|line, _| {
            if line.starts_with("GET /v1/agents/?") {
                let a: Vec<Value> = (0..MAX_AGENTS)
                    .map(|i| json!({"id":format!("agent-{i}"),"name":"n"}))
                    .collect();
                (200, Value::Array(a).to_string())
            } else {
                let b: Vec<Value> = (0..MAX_BLOCKS_PER_AGENT)
                    .map(|i| json!({"id":format!("b{i}"),"value":"v"}))
                    .collect();
                (200, Value::Array(b).to_string())
            }
        }))
        .await;
        assert_eq!(list_from(&base, "k").await.unwrap().len(), MAX_ITEMS);
    }

    #[test]
    fn the_base_url_is_the_loopback_constant() {
        assert_eq!(LETTA_BASE, "http://127.0.0.1:8283");
        assert!(super::super::keychain::is_allowed(LETTA_SECRET));
    }
}
