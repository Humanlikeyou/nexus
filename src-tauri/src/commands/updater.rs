//! Auto-update (handover Phase 15). Built and INERT: nothing here can reach the network until
//! Taisun supplies the updater public key and a manifest host (gate, decision pending).
//!
//! Fail-closed switches, all must hold before any check runs (`resolve`):
//! 1. `UPDATER_SWITCH` is true (compile-time, false today);
//! 2. `plugins.updater.pubkey` in tauri.conf.json is a real minisign public key, not
//!    `PLACEHOLDER_NOT_CONFIGURED`;
//! 3. `plugins.updater.endpoints` is non-empty, every URL is https and they share one host (the
//!    only outbound host an update may use);
//! 4. outbound is allowed (`NEXUS_OUTBOUND_DISABLED` / Local only / Settings still win).
//!
//! `tauri_plugin_updater` is registered from `setup` only when 1-3 hold. The webview is granted no
//! updater permission; it can only call the commands below. `update_install_now` is not wired to a
//! real swap host yet (see `update_install.rs`, tested with a fake host): it refuses.
//!
//! Prefs `auto_update` and `check_daily` default to true (Taisun) and are stored, but have no
//! effect while unconfigured. No scheduler exists yet, so nothing checks in the background.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PUBKEY_PLACEHOLDER: &str = "PLACEHOLDER_NOT_CONFIGURED";
/// Default-off master switch. Flip only with Taisun's logged decision (docs/decisions/).
pub const UPDATER_SWITCH: bool = false;
const PREFS_FILE: &str = "update_prefs.json";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct UpdatePrefs {
    #[serde(default = "yes")]
    pub auto_update: bool,
    #[serde(default = "yes")]
    pub check_daily: bool,
}
fn yes() -> bool {
    true
}
impl Default for UpdatePrefs {
    fn default() -> Self {
        UpdatePrefs {
            auto_update: true,
            check_daily: true,
        }
    }
}

#[derive(Deserialize, Debug, Default, specta::Type)]
pub struct UpdatePrefsPatch {
    pub auto_update: Option<bool>,
    pub check_daily: Option<bool>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct UpdateStatus {
    /// `not_configured`, `outbound_disabled`, `idle` or `available`.
    pub state: String,
    pub configured: bool,
    pub current_version: String,
    pub auto_update: bool,
    pub check_daily: bool,
    /// The version the manifest ANNOUNCES. Not trusted: the manifest JSON is unsigned. It is
    /// authenticated only when the artifact's minisign signature is verified at download/install
    /// (the plugin and `update_install::run_install` both do that, and the signed trusted comment
    /// pins the version). Show it as "announced", never as proven.
    pub announced_version_unverified: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Resolved {
    pub pubkey: String,
    pub endpoints: Vec<String>,
    pub host: String,
    pub port: u16,
}

/// `(host, port)` of an https URL; None for any other scheme, a missing host or userinfo.
fn origin_of(url: &str) -> Option<(String, u16)> {
    let u = url::Url::parse(url).ok()?;
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    Some((
        u.host_str()?.to_ascii_lowercase(),
        u.port_or_known_default()?,
    ))
}

/// Pure decision: `Err(reason)` means not configured. The reason is for logs and tests only; the
/// command surface always says `not_configured`.
pub fn resolve(switch: bool, pubkey: &str, endpoints: &[String]) -> Result<Resolved, &'static str> {
    if !switch {
        return Err("switch off");
    }
    let pk = pubkey.trim();
    if pk.is_empty() || pk == PUBKEY_PLACEHOLDER {
        return Err("no public key");
    }
    // Must decode as a minisign key (the base64-of-text form Tauri uses).
    if !super::update_install::valid_pubkey(pk) {
        return Err("bad public key");
    }
    if endpoints.is_empty() {
        return Err("no endpoints");
    }
    let mut origin: Option<(String, u16)> = None;
    for e in endpoints {
        let o = origin_of(e).ok_or("endpoint must be https without credentials")?;
        match &origin {
            Some(x) if *x != o => return Err("endpoints span more than one host or port"),
            _ => origin = Some(o),
        }
    }
    let (host, port) = origin.unwrap();
    Ok(Resolved {
        pubkey: pk.into(),
        endpoints: endpoints.to_vec(),
        host,
        port,
    })
}

fn prefs_path(dir: &Path) -> PathBuf {
    dir.join(PREFS_FILE)
}

pub(crate) fn prefs_in(dir: &Path) -> UpdatePrefs {
    std::fs::read(prefs_path(dir))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub(crate) fn set_prefs_in(dir: &Path, patch: UpdatePrefsPatch) -> Result<UpdatePrefs, String> {
    let mut p = prefs_in(dir);
    if let Some(v) = patch.auto_update {
        p.auto_update = v;
    }
    if let Some(v) = patch.check_daily {
        p.check_daily = v;
    }
    let tmp = dir.join(format!("{PREFS_FILE}.tmp"));
    let body = serde_json::to_vec(&p).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&tmp, body).map_err(|e| format!("Could not save: {e}"))?;
    std::fs::rename(&tmp, prefs_path(dir)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not save: {e}")
    })?;
    Ok(p)
}

