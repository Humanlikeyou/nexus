//! Read-only status for the knowledge base (Verba + Weaviate) and the crew's tool allowlist.
//! No approval dialogs, no writes. Nothing upstream is passed through verbatim: `detail` is
//! fixed text, and `crew_tools` reads only the `CREW_ALLOWED_TOOLS` key from `stack/.env.crew`.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_VERBA_URL: &str = "http://127.0.0.1:8001";
const DEFAULT_WEAVIATE_URL: &str = "http://127.0.0.1:8080";
const TIMEOUT: Duration = Duration::from_secs(3);
/// Tools the crew service implements (stack/crew-service/main.py).
pub const KNOWN_CREW_TOOLS: [&str; 2] = ["kb_query", "memory_query"];
const ALLOWLIST_KEY: &str = "CREW_ALLOWED_TOOLS";

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KbState {
    Running,
    Empty,
    NotRunning,
    Error,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct KbStatus {
    pub state: KbState,
    /// Embedding model, from the Weaviate collection name (e.g. "nomic-embed-text").
    pub model: Option<String>,
    pub docs: Option<u64>,
    /// Fixed, sanitized text only; never raw Verba/gRPC/HTTP output.
    pub detail: Option<String>,
}

impl KbStatus {
    fn of(state: KbState, detail: &str) -> Self {
        KbStatus {
            state,
            model: None,
            docs: None,
            detail: Some(detail.to_string()),
        }
    }
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct CrewToolInfo {
    pub name: String,
    pub allowed: bool,
}

fn env_url(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| v.starts_with("http://127.0.0.1") || v.starts_with("http://localhost"))
        .unwrap_or_else(|| default.to_string())
}

#[tauri::command]
pub async fn kb_status() -> Result<KbStatus, String> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|_| "kb status client failed".to_string())?;
    Ok(kb_status_at(
        &client,
        &env_url("NEXUS_VERBA_URL", DEFAULT_VERBA_URL),
        &env_url("NEXUS_WEAVIATE_URL", DEFAULT_WEAVIATE_URL),
    )
    .await)
}

/// "VERBA_Embedding_nomic_embed_text" → "nomic-embed-text".
fn embedding_model(classes: &[String]) -> Option<String> {
    classes
        .iter()
        .find_map(|c| c.strip_prefix("VERBA_Embedding_"))
        .filter(|m| m.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') && m.len() <= 64)
        .map(|m| m.replace('_', "-"))
}

async fn kb_status_at(client: &reqwest::Client, verba: &str, weaviate: &str) -> KbStatus {
    // 1. Verba health.
    let resp = match client.get(format!("{verba}/api/health")).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[kb_status] verba health unreachable ({})", kind(&e));
            return KbStatus::of(KbState::NotRunning, "knowledge base not running");
        }
    };
    if !resp.status().is_success() {
        eprintln!("[kb_status] verba health HTTP {}", resp.status().as_u16());
        return KbStatus::of(KbState::Error, "knowledge base health check failed");
    }
    let health: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    if health
        .get("error")
        .and_then(|e| e.as_str())
        .is_some_and(|e| !e.trim().is_empty())
    {
        // Verba reports failures as 200 + {error: "<raw text>"}: log that it happened, not the text.
        eprintln!("[kb_status] verba health returned an error body (not shown)");
        return KbStatus::of(KbState::Error, "knowledge base returned an error");
    }

    // 2. Weaviate (read-only): collections, then the document count if any.
    let classes: Vec<String> = match client.get(format!("{weaviate}/v1/schema")).send().await {
        Ok(r) if r.status().is_success() => r
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| v.get("classes").and_then(|c| c.as_array()).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|c| c.get("class").and_then(|n| n.as_str()).map(str::to_string))
            .collect(),
        Ok(r) => {
            eprintln!("[kb_status] weaviate schema HTTP {}", r.status().as_u16());
            return KbStatus::of(KbState::Error, "vector store query failed");
        }
        Err(e) => {
            eprintln!("[kb_status] weaviate unreachable ({})", kind(&e));
            return KbStatus::of(KbState::NotRunning, "vector store not running");
        }
    };
    let model = embedding_model(&classes);
    let docs = if classes.iter().any(|c| c == "VERBA_DOCUMENTS") {
        match document_count(client, weaviate).await {
            Some(n) => n,
            None => {
                return KbStatus {
                    model,
                    ..KbStatus::of(KbState::Error, "knowledge base query failed")
                }
            }
        }
    } else {
        0
    };
    if docs == 0 {
        return KbStatus {
            state: KbState::Empty,
            model,
            docs: Some(0),
            detail: Some("knowledge base is empty".to_string()),
        };
    }
    KbStatus {
        state: KbState::Running,
        model,
        docs: Some(docs),
        detail: None,
    }
}

