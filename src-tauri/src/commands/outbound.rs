//! Settings default for the outbound kill switch (handover Phase 7 step 4).
//!
//! Outbound (Telegram sends, outbox delivery, research, the Claude tier, notify relay, ...) is
//! DISABLED unless Settings explicitly enables it. The decision is made by
//! `telegram::outbound_disabled()`: navy local-only forces disabled, `NEXUS_OUTBOUND_DISABLED=1`
//! (shell or LaunchAgent) forces disabled and cannot be overridden from Settings, and otherwise
//! the saved setting decides. Nothing is baked into `Info.plist` any more.
//!
//! Enabling sends data off this Mac, so it goes through the native approval dialog
//! (`approval_gate`). Disabling never asks. The setting lives in `<data_dir>/outbound_setting.json`
//! (0600, atomic write). A missing, unreadable or malformed file means disabled.
//!
//! Tamper resistance: `{"allowed":true}` alone is NOT enough. The file carries a `tag`, an
//! HMAC-SHA256 under a random per-device key kept in the Keychain (`DEVICE_KEY_NAME`; deliberately
//! not in `keychain::SECRETS`, so Settings never lists it and `secret_set` refuses it). A file
//! without a valid tag (hand-edited, copied from another Mac) reads as disabled. The file is also
//! left out of backups and never written by a restore (`backups::EXCLUDED_SECRET_FILES`,
//! `packager::RESTORE_NEVER`). Enabling always asks the native approval.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::approval_gate;
use super::keychain::{self, Keychain, KeychainError};
use sha2::{Digest, Sha256};

pub(crate) const FILE_NAME: &str = "outbound_setting.json";
/// Keychain item (service = `keychain::service_for(..)`) holding the per-device tag key.
const DEVICE_KEY_NAME: &str = "outbound_device_key";
const TAG_MESSAGE: &[u8] = b"amara-outbound-setting:v1:allowed=true";

pub(crate) fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn device_tag(key: &[u8]) -> String {
    hex::encode(hmac_sha256(key, TAG_MESSAGE))
}

/// The device key; created (32 random bytes) only when `create` and none exists.
pub(crate) fn device_key(kc: &dyn Keychain, create: bool) -> Option<Vec<u8>> {
    let svc = keychain::service_for(DEVICE_KEY_NAME);
    match kc.get(&svc) {
        Ok(k) if k.len() >= 16 => Some(k),
        Err(KeychainError::NotFound) if create => {
            let mut k = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut k);
            kc.set(&svc, &k).ok()?;
            Some(k.to_vec())
        }
        _ => None,
    }
}

/// Loaded once at startup (`init_outbound_setting`), updated by `outbound_set`. Default false.
static OUTBOUND_ALLOWED: AtomicBool = AtomicBool::new(false);

pub fn outbound_allowed() -> bool {
    OUTBOUND_ALLOWED.load(Ordering::SeqCst)
}

/// The decision rule as a pure function, so the contract is testable without env or globals.
/// `telegram::outbound_disabled()` spells the same three terms inline (its text is pinned by the
/// navy test in home_windows.rs); `outbound_disabled_is_the_same_rule` keeps the two together.
#[cfg(test)]
pub(crate) fn compute_outbound_disabled(
    local_only: bool,
    env_value: Option<&str>,
    allowed_in_settings: bool,
) -> bool {
    local_only || super::telegram::parse_outbound_disabled(env_value) || !allowed_in_settings
}

fn setting_path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// Saved value; true only for `{"allowed": true, "tag": <valid device tag>}`. Everything else
/// (missing, malformed, no or wrong tag, no device key) reads as false (fail closed).
pub(crate) fn load_in(dir: &Path, kc: &dyn Keychain) -> bool {
    let Some(v) = std::fs::read(setting_path(dir))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    else {
        return false;
    };
    if v.get("allowed").and_then(serde_json::Value::as_bool) != Some(true) {
        return false;
    }
    let (Some(tag), Some(key)) = (
        v.get("tag").and_then(serde_json::Value::as_str),
        device_key(kc, false),
    ) else {
        return false;
    };
    ct_eq(tag.as_bytes(), device_tag(&key).as_bytes())
}

