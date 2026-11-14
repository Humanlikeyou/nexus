//! The persisted "act" setting: whether the app may start services on launch, restart ones that stop
//! and stop the ones it started on quit. Effective only when BOTH the compile-time
//! `SUPERVISOR_ACT_ENABLED` and this setting are true, so the first ship (const false) acts on nothing
//! whatever the file says.
//!
//! Same tamper rule as `outbound.rs`: `{"enabled":true}` alone is not enough; the file carries an
//! HMAC-SHA256 tag under the per-device Keychain key (domain-separated by its own message), so a
//! hand-edited or copied file reads as off. Enabling always asks the native approval; disabling never
//! asks. The file is left out of backups and never written by a restore.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::commands::approval_gate;
use crate::commands::keychain::{self, Keychain};
use crate::commands::outbound::{ct_eq, device_key, hmac_sha256};

use super::runner::SUPERVISOR_ACT_ENABLED;

pub(crate) const FILE_NAME: &str = "supervisor_act.json";
const TAG_MESSAGE: &[u8] = b"amara-supervisor-act:v1:enabled=true";

static ACT_SETTING: AtomicBool = AtomicBool::new(false);

fn device_tag(key: &[u8]) -> String {
    hex::encode(hmac_sha256(key, TAG_MESSAGE))
}

/// The saved choice only.
pub fn setting_on() -> bool {
    ACT_SETTING.load(Ordering::SeqCst)
}

/// The pure rule: both the build switch and the saved setting.
pub(crate) fn effective_with(build: bool, setting: bool) -> bool {
    build && setting
}

/// Whether the supervisor may act right now.
pub fn effective() -> bool {
    effective_with(SUPERVISOR_ACT_ENABLED, setting_on())
}

fn setting_path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

const FILE_CAP: u64 = 4096;

/// One open (no symlink followed, no FIFO block), regular files only, at most `FILE_CAP` bytes.
fn read_capped(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let meta = f.metadata().ok()?;
    if !meta.is_file() || meta.len() > FILE_CAP {
        return None;
    }
    let mut buf = Vec::new();
    f.take(FILE_CAP).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// True only for `{"enabled": true, "tag": <valid device tag>}`; everything else reads as off.
pub(crate) fn load_in(dir: &Path, kc: &dyn Keychain) -> bool {
    let Some(v) = read_capped(&setting_path(dir))
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
    else {
        return false;
    };
    if v.get("enabled").and_then(serde_json::Value::as_bool) != Some(true) {
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

fn store_in(dir: &Path, enabled: bool, kc: &dyn Keychain) -> Result<(), String> {
    let path = setting_path(dir);
    let tmp = dir.join(format!("{FILE_NAME}.tmp"));
    let body = if enabled {
        let key = device_key(kc, true).ok_or("Could not save: Keychain unavailable")?;
        serde_json::json!({ "enabled": true, "tag": device_tag(&key) }).to_string()
    } else {
        serde_json::json!({ "enabled": false }).to_string()
    };
    crate::app_paths::write_private(&tmp, body).map_err(|e| format!("Could not save: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not save: {e}")
    })
}

/// Persist `enabled`. Turning it ON always awaits `approve` (the native dialog); an `Err` from it
/// is returned and nothing is written. Turning it OFF never asks.
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

/// App start: load the saved setting. An absent or untagged file is off.
pub fn init() {
    #[cfg(not(test))]
    ACT_SETTING.store(
        load_in(&crate::app_paths::data_dir(), &keychain::MacKeychain),
        Ordering::SeqCst,
    );
}

/// The command body: native approval to enable, then persist, then update the live flag.
pub(crate) async fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    let dir = crate::app_paths::data_dir();
    let approve = || async {
        let text = approval_gate::approval_text(
            "Allow Amara OS to manage its services",
            "This lets Amara OS start the services it needs when it opens, restart ones that stop, \
             and stop the ones it started when you quit. Services that were already running are \
             never stopped. It only takes effect in a build where this is switched on. You can turn \
             it off again here.",
        )?;
        approval_gate::require_approval(
            app,
            &[approval_gate::service_key("supervisor-act-setting")],
            text,
        )
        .await
        .map(|_guard| ())
    };
    let saved = set_in(&dir, enabled, &keychain::MacKeychain, approve).await?;
    ACT_SETTING.store(saved, Ordering::SeqCst);
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::keychain::FakeKeychain;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sup-act-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn effective_needs_the_build_switch_and_the_setting() {
        assert!(!effective_with(false, false));
        assert!(!effective_with(false, true));
        assert!(!effective_with(true, false));
        assert!(effective_with(true, true));
        // In this build the const is off, so no setting can make it effective.
        assert!(!effective());
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
        let off = set_in(&d, false, &kc, || async {
            Err::<(), String>("approval must not be asked to disable".into())
        })
        .await
        .unwrap();
        assert!(!off);
        assert!(!load_in(&d, &kc));
    }

    #[tokio::test]
    async fn untagged_foreign_or_malformed_files_read_off() {
        let d = tmp("tamper");
        let kc = FakeKeychain::default();
        device_key(&kc, true).unwrap();
        for body in [
            r#"{"enabled":true}"#,
            r#"{"enabled":true,"tag":"00"}"#,
            r#"{"enabled":"yes"}"#,
            "not json",
            "",
        ] {
            std::fs::write(setting_path(&d), body).unwrap();
            assert!(!load_in(&d, &kc), "{body:?}");
        }
        // A file tagged by another device's key is off here; a device with no key is off too.
        let other = FakeKeychain::default();
        set_in(&d, true, &other, || async { Ok(()) }).await.unwrap();
        assert!(!load_in(&d, &kc));
        let empty = FakeKeychain::default();
        assert!(!load_in(&d, &empty));
        assert!(device_key(&empty, false).is_none());
    }

    #[tokio::test]
    async fn a_linked_or_oversize_file_reads_off() {
        let d = tmp("cap");
        let kc = FakeKeychain::default();
        set_in(&d, true, &kc, || async { Ok(()) }).await.unwrap();
        assert!(load_in(&d, &kc));
        // Same valid body behind a symlink: off.
        let real = d.join("real.json");
        std::fs::rename(setting_path(&d), &real).unwrap();
        std::os::unix::fs::symlink(&real, setting_path(&d)).unwrap();
        assert!(!load_in(&d, &kc));
        // Valid JSON padded past the cap: off.
        std::fs::remove_file(setting_path(&d)).unwrap();
        let body = std::fs::read_to_string(&real).unwrap();
        let padded = format!("{body}{}", " ".repeat(FILE_CAP as usize));
        std::fs::write(setting_path(&d), padded).unwrap();
        assert!(!load_in(&d, &kc));
        std::fs::write(setting_path(&d), body).unwrap();
        assert!(load_in(&d, &kc));
    }

    #[test]
    fn the_file_is_never_backed_up_or_restored() {
        assert!(crate::commands::backups::is_excluded_name(FILE_NAME, 0));
        assert!(crate::commands::backups::is_excluded_name(FILE_NAME, 2));
        assert!(crate::commands::packager::RESTORE_NEVER.contains(&FILE_NAME));
        let rel: Vec<String> = ["nexus-config", FILE_NAME].map(String::from).to_vec();
        assert!(crate::commands::packager::restore_target(&rel).is_none());
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
    fn the_command_asks_through_the_native_gate() {
        let src = include_str!("act.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("approval_gate::require_approval"));
    }
}