async fn document_count(client: &reqwest::Client, weaviate: &str) -> Option<u64> {
    let body =
        serde_json::json!({ "query": "{ Aggregate { VERBA_DOCUMENTS { meta { count } } } }" });
    let v: serde_json::Value = client
        .post(format!("{weaviate}/v1/graphql"))
        .json(&body)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    if v.get("errors").is_some() {
        eprintln!("[kb_status] weaviate count returned errors (not shown)");
        return None;
    }
    v.pointer("/data/Aggregate/VERBA_DOCUMENTS/0/meta/count")?
        .as_u64()
}

fn kind(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timeout"
    } else if e.is_connect() {
        "connect"
    } else {
        "request"
    }
}

// ── Crew tools ──────────────────────────────────────────────────────────────

/// `NEXUS_CREW_ENV_FILE`, else `~/projects/nexus/stack/.env.crew` (written by
/// scripts/sync-crew-env.sh; a Finder-launched app has no repo path of its own).
fn crew_env_file() -> PathBuf {
    std::env::var("NEXUS_CREW_ENV_FILE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("projects/nexus/stack/.env.crew")
        })
}

#[tauri::command]
pub fn crew_tools() -> Result<Vec<CrewToolInfo>, String> {
    crew_tools_from(&crew_env_file())
}

/// Reads only the CREW_ALLOWED_TOOLS line; every other line (the crew key) is skipped
/// without being kept. Missing file or empty value: no tools.
fn crew_tools_from(path: &Path) -> Result<Vec<CrewToolInfo>, String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(Vec::new()),
    };
    if !meta.is_file() {
        return Err("crew env file is not a regular file".to_string());
    }
    let text =
        std::fs::read_to_string(path).map_err(|_| "crew env file could not be read".to_string())?;
    let raw = allowlist_value(&text).unwrap_or_default();
    let allowed: Vec<&str> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if allowed.is_empty() {
        return Ok(Vec::new());
    }
    Ok(KNOWN_CREW_TOOLS
        .iter()
        .map(|t| CrewToolInfo {
            name: t.to_string(),
            allowed: allowed.contains(t),
        })
        .collect())
}

