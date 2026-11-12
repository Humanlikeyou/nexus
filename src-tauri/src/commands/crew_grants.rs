//! 1110i: the per-crew Files / terminal switch. The enforcement lives in crew-service; these
//! commands are the app's side. Turning it ON goes through the native approval dialog (a click in
//! the page alone can never grant), then crew-service must confirm within 15 s or nothing is on.
//! Loopback only; no secrets or command output pass through here.

use super::crew::{crew_service_url, crew_token, http_error, CREW_TOKEN_HEADER};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const TOOLS: [&str; 3] = ["fs_read", "fs_write", "shell"];
const DURATIONS: [&str; 3] = ["run", "1h", "24h"];
const DEFAULT_DURATION: &str = "run";
const CONFIRM_TIMEOUT_S: u64 = 15;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct CrewToolGrant {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub scope_path: Option<String>,
    #[serde(default)]
    pub duration: Option<String>,
    #[serde(default)]
    pub granted_at: Option<f64>,
    #[serde(default)]
    pub expires_at: Option<f64>,
    #[serde(default)]
    pub granted_by: Option<String>,
    /// From crew-service: what it is enforcing right now. The UI shows "On" from this only.
    #[serde(default)]
    pub enforced_allowlist: Vec<String>,
    #[serde(default)]
    pub pending_request: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct CrewAuditRow {
    pub id: i64,
    pub at: f64,
    pub actor: String,
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i64>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
    #[serde(default)]
    pub checkpoint: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
}

/// Crew names as the crew service accepts them: 1-64 of letters, digits, space, `_ . -`.
pub(crate) fn valid_crew(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.chars().count() <= 64
        && first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-'))
}

fn slug(crew: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in crew.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let t = out.trim_matches('-');
    if t.is_empty() {
        "crew".into()
    } else {
        t.into()
    }
}

/// Validates the switch request. Duration defaults to "this run only"; there is no "until off".
fn check_set(
    crew: &str,
    tools: &[String],
    duration: Option<&str>,
) -> Result<(Vec<String>, String), String> {
    if !valid_crew(crew) {
        return Err("Invalid crew name.".into());
    }
    if tools.is_empty() || tools.iter().any(|t| !TOOLS.contains(&t.as_str())) {
        return Err("Invalid tool list.".into());
    }
    let duration = duration.unwrap_or(DEFAULT_DURATION);
    if !DURATIONS.contains(&duration) {
        return Err("Invalid duration.".into());
    }
    let mut t: Vec<String> = tools.to_vec();
    t.sort();
    t.dedup();
    Ok((t, duration.to_string()))
}

fn dialog_text(crew: &str, tools: &[String], duration: &str) -> Result<String, String> {
    let when = match duration {
        "run" => "this run only",
        "1h" => "1 hour",
        _ => "24 hours",
    };
    super::approval_gate::approval_text(
        &format!(
            "Turn on files and terminal for crew: {}",
            crew.chars().take(64).collect::<String>()
        ),
        &format!(
            "Tools: {}. Folder: crew-sandbox/{} only. For: {}. Writes and commands still ask you once per run. Network, git push, sudo and spending stay blocked.",
            tools.join(", "),
            slug(crew),
            when
        ),
    )
}

trait Backend: Sync {
    fn get(
        &self,
        crew: &str,
    ) -> impl std::future::Future<Output = Result<CrewToolGrant, String>> + Send;
    fn set(
        &self,
        crew: &str,
        tools: &[String],
        duration: &str,
    ) -> impl std::future::Future<Output = Result<CrewToolGrant, String>> + Send;
    fn revoke(
        &self,
        crew: &str,
    ) -> impl std::future::Future<Output = Result<CrewToolGrant, String>> + Send;
    fn audit(
        &self,
        crew: &str,
        limit: u32,
        before: Option<i64>,
    ) -> impl std::future::Future<Output = Result<Vec<CrewAuditRow>, String>> + Send;
}

trait Gate: Sync {
    fn approve(
        &self,
        key: String,
        text: String,
    ) -> impl std::future::Future<Output = Result<Box<dyn Send>, String>> + Send;
}

struct NativeGate(tauri::AppHandle);

impl Gate for NativeGate {
    async fn approve(&self, key: String, text: String) -> Result<Box<dyn Send>, String> {
        let guard = super::approval_gate::require_approval(&self.0, &[key], text).await?;
        Ok(Box::new(guard))
    }
}

struct HttpBackend;

fn url(crew: &str, tail: &str) -> String {
    // The name is validated (no `/`, `?`, `#`, `%`), so it cannot alter the path.
    format!("{}/crews/tools/{}/{}", crew_service_url(), crew, tail)
}

async fn read_grant(resp: reqwest::Response, op: &str) -> Result<CrewToolGrant, String> {
    match resp.status().as_u16() {
        200..=299 => {}
        403 => return Err("Crew service refused to turn this on.".into()),
        422 => {
            return Err(
                "Could not turn it on: the sandbox folder is not set up for crews yet.".into(),
            )
        }
        _ => return Err(http_error(op, resp.status())),
    }
    resp.json::<CrewToolGrant>()
        .await
        .map_err(|_| "Unexpected grant state from the crew service.".to_string())
}

impl Backend for HttpBackend {
    async fn get(&self, crew: &str) -> Result<CrewToolGrant, String> {
        let resp = reqwest::Client::new()
            .get(url(crew, "grant"))
            .header(CREW_TOKEN_HEADER, crew_token()?)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|_| "Crew service unreachable.".to_string())?;
        read_grant(resp, "grant").await
    }

    async fn set(
        &self,
        crew: &str,
        tools: &[String],
        duration: &str,
    ) -> Result<CrewToolGrant, String> {
        // crew-service must confirm the enforced allowlist; a timeout is an error and stays off.
        let resp = reqwest::Client::new()
            .post(url(crew, "grant"))
            .header(CREW_TOKEN_HEADER, crew_token()?)
            .json(&serde_json::json!({ "tools": tools, "duration": duration, "granted_by": "taisun" }))
            .timeout(std::time::Duration::from_secs(CONFIRM_TIMEOUT_S))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "Crew service did not confirm in time. Files and terminal stay off.".to_string()
                } else {
                    "Crew service unreachable.".to_string()
                }
            })?;
        read_grant(resp, "grant").await
    }

    async fn revoke(&self, crew: &str) -> Result<CrewToolGrant, String> {
        let resp = reqwest::Client::new()
            .post(url(crew, "revoke"))
            .header(CREW_TOKEN_HEADER, crew_token()?)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|_| "Crew service unreachable.".to_string())?;
        read_grant(resp, "revoke").await
    }

    async fn audit(
        &self,
        crew: &str,
        limit: u32,
        before: Option<i64>,
    ) -> Result<Vec<CrewAuditRow>, String> {
        let mut q = vec![("limit", limit.to_string())];
        if let Some(b) = before {
            q.push(("before", b.to_string()));
        }
        let resp = reqwest::Client::new()
            .get(url(crew, "audit"))
            .header(CREW_TOKEN_HEADER, crew_token()?)
            .query(&q)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|_| "Crew service unreachable.".to_string())?;
        if !resp.status().is_success() {
            return Err(http_error("audit", resp.status()));
        }
        resp.json::<Vec<CrewAuditRow>>()
            .await
            .map_err(|_| "Unexpected audit rows from the crew service.".to_string())
    }
}