fn store_in(dir: &Path, allowed: bool, kc: &dyn Keychain) -> Result<(), String> {
    let path = setting_path(dir);
    let tmp = dir.join(format!("{FILE_NAME}.tmp"));
    let body = if allowed {
        let key = device_key(kc, true).ok_or("Could not save: Keychain unavailable")?;
        serde_json::json!({ "allowed": true, "tag": device_tag(&key) }).to_string()
    } else {
        serde_json::json!({ "allowed": false }).to_string()
    };
    crate::app_paths::write_private(&tmp, body).map_err(|e| format!("Could not save: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not save: {e}")
    })
}

/// Persist `enabled`. Turning outbound ON ALWAYS awaits `approve` (the native dialog), whatever
/// the file currently says; an `Err` from it (declined, busy, ...) is returned and nothing is
/// written. Turning it OFF never asks.
pub(crate) async fn set_in<F, Fut>(
    dir: &Path,
    enabled: bool,
    kc: &dyn Keychain,
    approve: F,
) -> Result<bool, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    if enabled {
        approve().await?;
    }
    store_in(dir, enabled, kc)?;
    Ok(enabled)
}

/// App start: load the saved setting. Absent file = disabled.
pub fn init_outbound_setting() {
    #[cfg(not(test))]
    OUTBOUND_ALLOWED.store(
        load_in(&crate::app_paths::data_dir(), &keychain::MacKeychain),
        Ordering::SeqCst,
    );
}

/// Whether Settings has outbound enabled. (This is the saved choice only: the env var and navy
/// mode can still force outbound off; `telegram::outbound_disabled()` is the effective answer.)
#[tauri::command]
#[specta::specta]
pub fn outbound_get() -> bool {
    outbound_allowed()
}