/// Last `CREW_ALLOWED_TOOLS=` value (dotenv semantics), quotes stripped, limited to tool-name
/// characters so nothing else can leak through.
fn allowlist_value(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .find_map(|line| {
            let line = line.trim_start();
            let line = line.strip_prefix("export ").unwrap_or(line);
            line.strip_prefix(ALLOWLIST_KEY)?.strip_prefix('=')
        })
        .map(|v| {
            v.trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .chars()
                .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == ',')
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Minimal HTTP mock: answers every request by path with (status, body).
    async fn mock(routes: Vec<(&'static str, u16, &'static str)>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let (status, body) = routes
                        .iter()
                        .find(|(p, _, _)| *p == path)
                        .map(|(_, s, b)| (*s, *b))
                        .unwrap_or((404, "{}"));
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder().timeout(TIMEOUT).build().unwrap()
    }

    const ALIVE: &str = r#"{"message":"Alive!"}"#;
    const GRPC: &str = r#"{"error":"Query failed: AioRpcError status = StatusCode.UNKNOWN debug_error_string = \"ipv4:172.18.0.4:50051\""}"#;

    #[tokio::test]
    async fn connection_refused_is_not_running() {
        // Bind then drop: nothing listens on this port.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("http://127.0.0.1:{port}");
        let s = kb_status_at(&client(), &url, &url).await;
        assert_eq!(s.state, KbState::NotRunning);
        assert_eq!(s.detail.as_deref(), Some("knowledge base not running"));
    }

    #[tokio::test]
    async fn verba_200_with_error_is_sanitized() {
        let url = mock(vec![("/api/health", 200, GRPC)]).await;
        let s = kb_status_at(&client(), &url, &url).await;
        assert_eq!(s.state, KbState::Error);
        assert_eq!(
            s.detail.as_deref(),
            Some("knowledge base returned an error")
        );
        let json = serde_json::to_string(&s).unwrap();
        for raw in ["AioRpcError", "StatusCode", "172.18", "debug_error_string"] {
            assert!(!json.contains(raw), "{raw} leaked");
        }
    }

    #[tokio::test]
    async fn no_documents_collection_is_empty_with_model() {
        let url = mock(vec![
            ("/api/health", 200, ALIVE),
            (
                "/v1/schema",
                200,
                r#"{"classes":[{"class":"VERBA_CONFIGURATION"},{"class":"VERBA_Embedding_nomic_embed_text"}]}"#,
            ),
        ])
        .await;
        let s = kb_status_at(&client(), &url, &url).await;
        assert_eq!(
            s,
            KbStatus {
                state: KbState::Empty,
                model: Some("nomic-embed-text".into()),
                docs: Some(0),
                detail: Some("knowledge base is empty".into()),
            }
        );
        assert_eq!(
            serde_json::to_value(&s).unwrap()["state"],
            serde_json::json!("empty")
        );
    }

    #[tokio::test]
    async fn documents_present_is_running_with_count() {
        let url = mock(vec![
            ("/api/health", 200, ALIVE),
            (
                "/v1/schema",
                200,
                r#"{"classes":[{"class":"VERBA_DOCUMENTS"},{"class":"VERBA_Embedding_nomic_embed_text"}]}"#,
            ),
            (
                "/v1/graphql",
                200,
                r#"{"data":{"Aggregate":{"VERBA_DOCUMENTS":[{"meta":{"count":8}}]}}}"#,
            ),
        ])
        .await;
        let s = kb_status_at(&client(), &url, &url).await;
        assert_eq!((s.state, s.docs), (KbState::Running, Some(8)));
        assert_eq!(s.detail, None);
    }

    #[tokio::test]
    async fn graphql_errors_and_http_errors_are_fixed_text() {
        let url = mock(vec![
            ("/api/health", 200, ALIVE),
            (
                "/v1/schema",
                200,
                r#"{"classes":[{"class":"VERBA_DOCUMENTS"}]}"#,
            ),
            (
                "/v1/graphql",
                200,
                r#"{"errors":[{"message":"raw weaviate text"}]}"#,
            ),
        ])
        .await;
        let s = kb_status_at(&client(), &url, &url).await;
        assert_eq!(s.state, KbState::Error);
        assert!(!serde_json::to_string(&s).unwrap().contains("raw weaviate"));

        let url = mock(vec![("/api/health", 500, r#"{"detail":"Traceback ..."}"#)]).await;
        let s = kb_status_at(&client(), &url, &url).await;
        assert_eq!(s.state, KbState::Error);
        assert!(!serde_json::to_string(&s).unwrap().contains("Traceback"));
    }

    fn env_file(body: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("nexus-crew-env-{}", uuid::Uuid::new_v4()));
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn crew_tools_reads_only_the_allowlist_and_never_the_secret() {
        let secret = "SECRETVALUE_should_never_appear_0123456789abcdef";
        let p = env_file(&format!(
            "# generated\nCREW_INGRESS_SECRET={secret}\nCREW_ALLOWED_TOOLS=kb_query\nOTHER=x\n"
        ));
        let tools = crew_tools_from(&p).unwrap();
        assert_eq!(
            tools,
            vec![
                CrewToolInfo {
                    name: "kb_query".into(),
                    allowed: true
                },
                CrewToolInfo {
                    name: "memory_query".into(),
                    allowed: false
                },
            ]
        );
        let json = serde_json::to_string(&tools).unwrap();
        assert!(!json.contains(secret) && !json.contains("SECRETVALUE"));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn crew_tools_both_allowed_last_line_wins_and_quotes_strip() {
        let p =
            env_file("CREW_ALLOWED_TOOLS=\nexport CREW_ALLOWED_TOOLS=\"kb_query, memory_query\"\n");
        assert!(crew_tools_from(&p).unwrap().iter().all(|t| t.allowed));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn empty_or_missing_allowlist_gives_no_tools() {
        for body in [
            "CREW_INGRESS_SECRET=abc\n",
            "CREW_ALLOWED_TOOLS=\n",
            "CREW_ALLOWED_TOOLS=  \"\"\n",
            "",
        ] {
            let p = env_file(body);
            assert_eq!(crew_tools_from(&p).unwrap(), vec![], "{body:?}");
            let _ = std::fs::remove_file(p);
        }
        let missing = std::env::temp_dir().join("nexus-crew-env-does-not-exist");
        assert_eq!(crew_tools_from(&missing).unwrap(), vec![]);
    }

    #[test]
    fn a_similar_key_name_is_not_the_allowlist() {
        let p = env_file("CREW_ALLOWED_TOOLS_OLD=kb_query\nXCREW_ALLOWED_TOOLS=kb_query\n");
        assert_eq!(crew_tools_from(&p).unwrap(), vec![]);
        let _ = std::fs::remove_file(p);
    }
}