fn conf_values(config: &tauri::Config) -> (String, Vec<String>) {
    let v = config.plugins.0.get("updater");
    let pubkey = v
        .and_then(|v| v.get("pubkey"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let endpoints = v
        .and_then(|v| v.get("endpoints"))
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    (pubkey, endpoints)
}

pub fn resolve_from_config(config: &tauri::Config) -> Result<Resolved, &'static str> {
    let (pk, eps) = conf_values(config);
    resolve(UPDATER_SWITCH, &pk, &eps)
}

fn status_for(
    resolved: &Result<Resolved, &'static str>,
    outbound_blocked: bool,
    prefs: UpdatePrefs,
    current: &str,
    available: Option<String>,
) -> UpdateStatus {
    let configured = resolved.is_ok();
    let state = if !configured {
        "not_configured"
    } else if outbound_blocked {
        "outbound_disabled"
    } else if available.is_some() {
        "available"
    } else {
        "idle"
    };
    UpdateStatus {
        state: state.into(),
        configured,
        current_version: current.into(),
        auto_update: prefs.auto_update,
        check_daily: prefs.check_daily,
        announced_version_unverified: if configured { available } else { None },
    }
}

#[tauri::command]
#[specta::specta]
pub async fn update_status(app: tauri::AppHandle) -> Result<UpdateStatus, String> {
    let r = resolve_from_config(app.config());
    let prefs = prefs_in(&crate::app_paths::data_dir());
    Ok(status_for(
        &r,
        super::telegram::outbound_disabled(),
        prefs,
        &app.package_info().version.to_string(),
        None,
    ))
}

/// The download must come from the one pinned manifest origin (host and port, https).
pub(crate) fn download_allowed(r: &Resolved, download_url: &str) -> bool {
    origin_of(download_url).is_some_and(|(h, p)| h == r.host && p == r.port)
}

/// Fails closed with `not_configured` until the key, host and switch exist.
#[tauri::command]
#[specta::specta]
pub async fn update_check(app: tauri::AppHandle) -> Result<UpdateStatus, String> {
    use tauri_plugin_updater::UpdaterExt;
    let r = resolve_from_config(app.config());
    if r.is_err() {
        return Err("not_configured".into());
    }
    if super::telegram::outbound_disabled() {
        return Err("outbound_disabled".into());
    }
    // Registered from setup only when configured (lib.rs); the signature is verified by the plugin.
    // Redirects are disabled: a redirect could move the manifest or the download off the pinned host.
    let updater = app
        .updater_builder()
        .configure_client(|c| c.redirect(reqwest_updater::redirect::Policy::none()))
        .build()
        .map_err(|_| "not_configured".to_string())?;
    let found = updater
        .check()
        .await
        .map_err(|e| format!("check_failed: {e}"))?;
    if let (Some(u), Ok(res)) = (&found, &r) {
        if !download_allowed(res, u.download_url.as_str()) {
            return Err("download_host_not_allowed".into());
        }
    }
    let prefs = prefs_in(&crate::app_paths::data_dir());
    Ok(status_for(
        &r,
        false,
        prefs,
        &app.package_info().version.to_string(),
        found.map(|u| u.version),
    ))
}

/// Not wired to a real swap host: refuses. The install order is `update_install::run_install`.
#[tauri::command]
#[specta::specta]
pub async fn update_install_now(app: tauri::AppHandle) -> Result<(), String> {
    if resolve_from_config(app.config()).is_err() {
        return Err("not_configured".into());
    }
    Err("install_not_available".into())
}

#[tauri::command]
#[specta::specta]
pub async fn update_set_prefs(patch: UpdatePrefsPatch) -> Result<UpdatePrefs, String> {
    let dir = crate::app_paths::data_dir();
    tokio::task::spawn_blocking(move || set_prefs_in(&dir, patch))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn good_key() -> String {
        base64::engine::general_purpose::STANDARD.encode(
            "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3",
        )
    }
    fn eps(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("nexus-upd-{tag}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn shipped_config_is_the_placeholder_and_the_switch_is_off() {
        const { assert!(!UPDATER_SWITCH) };
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(conf["plugins"]["updater"]["pubkey"], PUBKEY_PLACEHOLDER);
        assert_eq!(
            conf["plugins"]["updater"]["endpoints"],
            serde_json::json!([])
        );
        assert_eq!(
            conf["bundle"]["createUpdaterArtifacts"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn placeholder_key_or_switch_off_means_not_configured() {
        let e = eps(&["https://updates.example.test/latest.json"]);
        assert!(resolve(true, PUBKEY_PLACEHOLDER, &e).is_err());
        assert!(resolve(true, "", &e).is_err());
        assert!(resolve(true, "garbage", &e).is_err());
        assert!(
            resolve(false, &good_key(), &e).is_err(),
            "switch off wins over a real key"
        );
        assert!(resolve(true, &good_key(), &e).is_ok());
    }

    #[test]
    fn endpoints_must_be_https_single_host_without_credentials() {
        let k = good_key();
        assert!(resolve(true, &k, &[]).is_err());
        assert!(resolve(true, &k, &eps(&["http://h.test/a"])).is_err());
        assert!(resolve(true, &k, &eps(&["https://u:p@h.test/a"])).is_err());
        assert!(resolve(true, &k, &eps(&["https://a.test/x", "https://b.test/x"])).is_err());
        let r = resolve(true, &k, &eps(&["https://A.test/x", "https://a.test/y"])).unwrap();
        assert_eq!((r.host.as_str(), r.port), ("a.test", 443));
        // port is part of the origin
        assert!(resolve(
            true,
            &k,
            &eps(&["https://a.test/x", "https://a.test:8443/x"])
        )
        .is_err());
        assert!(resolve(
            true,
            &k,
            &eps(&["https:///x", "ftp://a.test/x", "not a url"])
        )
        .is_err());
    }

    #[test]
    fn download_must_stay_on_the_pinned_origin() {
        let r = resolve(true, &good_key(), &eps(&["https://a.test/latest.json"])).unwrap();
        assert!(download_allowed(&r, "https://a.test/dl/Amara.tar.gz"));
        assert!(download_allowed(&r, "https://A.TEST:443/dl/x"));
        for bad in [
            "https://evil.test/x",
            "https://a.test.evil.test/x",
            "https://a.test:8443/x",
            "http://a.test/x",
            "https://u:p@a.test/x",
            "https://a.test@evil.test/x",
            "",
        ] {
            assert!(!download_allowed(&r, bad), "{bad}");
        }
    }

    #[test]
    fn unconfigured_status_is_inert_even_with_prefs_on() {
        let r = resolve(false, PUBKEY_PLACEHOLDER, &[]);
        let s = status_for(
            &r,
            false,
            UpdatePrefs::default(),
            "0.1.0",
            Some("9.9.9".into()),
        );
        assert_eq!(s.state, "not_configured");
        assert!(!s.configured && s.auto_update && s.check_daily);
        assert_eq!(
            s.announced_version_unverified, None,
            "no version without a verified check"
        );
    }

    #[test]
    fn outbound_disabled_is_reported_when_configured() {
        let r = resolve(true, &good_key(), &eps(&["https://h.test/l.json"]));
        assert_eq!(
            status_for(&r, true, UpdatePrefs::default(), "0.1.0", None).state,
            "outbound_disabled"
        );
        assert_eq!(
            status_for(&r, false, UpdatePrefs::default(), "0.1.0", None).state,
            "idle"
        );
    }

    #[test]
    fn prefs_default_to_on_and_persist() {
        let d = tmp("prefs");
        assert_eq!(
            prefs_in(&d),
            UpdatePrefs {
                auto_update: true,
                check_daily: true
            }
        );
        assert!(!prefs_path(&d).exists());
        set_prefs_in(
            &d,
            UpdatePrefsPatch {
                auto_update: Some(false),
                check_daily: None,
            },
        )
        .unwrap();
        assert_eq!(
            prefs_in(&d),
            UpdatePrefs {
                auto_update: false,
                check_daily: true
            }
        );
        std::fs::write(prefs_path(&d), "{oops").unwrap();
        assert_eq!(prefs_in(&d), UpdatePrefs::default());
    }

    #[test]
    fn updater_plugin_is_registered_only_when_configured() {
        let lib = include_str!("../lib.rs");
        assert!(lib.contains("updater::resolve_from_config"));
        let ls: Vec<&str> = lib.lines().map(str::trim).collect();
        let i = ls
            .iter()
            .position(|l| l.starts_with(".plugin(tauri_plugin_updater"))
            .unwrap();
        assert_eq!(
            ls[i - 1],
            "app.handle()",
            "only called on `app.handle()` ..."
        );
        assert!(
            ls[i - 2].contains("resolve_from_config"),
            "... inside the configured check"
        );
    }
}
