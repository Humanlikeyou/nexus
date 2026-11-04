//! Bounded, read-only bridge to the NOOP `noop-local-access` MCP CLI (stdio JSON-RPC).
//! Fixed tool allowlist, validated args, no shell, hard timeout, parsed JSON.
//! Never logs payloads: results may contain biometric values.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
pub const ALLOWED_TOOLS: [&str; 5] = [
    "health_snapshot",
    "metric_series",
    "data_freshness",
    "sleep_summary",
    "workout_summary",
];
const METRIC_KEYS: [&str; 9] = [
    "recovery",
    "hrv",
    "rhr",
    "resp_rate",
    "spo2",
    "strain",
    "sleep_total_min",
    "steps",
    "active_kcal",
];
const SETTINGS_KEY: &str = "noop_settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WhoopSource {
    /// "NOOP (recommended)": NOOP owns strap sync; the direct BLE path must not start.
    #[default]
    #[serde(rename = "noop")]
    Noop,
    /// "Direct BLE".
    #[serde(rename = "ble")]
    Ble,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct NoopSettings {
    #[serde(default)]
    pub whoop_source: WhoopSource,
    #[serde(default)]
    pub binary_path: Option<String>,
    #[serde(default)]
    pub db_path: Option<String>,
}

impl NoopSettings {
    /// Gate for any direct WHOOP BLE startup. False whenever NOOP is selected.
    /// No direct WHOOP BLE code is wired in this repo today; any future BLE start must check this.
    pub fn whoop_ble_allowed(&self) -> bool {
        self.whoop_source == WhoopSource::Ble
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum NoopQueryError {
    ToolNotAllowed(String),
    InvalidArgs(String),
    BinaryMissing(String),
    SpawnFailed(String),
    Timeout(u64),
    ProcessFailed(String),
    BadOutput(String),
    ToolError(String),
}

impl NoopQueryError {
    /// Variant name only; safe to log (no payload).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ToolNotAllowed(_) => "tool_not_allowed",
            Self::InvalidArgs(_) => "invalid_args",
            Self::BinaryMissing(_) => "binary_missing",
            Self::SpawnFailed(_) => "spawn_failed",
            Self::Timeout(_) => "timeout",
            Self::ProcessFailed(_) => "process_failed",
            Self::BadOutput(_) => "bad_output",
            Self::ToolError(_) => "tool_error",
        }
    }
}

pub fn load_settings() -> NoopSettings {
    super::kv::load_kv(SETTINGS_KEY.to_string())
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn get_noop_settings() -> NoopSettings {
    load_settings()
}

#[tauri::command]
pub fn set_noop_settings(settings: NoopSettings) -> Result<(), String> {
    for p in [&settings.binary_path, &settings.db_path]
        .into_iter()
        .flatten()
    {
        if !Path::new(p).is_absolute() {
            return Err("paths must be absolute".into());
        }
    }
    let s = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    super::kv::save_kv(SETTINGS_KEY.to_string(), s)
}

fn int_arg(a: &Map<String, Value>, k: &str, min: i64, max: i64) -> Result<(), NoopQueryError> {
    match a.get(k) {
        None => Ok(()),
        Some(v) => match v.as_i64() {
            Some(n) if (min..=max).contains(&n) => Ok(()),
            _ => Err(NoopQueryError::InvalidArgs(format!(
                "{k} must be an integer {min}..={max}"
            ))),
        },
    }
}

fn is_day(s: &str) -> bool {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() && s.len() == 10
}

fn is_source_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Validate tool + args against the allowlist. Unknown keys are rejected.
pub fn validate(tool: &str, args: &Value) -> Result<Map<String, Value>, NoopQueryError> {
    if !ALLOWED_TOOLS.contains(&tool) {
        return Err(NoopQueryError::ToolNotAllowed(
            tool.chars().take(64).collect(),
        ));
    }
    let a = match args {
        Value::Null => Map::new(),
        Value::Object(m) => m.clone(),
        _ => return Err(NoopQueryError::InvalidArgs("args must be an object".into())),
    };
    let allowed: &[&str] = match tool {
        "health_snapshot" | "sleep_summary" | "workout_summary" => &["days"],
        "metric_series" => &["key", "source", "days", "from_day", "to_day", "limit"],
        _ => &[],
    };
    if let Some(k) = a.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(NoopQueryError::InvalidArgs(format!(
            "unexpected arg: {}",
            k.chars().take(32).collect::<String>()
        )));
    }
    let max_days = if tool == "health_snapshot" { 120 } else { 4000 };
    int_arg(&a, "days", 1, max_days)?;
    if tool == "metric_series" {
        match a.get("key").and_then(Value::as_str) {
            Some(k) if METRIC_KEYS.contains(&k) => {}
            _ => {
                return Err(NoopQueryError::InvalidArgs(
                    "key must be a known metric key".into(),
                ))
            }
        }
        if let Some(v) = a.get("source") {
            if !v.as_str().is_some_and(is_source_id) {
                return Err(NoopQueryError::InvalidArgs(
                    "source must match [A-Za-z0-9_-]{1,64}".into(),
                ));
            }
        }
        for k in ["from_day", "to_day"] {
            if let Some(v) = a.get(k) {
                if !v.as_str().is_some_and(is_day) {
                    return Err(NoopQueryError::InvalidArgs(format!(
                        "{k} must be YYYY-MM-DD"
                    )));
                }
            }
        }
        int_arg(&a, "limit", 1, 2000)?;
    }
    Ok(a)
}

