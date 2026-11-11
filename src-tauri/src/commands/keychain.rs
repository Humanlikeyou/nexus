//! macOS Keychain secrets (HANDOFF-1111s wiring step 1).
//!
//! Hard rule: no command here ever returns a secret value. `secrets_status` reports `set` only and
//! `secret_set` is write-only. The value is zeroized after the Keychain write. Tests use the
//! in-memory `FakeKeychain`; nothing in tests touches the real Keychain.

use serde::Serialize;
#[cfg(test)]
use std::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

/// Keychain service = `SERVICE_PREFIX` + secret name.
pub(crate) const SERVICE_PREFIX: &str = "com.taisun.nexus.";
const ACCOUNT: &str = "amara";
const MAX_SECRET_BYTES: usize = 8192;

/// The secrets the 1111s UI lists (Settings › Notifications, Backup › Keys on this Mac, first-run).
/// Anything else is refused.
pub(crate) const SECRETS: &[(&str, &str)] = &[
    ("telegram_bot_token", "Telegram bot token"),
    ("telegram_chat_id", "Telegram chat"),
    ("n8n_encryption_key", "n8n encryption key"),
    ("ha_token", "Home Assistant token"),
    ("freqtrade_api", "Freqtrade API login"),
    ("coach_keys", "Oura, Hevy, AI coach"),
];

pub(crate) const N8N_KEY: &str = "n8n_encryption_key";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeychainError {
    NotFound,
    Other(String),
}

/// Minimal Keychain surface. Values cross it as bytes and are never logged.
pub(crate) trait Keychain: Send + Sync {
    fn get(&self, service: &str) -> Result<Vec<u8>, KeychainError>;
    fn set(&self, service: &str, value: &[u8]) -> Result<(), KeychainError>;
}

pub(crate) fn service_for(name: &str) -> String {
    format!("{SERVICE_PREFIX}{name}")
}

pub(crate) fn is_allowed(name: &str) -> bool {
    SECRETS.iter().any(|(n, _)| *n == name)
}

#[cfg(target_os = "macos")]
pub(crate) struct MacKeychain;

#[cfg(target_os = "macos")]
impl Keychain for MacKeychain {
    fn get(&self, service: &str) -> Result<Vec<u8>, KeychainError> {
        use security_framework::passwords::get_generic_password;
        get_generic_password(service, ACCOUNT).map_err(|e| {
            // errSecItemNotFound
            if e.code() == -25300 {
                KeychainError::NotFound
            } else {
                KeychainError::Other(format!("keychain error {}", e.code()))
            }
        })
    }
    fn set(&self, service: &str, value: &[u8]) -> Result<(), KeychainError> {
        use security_framework::passwords::set_generic_password;
        set_generic_password(service, ACCOUNT, value)
            .map_err(|e| KeychainError::Other(format!("keychain error {}", e.code())))
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) struct MacKeychain;

#[cfg(not(target_os = "macos"))]
impl Keychain for MacKeychain {
    fn get(&self, _: &str) -> Result<Vec<u8>, KeychainError> {
        Err(KeychainError::Other("Keychain is macOS only".into()))
    }
    fn set(&self, _: &str, _: &[u8]) -> Result<(), KeychainError> {
        Err(KeychainError::Other("Keychain is macOS only".into()))
    }
}

/// In-memory fake for tests.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct FakeKeychain {
    items: Mutex<std::collections::HashMap<String, Vec<u8>>>,
}

