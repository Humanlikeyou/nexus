//! R13-F: a read-only n8n probe for the Crews health tile. GET only, loopback only. Connected comes
//! from n8n's unauthenticated /healthz. Workflow and failure counts need n8n's API key, which is
//! never in this repo: it is read from an optional 0600 file in the app data dir (`n8n_api_key`).
//! No key (or no answer) means those fields are `None` and the UI keeps them unwired. This module
//! never creates, changes, activates or deletes anything in n8n.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const N8N_URL: &str = "http://127.0.0.1:5678";
/// File under the app data dir holding an n8n API key (0600, regular file, not a symlink).
pub const N8N_KEY_FILE: &str = "n8n_api_key";
const KEY_HEADER: &str = "X-N8N-API-KEY";
const PAGE: u32 = 250;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, specta::Type)]
pub struct N8nStatus {
    pub connected: bool,
    /// Workflows in n8n; `None` when not obtainable read-only without a key.
    pub workflows: Option<u32>,
    /// Failed executions in the last 7 days; `None` when unknown.
    pub failures7d: Option<u32>,
}

/// `{"data":[...],"nextCursor":null}` -> number of items; `None` if the list is paged (count unknown).
pub(crate) fn parse_count(v: &Value) -> Option<u32> {
    let data = v.get("data")?.as_array()?;
    if v.get("nextCursor").is_some_and(|c| !c.is_null()) {
        return None;
    }
    u32::try_from(data.len()).ok()
}

/// Executions with `status=error` whose `startedAt` is within 7 days of `now`.
pub(crate) fn parse_failures(v: &Value, now: DateTime<Utc>) -> Option<u32> {
    let data = v.get("data")?.as_array()?;
    if v.get("nextCursor").is_some_and(|c| !c.is_null()) {
        return None;
    }
    let since = now - Duration::days(7);
    let n = data
        .iter()
        .filter(|e| {
            e.get("startedAt")
                .and_then(|s| s.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .is_some_and(|t| t.with_timezone(&Utc) >= since)
        })
        .count();
    u32::try_from(n).ok()
}

async fn get_json(client: &reqwest::Client, path: &str, key: &str) -> Option<Value> {
    let resp = client
        .get(format!("{N8N_URL}{path}"))
        .header(KEY_HEADER, key)
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<Value>().await.ok()
}

/// n8n's unauthenticated /healthz, GET only (also used by the system map).
pub(crate) async fn n8n_healthz(client: &reqwest::Client, timeout: std::time::Duration) -> bool {
    client
        .get(format!("{N8N_URL}/healthz"))
        .timeout(timeout)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Keychain `n8n_api_key` first, then the file (env ignored; the file must be 0600, regular,
/// non-empty: the crew token reader's checks).
fn n8n_api_key() -> Option<String> {
    super::keychain_migrate::with_live_keychain(|kc| {
        n8n_api_key_with(kc, &crate::app_paths::data_dir().join(N8N_KEY_FILE))
    })
}

pub(crate) fn n8n_api_key_with(
    kc: &dyn super::keychain::Keychain,
    file: &std::path::Path,
) -> Option<String> {
    super::keychain_migrate::secret_prefer_keychain(
        kc,
        "n8n_api_key",
        |v| Some(v.to_string()),
        || super::crew::crew_token_from(None, file).ok(),
    )
}

/// Never an error: every state is something the UI shows.
#[tauri::command]
#[specta::specta]
pub async fn n8n_status() -> Result<N8nStatus, String> {
    let client =
        super::loopback_http::loopback_client(super::loopback_http::LOOPBACK_CONNECT_TIMEOUT);
    let up = n8n_healthz(&client, std::time::Duration::from_secs(3)).await;
    if !up {
        return Ok(N8nStatus::default());
    }
    let key = n8n_api_key();
    let (mut workflows, mut failures7d) = (None, None);
    if let Some(key) = key {
        workflows = get_json(&client, &format!("/api/v1/workflows?limit={PAGE}"), &key)
            .await
            .and_then(|v| parse_count(&v));
        failures7d = get_json(
            &client,
            &format!("/api/v1/executions?status=error&limit={PAGE}"),
            &key,
        )
        .await
        .and_then(|v| parse_failures(&v, Utc::now()));
    }
    Ok(N8nStatus {
        connected: true,
        workflows,
        failures7d,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_a_single_page_and_gives_up_on_a_paged_list() {
        assert_eq!(
            parse_count(&json!({"data":[{},{},{}],"nextCursor":null})),
            Some(3)
        );
        assert_eq!(parse_count(&json!({"data":[]})), Some(0));
        assert_eq!(parse_count(&json!({"data":[{}],"nextCursor":"abc"})), None);
        assert_eq!(parse_count(&json!({"message":"unauthorized"})), None);
        assert_eq!(parse_count(&json!([1, 2])), None);
    }

    #[test]
    fn failures_count_only_the_last_seven_days() {
        let now = DateTime::parse_from_rfc3339("2026-11-11T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let v = json!({"data":[
            {"startedAt":"2026-11-11T11:00:00.000Z"},
            {"startedAt":"2026-11-05T12:00:01Z"},
            {"startedAt":"2026-11-04T11:59:59Z"},
            {"startedAt":"garbage"},
            {"id":1}
        ],"nextCursor":null});
        assert_eq!(parse_failures(&v, now), Some(2));
        assert_eq!(
            parse_failures(
                &json!({"data":[{"startedAt":"2026-11-11T11:00:00Z"}],"nextCursor":"x"}),
                now
            ),
            None
        );
    }

    #[test]
    fn status_serializes_unwired_fields_as_null() {
        let s = N8nStatus {
            connected: true,
            workflows: None,
            failures7d: None,
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            json!({"connected": true, "workflows": null, "failures7d": null})
        );
        assert!(!N8nStatus::default().connected);
    }

    #[test]
    fn only_get_requests_and_loopback_are_used() {
        let src = include_str!("n8n_status.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            !prod.contains(".post(")
                && !prod.contains(".put(")
                && !prod.contains(".delete(")
                && !prod.contains(".patch(")
        );
        assert!(N8N_URL.starts_with("http://127.0.0.1:"));
    }
}