/// Run one MCP tools/call against `binary` with a hard timeout (child killed on expiry).
pub fn run_query(
    binary: &Path,
    db_path: Option<&str>,
    tool: &str,
    args: &Value,
    timeout: Duration,
) -> Result<Value, NoopQueryError> {
    let args = validate(tool, args)?;
    if !binary.is_absolute() || !binary.is_file() {
        return Err(NoopQueryError::BinaryMissing(
            "NOOP CLI not found; build Packages/NoopLocalAccess with `swift build -c release` and set its path in Settings".into(),
        ));
    }
    let mut cmd = Command::new(binary);
    cmd.arg("mcp")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    if let Some(db) = db_path {
        cmd.env("NOOP_DB_PATH", db);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| NoopQueryError::SpawnFailed(e.kind().to_string()))?;
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"nexus","version":"1"}}});
    let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":Value::Object(args)}});
    {
        let mut stdin = child.stdin.take().expect("piped stdin");
        let _ = writeln!(stdin, "{init}\n{call}");
    } // closing stdin ends the server's read loop
    let mut stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.by_ref().take(16 * 1024 * 1024).read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(NoopQueryError::Timeout(timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(NoopQueryError::ProcessFailed(e.kind().to_string())),
        }
    };
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(NoopQueryError::ProcessFailed(format!(
            "exit {:?}",
            status.code()
        )));
    }
    parse_response(&out)
}

/// Find the id=2 response; return parsed structured content (or JSON text content).
pub fn parse_response(out: &[u8]) -> Result<Value, NoopQueryError> {
    let text = std::str::from_utf8(out)
        .map_err(|_| NoopQueryError::BadOutput("non-UTF-8 stdout".into()))?;
    let resp = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|v| v.get("id") == Some(&json!(2)))
        .ok_or_else(|| NoopQueryError::BadOutput("no tools/call response".into()))?;
    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error");
        return Err(NoopQueryError::ToolError(msg.chars().take(200).collect()));
    }
    let result = resp
        .get("result")
        .ok_or_else(|| NoopQueryError::BadOutput("missing result".into()))?;
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(NoopQueryError::ToolError("tool reported an error".into()));
    }
    if let Some(sc) = result.get("structuredContent") {
        return Ok(sc.clone());
    }
    let t = result
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| NoopQueryError::BadOutput("no content".into()))?;
    serde_json::from_str(t).map_err(|_| NoopQueryError::BadOutput("content is not JSON".into()))
}