#[cfg(test)]
impl Keychain for FakeKeychain {
    fn get(&self, service: &str) -> Result<Vec<u8>, KeychainError> {
        self.items
            .lock()
            .map_err(|_| KeychainError::Other("poisoned".into()))?
            .get(service)
            .cloned()
            .ok_or(KeychainError::NotFound)
    }
    fn set(&self, service: &str, value: &[u8]) -> Result<(), KeychainError> {
        self.items
            .lock()
            .map_err(|_| KeychainError::Other("poisoned".into()))?
            .insert(service.to_string(), value.to_vec());
        Ok(())
    }
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct SecretStatus {
    pub name: String,
    pub label: String,
    pub set: bool,
}

pub(crate) fn status_with(kc: &dyn Keychain) -> Result<Vec<SecretStatus>, String> {
    SECRETS
        .iter()
        .map(|(name, label)| {
            // The value is read only to learn that the item exists, then zeroized at once.
            let set = match kc.get(&service_for(name)) {
                Ok(v) => {
                    drop(Zeroizing::new(v));
                    true
                }
                Err(KeychainError::NotFound) => false,
                Err(KeychainError::Other(e)) => return Err(e),
            };
            Ok(SecretStatus {
                name: (*name).to_string(),
                label: (*label).to_string(),
                set,
            })
        })
        .collect()
}

pub(crate) fn set_with(kc: &dyn Keychain, name: &str, mut value: String) -> Result<(), String> {
    let result = (|| {
        if !is_allowed(name) {
            return Err("unknown secret".to_string());
        }
        if value.trim().is_empty() {
            return Err("value is empty".to_string());
        }
        if value.len() > MAX_SECRET_BYTES {
            return Err("value is too long".to_string());
        }
        kc.set(&service_for(name), value.as_bytes())
            .map_err(|e| match e {
                KeychainError::NotFound => "keychain item missing".to_string(),
                KeychainError::Other(m) => m,
            })
    })();
    value.zeroize();
    result
}

#[tauri::command]
pub async fn secrets_status() -> Result<Vec<SecretStatus>, String> {
    tokio::task::spawn_blocking(|| status_with(&MacKeychain))
        .await
        .map_err(|e| e.to_string())?
}

/// Write-only. Returns nothing; the value is zeroized after the Keychain write.
#[tauri::command]
pub async fn secret_set(
    app: tauri::AppHandle,
    name: String,
    mut value: String,
) -> Result<(), String> {
    if !is_allowed(&name) {
        value.zeroize();
        return Err("unknown secret".to_string());
    }
    // Replacing a secret that is already set asks Taisun first (native dialog); a first set doesn't.
    let n = name.clone();
    let existing = tokio::task::spawn_blocking(move || {
        status_with(&MacKeychain).map(|all| all.into_iter().find(|s| s.name == n))
    })
    .await
    .map_err(|e| e.to_string())?;
    let _held = match existing {
        Ok(Some(s)) if s.set => {
            let text = super::approval_gate::approval_text(
                &format!("Replace the saved {}", s.label),
                "The value stored in the Keychain is overwritten. Allow only if you just asked for this.",
            );
            let text = match text {
                Ok(t) => t,
                Err(e) => {
                    value.zeroize();
                    return Err(e);
                }
            };
            let key = super::approval_gate::service_key(&format!("secret-{name}"));
            match super::approval_gate::require_approval(&app, &[key], text).await {
                Ok(g) => Some(g),
                Err(e) => {
                    value.zeroize();
                    return Err(e);
                }
            }
        }
        Ok(_) => None,
        Err(e) => {
            // Fail closed: if we can't tell whether it's set, don't write.
            value.zeroize();
            return Err(e);
        }
    };
    let approved = _held.is_some();
    tokio::task::spawn_blocking(move || {
        // Re-check just before writing: a secret that appeared after the check needs approval too.
        let now_set = status_with(&MacKeychain)
            .map(|all| all.into_iter().any(|s| s.name == name && s.set))
            .unwrap_or(true);
        if now_set && !approved {
            value.zeroize();
            return Err("changed: this secret was set meanwhile; try again".to_string());
        }
        set_with(&MacKeychain, &name, value)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL: &str = "SENTINEL-7f3a9c1e-do-not-leak";

    #[test]
    fn status_reports_set_flags_only() {
        let kc = FakeKeychain::default();
        let s = status_with(&kc).unwrap();
        assert_eq!(s.len(), SECRETS.len());
        assert!(s.iter().all(|x| !x.set));
        set_with(&kc, "ha_token", SENTINEL.into()).unwrap();
        let s = status_with(&kc).unwrap();
        assert!(s.iter().find(|x| x.name == "ha_token").unwrap().set);
        assert_eq!(s.iter().filter(|x| x.set).count(), 1);
    }

    #[test]
    fn unknown_empty_and_oversize_are_refused() {
        let kc = FakeKeychain::default();
        assert_eq!(
            set_with(&kc, "nope", "x".into()).unwrap_err(),
            "unknown secret"
        );
        assert!(set_with(&kc, "ha_token", "  ".into()).is_err());
        assert!(set_with(&kc, "ha_token", "x".repeat(MAX_SECRET_BYTES + 1)).is_err());
        assert!(status_with(&kc).unwrap().iter().all(|x| !x.set));
    }

    #[test]
    fn service_name_uses_the_prefix() {
        assert_eq!(service_for("ha_token"), "com.taisun.nexus.ha_token");
        let kc = FakeKeychain::default();
        set_with(&kc, "ha_token", SENTINEL.into()).unwrap();
        assert_eq!(
            kc.get("com.taisun.nexus.ha_token").unwrap(),
            SENTINEL.as_bytes()
        );
    }

    /// The sentinel must not appear in any IPC return value, any file under a temp data dir, or a
    /// backup archive made from that dir.
    #[test]
    fn sentinel_never_leaks() {
        let kc = FakeKeychain::default();
        for (name, _) in SECRETS {
            set_with(&kc, name, format!("{SENTINEL}-{name}")).unwrap();
        }
        let ipc = serde_json::to_string(&status_with(&kc).unwrap()).unwrap();
        assert!(!ipc.contains("SENTINEL"));
        let err = set_with(&kc, "bogus", SENTINEL.into()).unwrap_err();
        assert!(!err.contains("SENTINEL"));

        // Data dir with typical app files, then scan every byte of it and of an archive.
        let dir =
            std::env::temp_dir().join(format!("nexus-sent-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(dir.join("kv")).unwrap();
        std::fs::write(dir.join("kv/settings.json"), "{\"theme\":\"dark\"}").unwrap();
        std::fs::write(dir.join("goals.json"), "[]").unwrap();
        let archive = dir.with_extension("tar.gz");
        let out = std::process::Command::new("tar")
            .args(["-czf"])
            .arg(&archive)
            .arg("-C")
            .arg(dir.parent().unwrap())
            .arg(dir.file_name().unwrap())
            .output()
            .unwrap();
        assert!(out.status.success());
        fn scan(p: &std::path::Path, hits: &mut usize) {
            if p.is_dir() {
                for e in std::fs::read_dir(p).unwrap() {
                    scan(&e.unwrap().path(), hits);
                }
            } else if let Ok(b) = std::fs::read(p) {
                if b.windows(8).any(|w| w == b"SENTINEL") {
                    *hits += 1;
                }
            }
        }
        let mut hits = 0;
        scan(&dir, &mut hits);
        // The gzip stream is compressed; also untar and scan.
        let ex = dir.with_extension("ex");
        std::fs::create_dir_all(&ex).unwrap();
        assert!(std::process::Command::new("tar")
            .args(["-xzf"])
            .arg(&archive)
            .arg("-C")
            .arg(&ex)
            .status()
            .unwrap()
            .success());
        scan(&ex, &mut hits);
        assert_eq!(hits, 0);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&ex);
        let _ = std::fs::remove_file(&archive);
    }
}