/// Settings toggle. Enabling asks through the native approval dialog because it lets data leave
/// this Mac; disabling is immediate. Returns the saved value.
#[tauri::command]
#[specta::specta]
pub async fn outbound_set(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    let dir = crate::app_paths::data_dir();
    let approve = || async {
        let text = approval_gate::approval_text(
            "Allow Amara OS to send data off this Mac",
            "This turns on outbound sends: Telegram messages, outbox delivery, web research and \
             the Claude tier (when also allowed). You can turn it off again in Settings. \
             NEXUS_OUTBOUND_DISABLED=1 and Local only still override this.",
        )?;
        approval_gate::require_approval(
            &app,
            &[approval_gate::service_key("outbound-setting")],
            text,
        )
        .await
        .map(|_guard| ())
    };
    let saved = set_in(&dir, enabled, &keychain::MacKeychain, approve).await?;
    OUTBOUND_ALLOWED.store(saved, Ordering::SeqCst);
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::keychain::FakeKeychain;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("outbound-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn default_is_disabled_with_no_env_and_no_setting() {
        assert!(compute_outbound_disabled(false, None, false));
        // The real function: the process default for the saved choice is off, so it is disabled
        // whatever the env holds (no env access here).
        assert!(!OUTBOUND_ALLOWED.load(Ordering::SeqCst));
        assert!(crate::commands::telegram::outbound_disabled());
        assert!(!load_in(&tmp("missing"), &FakeKeychain::default()));
    }

    #[test]
    fn settings_enable_turns_outbound_on_only_when_nothing_overrides() {
        assert!(!compute_outbound_disabled(false, None, true));
        assert!(!compute_outbound_disabled(false, Some("0"), true));
        assert!(!compute_outbound_disabled(false, Some(""), true));
    }

    #[test]
    fn env_forces_disabled_even_if_settings_enable() {
        for v in ["1", "true", "YES", " on "] {
            assert!(compute_outbound_disabled(false, Some(v), true), "{v}");
        }
    }

    #[test]
    fn navy_forces_disabled_even_if_settings_enable() {
        assert!(compute_outbound_disabled(true, None, true));
        assert!(compute_outbound_disabled(true, Some("0"), true));
    }

    #[test]
    fn malformed_or_wrong_typed_file_reads_disabled() {
        let d = tmp("bad");
        let kc = FakeKeychain::default();
        for body in [
            "",
            "not json",
            "{}",
            r#"{"allowed":"true"}"#,
            r#"{"allowed":1}"#,
        ] {
            std::fs::write(setting_path(&d), body).unwrap();
            assert!(!load_in(&d, &kc), "{body:?}");
        }
    }

    #[test]
    fn untagged_or_foreign_tagged_true_reads_disabled() {
        let d = tmp("tag");
        let kc = FakeKeychain::default();
        // The pre-fix file shape: no tag. Must NOT enable, with or without a device key.
        std::fs::write(setting_path(&d), r#"{"allowed":true}"#).unwrap();
        assert!(!load_in(&d, &kc));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(set_in(&d, true, &kc, || async { Ok(()) }))
            .unwrap();
        assert!(load_in(&d, &kc));
        // Same file on another device (different Keychain key) is disabled.
        let other = FakeKeychain::default();
        device_key(&other, true).unwrap();
        assert!(!load_in(&d, &other));
        // A device with no key at all is disabled, and loading never creates one.
        let empty = FakeKeychain::default();
        assert!(!load_in(&d, &empty));
        assert!(device_key(&empty, false).is_none());
        // Tampered tag.
        std::fs::write(setting_path(&d), r#"{"allowed":true,"tag":"00"}"#).unwrap();
        assert!(!load_in(&d, &kc));
    }

    #[tokio::test]
    async fn enabling_asks_even_when_the_file_already_reads_true() {
        let d = tmp("always-ask");
        let kc = FakeKeychain::default();
        set_in(&d, true, &kc, || async { Ok(()) }).await.unwrap();
        assert!(load_in(&d, &kc));
        let r = set_in(&d, true, &kc, || async {
            Err(approval_gate::denied_message())
        })
        .await;
        assert!(r.unwrap_err().starts_with(approval_gate::DECLINED_PREFIX));
    }

    #[test]
    fn hmac_matches_rfc4231_case_2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex::encode(mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn the_file_is_never_backed_up_or_restored() {
        assert!(crate::commands::backups::is_excluded_name(FILE_NAME, 0));
        assert!(crate::commands::backups::is_excluded_name(FILE_NAME, 2));
        assert!(crate::commands::packager::RESTORE_NEVER.contains(&FILE_NAME));
        let rel: Vec<String> = ["nexus-config", FILE_NAME].map(String::from).to_vec();
        assert!(crate::commands::packager::restore_target(&rel).is_none());
    }

    #[tokio::test]
    async fn enabling_needs_approval_and_decline_writes_nothing() {
        let d = tmp("decline");
        let kc = FakeKeychain::default();
        let r = set_in(&d, true, &kc, || async {
            Err(approval_gate::denied_message())
        })
        .await;
        assert!(r.unwrap_err().starts_with(approval_gate::DECLINED_PREFIX));
        assert!(!setting_path(&d).exists());
        assert!(!load_in(&d, &kc));
    }

    #[tokio::test]
    async fn approved_enable_persists_and_disable_never_asks() {
        let d = tmp("roundtrip");
        let kc = FakeKeychain::default();
        assert!(set_in(&d, true, &kc, || async { Ok(()) }).await.unwrap());
        assert!(load_in(&d, &kc));
        // Disabling must not call the approval closure.
        let off = set_in(&d, false, &kc, || async {
            Err::<(), String>("approval must not be asked to disable".into())
        })
        .await
        .unwrap();
        assert!(!off);
        assert!(!load_in(&d, &kc));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn setting_file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("mode");
        let kc = FakeKeychain::default();
        set_in(&d, true, &kc, || async { Ok(()) }).await.unwrap();
        let mode = std::fs::metadata(setting_path(&d))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn outbound_disabled_is_the_same_rule() {
        let tg = include_str!("telegram.rs");
        let f = &tg[tg.find("pub fn outbound_disabled()").unwrap()..];
        let f = &f[..f.find("\n}").unwrap()];
        assert!(f.contains("local_only_on()"));
        assert!(f.contains("parse_outbound_disabled(env.as_deref())"));
        assert!(f.contains("!allowed"));
    }

    #[test]
    fn the_command_asks_through_the_native_gate() {
        let src = include_str!("outbound.rs");
        let prod = &src[..src.find("#[cfg(test)]\nmod tests").unwrap()];
        assert!(prod.contains("approval_gate::require_approval"));
    }
}
