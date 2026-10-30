//! Read-only status for the knowledge base (Verba + Weaviate) and the crew's tool allowlist.
//! No approval dialogs, no writes. Nothing upstream is passed through verbatim: `detail` is
//! fixed text, and `crew_tools` reads only `stack/crew-tools.conf`
//! (never `.env.crew`, which holds the crew key).

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Fixed loopback endpoints; deliberately not configurable (no env/URL override).
const KB_HOST: &str = "127.0.0.1";
const VERBA_PORT: u16 = 8001;
const WEAVIATE_PORT: u16 = 8080;
const CREW_TOOLS_FILE: &str = "crew-tools.conf";
const CREW_TOOLS_UNAVAILABLE: &str = "crew tools config unavailable";
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

fn loopback_url(port: u16) -> String {
    format!("http://{KB_HOST}:{port}")
}

#[tauri::command]
pub async fn kb_status() -> Result<KbStatus, String> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|_| "kb status client failed".to_string())?;
    Ok(kb_status_at(
        &client,
        &loopback_url(VERBA_PORT),
        &loopback_url(WEAVIATE_PORT),
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

/// `~/projects/nexus/stack` (a Finder-launched app has no repo path of its own).
fn stack_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("projects/nexus/stack")
}

#[tauri::command]
pub fn crew_tools() -> Result<Vec<CrewToolInfo>, String> {
    let stack = stack_dir();
    crew_tools_at(&stack, &stack.join(CREW_TOOLS_FILE))
}

fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// Reads `file` (normally `<stack>/crew-tools.conf`, written by scripts/sync-crew-env.sh) and
/// only its CREW_ALLOWED_TOOLS line. Refuses symlinks, files outside `stack`, files not owned
/// by this user, and files readable by group/other. Missing file or empty value: no tools.
/// Errors are fixed text: no paths, no contents.
fn crew_tools_at(stack: &Path, file: &Path) -> Result<Vec<CrewToolInfo>, String> {
    use std::os::unix::fs::MetadataExt;
    let unavailable = || CREW_TOOLS_UNAVAILABLE.to_string();
    let meta = match std::fs::symlink_metadata(file) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(unavailable()),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(unavailable());
    }
    // Canonical location must be inside the (existing) stack dir; path_safety does the check.
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(unavailable)?;
    if !stack.is_dir() {
        return Err(unavailable());
    }
    let inside = crate::commands::path_safety::safe_join(stack, name).map_err(|_| unavailable())?;
    let canon = file.canonicalize().map_err(|_| unavailable())?;
    if inside != canon {
        return Err(unavailable());
    }
    if meta.uid() != current_uid() || meta.mode() & 0o077 != 0 {
        return Err(unavailable());
    }
    let text = std::fs::read_to_string(&canon).map_err(|_| unavailable())?;
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

    fn stack(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-stack-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    fn conf(dir: &Path, body: &str, mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(CREW_TOOLS_FILE);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn crew_tools_reads_conf_and_never_env_crew() {
        let d = stack("ok");
        let secret = "SECRETVALUE_should_never_appear_0123456789abcdef";
        // Decoy: .env.crew says both tools + a secret; the conf says only kb_query.
        std::fs::write(
            d.join(".env.crew"),
            format!("CREW_INGRESS_SECRET={secret}\nCREW_ALLOWED_TOOLS=kb_query,memory_query\n"),
        )
        .unwrap();
        let p = conf(
            &d,
            "# x\nOTHER=memory_query\nCREW_ALLOWED_TOOLS=kb_query\n",
            0o600,
        );
        let tools = crew_tools_at(&d, &p).unwrap();
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
        assert!(!json.contains("SECRETVALUE"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn crew_tools_both_allowed_last_line_wins_and_quotes_strip() {
        let d = stack("both");
        let p = conf(
            &d,
            "CREW_ALLOWED_TOOLS=\nexport CREW_ALLOWED_TOOLS=\"kb_query, memory_query\"\n",
            0o600,
        );
        assert!(crew_tools_at(&d, &p).unwrap().iter().all(|t| t.allowed));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn missing_empty_or_other_keys_give_no_tools() {
        let d = stack("empty");
        assert_eq!(crew_tools_at(&d, &d.join(CREW_TOOLS_FILE)).unwrap(), vec![]);
        for body in [
            "",
            "CREW_ALLOWED_TOOLS=\n",
            "CREW_ALLOWED_TOOLS=  \"\"\n",
            "CREW_ALLOWED_TOOLS_OLD=kb_query\nXCREW_ALLOWED_TOOLS=kb_query\n",
        ] {
            let p = conf(&d, body, 0o600);
            assert_eq!(crew_tools_at(&d, &p).unwrap(), vec![], "{body:?}");
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn symlink_wrong_mode_and_outside_path_are_refused_with_fixed_text() {
        let d = stack("refuse");
        let other = stack("outside");
        let real = conf(&other, "CREW_ALLOWED_TOOLS=kb_query\n", 0o600);
        // Symlink inside stack pointing at a valid file.
        let link = d.join(CREW_TOOLS_FILE);
        std::os::unix::fs::symlink(&real, &link).unwrap();
        // Outside the stack dir.
        let errs = [
            crew_tools_at(&d, &link).unwrap_err(),
            crew_tools_at(&d, &real).unwrap_err(),
        ];
        std::fs::remove_file(&link).unwrap();
        // Group/other readable.
        let loose = conf(&d, "CREW_ALLOWED_TOOLS=kb_query\n", 0o644);
        let e3 = crew_tools_at(&d, &loose).unwrap_err();
        for e in errs.iter().chain([&e3]) {
            assert_eq!(e, CREW_TOOLS_UNAVAILABLE);
            assert!(!e.contains('/') && !e.contains("kb_query"));
        }
        let _ = std::fs::remove_dir_all(d);
        let _ = std::fs::remove_dir_all(other);
    }

    #[test]
    fn kb_endpoints_are_fixed_loopback() {
        assert_eq!(loopback_url(VERBA_PORT), "http://127.0.0.1:8001");
        assert_eq!(loopback_url(WEAVIATE_PORT), "http://127.0.0.1:8080");
    }
}