async fn set_gated(
    gate: &impl Gate,
    backend: &impl Backend,
    crew: String,
    tools: Vec<String>,
    duration: Option<String>,
) -> Result<CrewToolGrant, String> {
    let (tools, duration) = check_set(&crew, &tools, duration.as_deref())?;
    let text = dialog_text(&crew, &tools, &duration)?;
    let _held = gate
        .approve(
            super::approval_gate::service_key(&format!("crew-grant:{crew}")),
            text,
        )
        .await?;
    let state = backend.set(&crew, &tools, &duration).await?;
    // "On" only if crew-service reports it enforcing the tools just granted.
    if !state.enabled || tools.iter().any(|t| !state.enforced_allowlist.contains(t)) {
        return Err(
            "Crew service did not confirm the allowlist. Files and terminal stay off.".into(),
        );
    }
    Ok(state)
}

#[tauri::command]
pub async fn crew_tool_grant_get(crew_id: String) -> Result<CrewToolGrant, String> {
    if !valid_crew(&crew_id) {
        return Err("Invalid crew name.".into());
    }
    HttpBackend.get(&crew_id).await
}

/// Taisun's switch. `duration` is "run" (default), "1h" or "24h".
#[tauri::command]
pub async fn crew_tool_grant_set(
    app: tauri::AppHandle,
    crew_id: String,
    tools: Vec<String>,
    duration: Option<String>,
) -> Result<CrewToolGrant, String> {
    set_gated(&NativeGate(app), &HttpBackend, crew_id, tools, duration).await
}