#[tauri::command]
pub async fn noop_query(tool: String, args: Option<Value>) -> Result<Value, NoopQueryError> {
    let s = load_settings();
    let bin = PathBuf::from(s.binary_path.clone().unwrap_or_default());
    let args = args.unwrap_or(Value::Null);
    tauri::async_runtime::spawn_blocking(move || {
        run_query(&bin, s.db_path.as_deref(), &tool, &args, DEFAULT_TIMEOUT)
    })
    .await
    .map_err(|_| NoopQueryError::ProcessFailed("join".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_tool_and_args() {
        assert!(matches!(
            validate("rm", &Value::Null),
            Err(NoopQueryError::ToolNotAllowed(_))
        ));
        assert!(validate("data_freshness", &json!({"days": 1})).is_err());
        assert!(validate("health_snapshot", &json!({"days": 121})).is_err());
        assert!(validate("health_snapshot", &json!({"days": "5"})).is_err());
        assert!(validate("health_snapshot", &json!([1])).is_err());
        assert!(validate("metric_series", &json!({})).is_err());
        assert!(validate("metric_series", &json!({"key": "hrv; rm -rf"})).is_err());
        assert!(validate("metric_series", &json!({"key": "hrv", "source": "../x"})).is_err());
        assert!(validate(
            "metric_series",
            &json!({"key": "hrv", "from_day": "2026-13-01"})
        )
        .is_err());
        assert!(validate("metric_series", &json!({"key": "hrv", "limit": 0})).is_err());
    }

    #[test]
    fn accepts_valid_args() {
        for t in ALLOWED_TOOLS.iter().filter(|t| **t != "metric_series") {
            assert!(validate(t, &Value::Null).is_ok());
        }
        assert!(validate("health_snapshot", &json!({"days": 14})).is_ok());
        assert!(validate("metric_series", &json!({"key": "recovery", "source": "my-whoop", "from_day": "2026-10-01", "to_day": "2026-10-31", "limit": 100})).is_ok());
    }

    #[test]
    fn missing_binary_is_structured() {
        let r = run_query(
            Path::new("/nonexistent/noop-local-access"),
            None,
            "data_freshness",
            &Value::Null,
            DEFAULT_TIMEOUT,
        );
        assert!(matches!(r, Err(NoopQueryError::BinaryMissing(_))));
        let r = run_query(
            Path::new("relative"),
            None,
            "data_freshness",
            &Value::Null,
            DEFAULT_TIMEOUT,
        );
        assert!(matches!(r, Err(NoopQueryError::BinaryMissing(_))));
    }

    #[cfg(unix)]
    fn script(body: &str) -> (tempdir_lite::Dir, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let d = tempdir_lite::Dir::new();
        let p = d.0.join("fake-noop");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        (d, p)
    }

    #[cfg(unix)]
    #[test]
    fn parses_fake_server_and_times_out() {
        let (_d, p) = script(
            r#"cat >/dev/null; echo '{"jsonrpc":"2.0","id":1,"result":{}}'; echo '{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"{\"ok\":true,\"n\":3}"}]}}'"#,
        );
        let v = run_query(&p, None, "data_freshness", &Value::Null, DEFAULT_TIMEOUT).unwrap();
        assert_eq!(v["n"], 3);
        let (_d2, p2) = script("sleep 30");
        let t = Instant::now();
        let r = run_query(
            &p2,
            None,
            "data_freshness",
            &Value::Null,
            Duration::from_millis(300),
        );
        assert!(matches!(r, Err(NoopQueryError::Timeout(_))));
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn parse_errors_are_structured() {
        assert!(matches!(
            parse_response(b"garbage"),
            Err(NoopQueryError::BadOutput(_))
        ));
        let e = br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"bad"}}"#;
        assert!(matches!(
            parse_response(e),
            Err(NoopQueryError::ToolError(_))
        ));
        let sc = br#"{"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"a":1}}}"#;
        assert_eq!(parse_response(sc).unwrap()["a"], 1);
    }

    #[test]
    fn whoop_source_defaults_to_noop_and_blocks_ble() {
        let s: NoopSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.whoop_source, WhoopSource::Noop);
        assert!(!s.whoop_ble_allowed());
        let b: NoopSettings = serde_json::from_str(r#"{"whoop_source":"ble"}"#).unwrap();
        assert!(b.whoop_ble_allowed());
    }

    mod tempdir_lite {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new() -> Self {
                use std::sync::atomic::{AtomicU32, Ordering};
                static N: AtomicU32 = AtomicU32::new(0);
                let p = std::env::temp_dir().join(format!(
                    "noopq-{}-{}",
                    std::process::id(),
                    N.fetch_add(1, Ordering::SeqCst)
                ));
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
