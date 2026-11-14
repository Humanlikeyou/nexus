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
    ("letta_server_password", "Letta memory password"),
    ("weaviate_api_key", "Knowledge base (Weaviate) key"),
    ("n8n_api_key", "n8n API key"),
    ("crew_service_token", "Crew service token"),
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

#[derive(Serialize, Debug, PartialEq, Eq, specta::Type)]
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
#[specta::specta]
pub async fn secrets_status() -> Result<Vec<SecretStatus>, String> {
    tokio::task::spawn_blocking(|| status_with(&MacKeychain))
        .await
        .map_err(|e| e.to_string())?
}

/// Replacing a set secret always asks; so does the first set of a `telegram_*` secret (it decides
/// where health alerts are sent). Other first sets don't.
pub(crate) fn needs_approval(name: &str, already_set: bool) -> bool {
    already_set
        || name.starts_with("telegram_")
        || super::keychain_migrate::FILE_BACKED
            .iter()
            .any(|(n, _)| *n == name)
        || name == "weaviate_api_key"
}

/// Per-name format check for `secret_set`. File-backed credentials are Keychain-first now, so a
/// malformed value must never be stored: `ha_token` uses the `.env` writer's strict format, the
/// other service credentials must be printable ASCII with no whitespace (header-safe).
pub(crate) fn validate_for(name: &str, value: &str) -> Result<(), String> {
    let ok = match name {
        "ha_token" => super::ha::valid_new_token(value),
        "n8n_api_key" | "crew_service_token" | "weaviate_api_key" => {
            !value.is_empty() && value.bytes().all(|b| b.is_ascii_graphic())
        }
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err("value has an invalid format".to_string())
    }
}

/// Last 4 characters, for a dialog. Never more.
pub(crate) fn tail4(s: &str) -> String {
    let t = s.trim();
    let n = t.chars().count();
    t.chars().skip(n.saturating_sub(4)).collect()
}

/// What the approval dialog says about the Telegram chat being saved.
pub(crate) fn chat_dialog_detail(old_tail: Option<&str>, new_tail: &str) -> String {
    match old_tail {
        Some(o) => format!("Replaces the chat ending in {o} with the chat ending in {new_tail}. Health alerts will go to the new chat. Allow only if you just asked for this."),
        None => format!("Health alerts will go to the chat ending in {new_tail}. Allow only if you just asked for this."),
    }
}

/// Write-only. Returns nothing; the value is zeroized after the Keychain write.
#[tauri::command]
#[specta::specta]
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
    if let Err(e) = validate_for(&name, &value) {
        value.zeroize();
        return Err(e);
    }
    let n = name.clone();
    let existing = tokio::task::spawn_blocking(move || {
        status_with(&MacKeychain).map(|all| all.into_iter().find(|s| s.name == n))
    })
    .await
    .map_err(|e| e.to_string())?;
    let _held = match existing {
        Ok(Some(s)) if needs_approval(&name, s.set) => {
            let (op, detail) = if s.set {
                (
                    format!("Replace the saved {}", s.label),
                    "The value stored in the Keychain is overwritten. Allow only if you just asked for this.",
                )
            } else if !name.starts_with("telegram_") {
                (
                    format!("Save the {}", s.label),
                    "It will be used instead of any copy in a file. Allow only if you just asked for this.",
                )
            } else {
                (
                    format!("Save the {}", s.label),
                    "It is stored in the Keychain and used only to send your health alerts to Telegram. Allow only if you just asked for this.",
                )
            };
            let mut detail = detail.to_string();
            if name == "telegram_chat_id" {
                // Show which chat is being replaced and which is arriving: last 4 characters only.
                let old = if s.set {
                    tokio::task::spawn_blocking(|| {
                        MacKeychain
                            .get(&service_for("telegram_chat_id"))
                            .ok()
                            .map(|v| tail4(&String::from_utf8_lossy(&Zeroizing::new(v))))
                    })
                    .await
                    .ok()
                    .flatten()
                } else {
                    None
                };
                detail = chat_dialog_detail(old.as_deref(), &tail4(&value));
            }
            let text = super::approval_gate::approval_text(&op, &detail);
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
    fn telegram_first_set_needs_approval_others_do_not() {
        assert!(needs_approval("telegram_bot_token", false));
        assert!(needs_approval("telegram_chat_id", false));
        assert!(needs_approval("ha_token", true));
        // First sets of the Keychain-first service credentials ask too (they shadow the file copy).
        for n in [
            "ha_token",
            "n8n_api_key",
            "crew_service_token",
            "weaviate_api_key",
        ] {
            assert!(needs_approval(n, false), "{n}");
        }
        assert!(!needs_approval("n8n_encryption_key", false));
        // every telegram_* name in the allowlist is covered
        assert!(SECRETS
            .iter()
            .filter(|(n, _)| n.starts_with("telegram_"))
            .all(|(n, _)| needs_approval(n, false)));
    }

    #[test]
    fn chat_dialog_shows_last_four_of_old_and_new_only() {
        assert_eq!(tail4("SENTINEL-chat-5521"), "5521");
        assert_eq!(tail4("12"), "12");
        let d = chat_dialog_detail(Some("5521"), "9908");
        assert!(d.contains("5521") && d.contains("9908") && !d.contains("SENTINEL"));
        assert!(chat_dialog_detail(None, "9908").contains("ending in 9908"));
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