/// Turning it off needs no dialog (it only removes access).
#[tauri::command]
pub async fn crew_tool_grant_revoke(crew_id: String) -> Result<CrewToolGrant, String> {
    if !valid_crew(&crew_id) {
        return Err("Invalid crew name.".into());
    }
    HttpBackend.revoke(&crew_id).await
}

#[tauri::command]
pub async fn crew_tool_audit(
    crew_id: String,
    limit: Option<u32>,
    before: Option<i64>,
) -> Result<Vec<CrewAuditRow>, String> {
    if !valid_crew(&crew_id) {
        return Err("Invalid crew name.".into());
    }
    HttpBackend
        .audit(&crew_id, limit.unwrap_or(50).clamp(1, 200), before)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn crew_names_cannot_alter_the_request_path() {
        for ok in ["Research", "Taiyo Skin crew", "a.b-c_d", "x"] {
            assert!(valid_crew(ok), "{ok}");
        }
        for bad in [
            "",
            " x",
            "../x",
            "a/b",
            "a?b",
            "a#b",
            "a%2f",
            "-x",
            &"x".repeat(65),
        ] {
            assert!(!valid_crew(bad), "{bad}");
        }
    }

    #[test]
    fn set_defaults_to_this_run_and_has_no_until_off() {
        let t = vec![
            "shell".to_string(),
            "fs_read".to_string(),
            "shell".to_string(),
        ];
        let (tools, d) = check_set("Research", &t, None).unwrap();
        assert_eq!(
            (tools, d.as_str()),
            (vec!["fs_read".into(), "shell".into()], "run")
        );
        for d in ["until_off", "forever", "48h", ""] {
            assert!(check_set("Research", &t, Some(d)).is_err(), "{d}");
        }
        assert!(check_set("Research", &["sudo".to_string()], None).is_err());
        assert!(check_set("Research", &[], None).is_err());
        assert!(check_set("../x", &t, None).is_err());
    }

    #[test]
    fn dialog_names_crew_tools_folder_and_the_blocked_list() {
        let text = dialog_text("Taiyo Skin", &["shell".into()], "1h").unwrap();
        for want in [
            "Taiyo Skin",
            "shell",
            "crew-sandbox/taiyo-skin",
            "1 hour",
            "git push",
            "sudo",
        ] {
            assert!(text.contains(want), "{want}: {text}");
        }
        assert!(dialog_text("R", &["shell".into()], "run")
            .unwrap()
            .contains("this run only"));
    }

    struct Stub {
        asked: Mutex<Vec<String>>,
        allow: bool,
    }
    impl Gate for Stub {
        async fn approve(&self, _key: String, text: String) -> Result<Box<dyn Send>, String> {
            self.asked.lock().unwrap().push(text);
            if self.allow {
                Ok(Box::new(()))
            } else {
                Err(super::super::approval_gate::denied_message())
            }
        }
    }

    struct Fake {
        calls: Mutex<Vec<String>>,
        reply: CrewToolGrant,
    }
    impl Backend for Fake {
        async fn get(&self, _c: &str) -> Result<CrewToolGrant, String> {
            Ok(self.reply.clone())
        }
        async fn set(&self, c: &str, t: &[String], d: &str) -> Result<CrewToolGrant, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{c}|{}|{d}", t.join(",")));
            Ok(self.reply.clone())
        }
        async fn revoke(&self, _c: &str) -> Result<CrewToolGrant, String> {
            Ok(CrewToolGrant::default())
        }
        async fn audit(
            &self,
            _c: &str,
            _l: u32,
            _b: Option<i64>,
        ) -> Result<Vec<CrewAuditRow>, String> {
            Ok(vec![])
        }
    }
    fn fake(enabled: bool, enforced: &[&str]) -> Fake {
        Fake {
            calls: Default::default(),
            reply: CrewToolGrant {
                enabled,
                tools: vec!["shell".into()],
                enforced_allowlist: enforced.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn on_needs_the_native_dialog_and_the_service_confirmation() {
        let gate = Stub {
            asked: Default::default(),
            allow: true,
        };
        let be = fake(true, &["kb_query", "shell"]);
        let st = set_gated(&gate, &be, "Research".into(), vec!["shell".into()], None)
            .await
            .unwrap();
        assert!(st.enabled);
        assert_eq!(gate.asked.lock().unwrap().len(), 1);
        assert_eq!(
            *be.calls.lock().unwrap(),
            vec!["Research|shell|run".to_string()]
        );
    }

    #[tokio::test]
    async fn a_denied_dialog_sends_nothing() {
        let gate = Stub {
            asked: Default::default(),
            allow: false,
        };
        let be = fake(true, &["shell"]);
        let err = set_gated(&gate, &be, "Research".into(), vec!["shell".into()], None)
            .await
            .unwrap_err();
        assert!(err.starts_with("declined:"), "{err}");
        assert!(be.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn bad_input_is_refused_before_any_dialog_or_request() {
        let gate = Stub {
            asked: Default::default(),
            allow: true,
        };
        let be = fake(true, &["shell"]);
        for (c, t, d) in [
            ("../x", vec!["shell"], None),
            ("R", vec!["sudo"], None),
            ("R", vec!["shell"], Some("until_off")),
        ] {
            let r = set_gated(
                &gate,
                &be,
                c.into(),
                t.into_iter().map(String::from).collect(),
                d.map(String::from),
            )
            .await;
            assert!(r.is_err());
        }
        assert!(gate.asked.lock().unwrap().is_empty());
        assert!(be.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unconfirmed_allowlist_is_an_error_not_on() {
        let gate = Stub {
            asked: Default::default(),
            allow: true,
        };
        for be in [fake(false, &["shell"]), fake(true, &["kb_query"])] {
            let err = set_gated(&gate, &be, "R".into(), vec!["shell".into()], None)
                .await
                .unwrap_err();
            assert!(err.contains("stay off"), "{err}");
        }
    }

    #[test]
    fn state_and_audit_rows_deserialize_with_defaults() {
        let g: CrewToolGrant = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
        assert_eq!(g, CrewToolGrant::default());
        let g: CrewToolGrant = serde_json::from_str(
            r#"{"enabled":true,"tools":["shell"],"scope_path":"/s","duration":"1h","granted_at":1.5,"expires_at":9.0,"granted_by":"taisun","enforced_allowlist":["shell"],"pending_request":null}"#,
        )
        .unwrap();
        assert!(g.enabled && g.expires_at == Some(9.0));
        let a: CrewAuditRow = serde_json::from_str(
            r#"{"id":3,"at":1.0,"actor":"taisun","kind":"grant","text":"x","target":null,"exit_code":null,"duration_ms":null,"checkpoint":null,"run_id":null}"#,
        )
        .unwrap();
        assert_eq!(a.kind, "grant");
    }

    #[test]
    fn url_is_loopback_and_the_http_error_never_echoes_a_body() {
        assert_eq!(
            url("Research", "grant"),
            "http://127.0.0.1:8400/crews/tools/Research/grant"
        );
        assert_eq!(
            http_error("grant", reqwest::StatusCode::INTERNAL_SERVER_ERROR),
            "Crew grant failed (HTTP 500)."
        );
    }
}
