//! Keychain-first secret readers and a click-only file-to-Keychain copy tool (Phase 7 step 2).
//! [GATE per move: Taisun logs each key move in docs/decisions/ first]
//!
//! * `secret_prefer_keychain`: Keychain first, then the existing file reader. A missing, blank,
//!   non-UTF-8 or unreadable Keychain item falls through to the file.
//! * `keychain_import_review`: names, source (`file|keychain|both|none`) and whether the file's
//!   mode is acceptable, for the three file-backed names only. Never values, never lengths.
//!   `~/.hermes/.env` and `stack/.env` are never read here (compose and Hermes need them as files).
//! * `keychain_migrate_one`: refuses while `KEYCHAIN_MIGRATE_ENABLED` is false, and always needs the
//!   native approval dialog. It copies the file value into the Keychain, reads it back and compares
//!   in constant time. It never deletes or edits the file and never fills an item that already
//!   has a value. The result and every error carry no secret and no length.

use super::keychain::{service_for, Keychain, KeychainError};
use serde::Serialize;
use std::path::Path;
use zeroize::Zeroizing;

/// The gate. Flip only with Taisun's words logged in docs/decisions/.
pub const KEYCHAIN_MIGRATE_ENABLED: bool = false;
pub const DISABLED: &str = "disabled: moving keys into the Keychain is not enabled yet.";
const MAX_SECRET_BYTES: usize = 8192;
const NOT_MOVABLE: &str = "That key is not one the app can move.";
const ALREADY: &str = "The Keychain already holds that key; nothing was changed.";
const KEYCHAIN_UNREADABLE: &str = "The Keychain could not be checked; nothing was changed.";
const NO_FILE_VALUE: &str = "No usable key was found in the file; nothing was changed.";
const WRITE_FAILED: &str = "The Keychain write failed; nothing else was changed.";
const MISMATCH: &str = "The Keychain copy did not match the file; the file was not touched.";

/// File-backed names the app may move, with the file each lives in (under the data dir).
/// `ha_token` is the `HA_TOKEN` line of `.env`; the others are whole-file secrets.
pub(crate) const FILE_BACKED: &[(&str, &str)] = &[
    ("ha_token", ".env"),
    ("n8n_api_key", "n8n_api_key"),
    ("crew_service_token", "crew_service_token"),
];

fn is_file_backed(name: &str) -> bool {
    FILE_BACKED.iter().any(|(n, _)| *n == name)
}

/// The Keychain value for an allowlisted `name`, trimmed; `None` when absent, blank, unreadable
/// or not UTF-8.
pub(crate) fn keychain_value(kc: &dyn Keychain, name: &str) -> Option<Zeroizing<String>> {
    if !super::keychain::is_allowed(name) {
        return None;
    }
    let bytes: Zeroizing<Vec<u8>> = match kc.get(&service_for(name)) {
        Ok(v) => Zeroizing::new(v),
        Err(KeychainError::NotFound) | Err(KeychainError::Other(_)) => return None,
    };
    let text = std::str::from_utf8(&bytes).ok()?.trim();
    if text.is_empty() {
        return None;
    }
    Some(Zeroizing::new(text.to_string()))
}

/// Keychain first, then `file_reader`. `from_keychain` turns the Keychain value into the caller's
/// result type. The reader runs only when the Keychain has nothing usable.
pub(crate) fn secret_prefer_keychain<T>(
    kc: &dyn Keychain,
    name: &str,
    from_keychain: impl FnOnce(Zeroizing<String>) -> T,
    file_reader: impl FnOnce() -> T,
) -> T {
    secret_prefer_keychain_if(kc, name, |_| true, from_keychain, file_reader)
}

/// As `secret_prefer_keychain`, but a Keychain value is used only when `accept` passes it; an
/// unacceptable value falls through to the file reader like a missing one.
pub(crate) fn secret_prefer_keychain_if<T>(
    kc: &dyn Keychain,
    name: &str,
    accept: impl FnOnce(&str) -> bool,
    from_keychain: impl FnOnce(Zeroizing<String>) -> T,
    file_reader: impl FnOnce() -> T,
) -> T {
    match keychain_value(kc, name) {
        Some(v) if accept(&v) => from_keychain(v),
        _ => file_reader(),
    }
}

/// Runs `f` with the real Keychain. In unit tests the Keychain is empty unless a test installs a
/// fake for its own thread, so no test ever touches the real one.
#[cfg(not(test))]
pub(crate) fn with_live_keychain<R>(f: impl FnOnce(&dyn Keychain) -> R) -> R {
    f(&super::keychain::MacKeychain)
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_KEYCHAIN: std::cell::RefCell<Option<std::sync::Arc<dyn Keychain>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_live_keychain<R>(f: impl FnOnce(&dyn Keychain) -> R) -> R {
    let installed = TEST_KEYCHAIN.with(|k| k.borrow().clone());
    match installed {
        Some(kc) => f(&*kc),
        None => f(&super::keychain::FakeKeychain::default()),
    }
}

/// The file-side value of a file-backed name. `Ok(None)` = no file (or no `HA_TOKEN` line);
/// `Err(())` = the file exists but is unsafe or unusable.
fn read_file_secret(name: &str, dir: &Path) -> Result<Option<Zeroizing<String>>, ()> {
    match name {
        "ha_token" => super::ha::app_env_token_value(dir).map_err(|_| ()),
        "n8n_api_key" | "crew_service_token" => {
            let path = dir.join(name);
            match std::fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(()),
                Ok(_) => super::crew::crew_token_from(None, &path)
                    .map(|v| Some(Zeroizing::new(v)))
                    .map_err(|_| ()),
            }
        }
        _ => Err(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    File,
    Keychain,
    Both,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportRow {
    pub name: String,
    pub source: ImportSource,
    /// False only when a file exists but is unsafe (wrong mode, symlink, not ours, unreadable).
    pub file_mode_ok: bool,
}

pub(crate) fn review_with(kc: &dyn Keychain, dir: &Path) -> Vec<ImportRow> {
    FILE_BACKED
        .iter()
        .map(|(name, _)| {
            let file = read_file_secret(name, dir);
            let file_mode_ok = file.is_ok();
            let has_file = matches!(file, Ok(Some(_)));
            let has_kc = keychain_value(kc, name).is_some();
            let source = match (has_file, has_kc) {
                (true, true) => ImportSource::Both,
                (true, false) => ImportSource::File,
                (false, true) => ImportSource::Keychain,
                (false, false) => ImportSource::None,
            };
            ImportRow {
                name: (*name).to_string(),
                source,
                file_mode_ok,
            }
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub async fn keychain_import_review() -> Result<Vec<ImportRow>, String> {
    tokio::task::spawn_blocking(|| {
        review_with(&super::keychain::MacKeychain, &crate::app_paths::data_dir())
    })
    .await
    .map_err(|_| "The review could not run.".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct MigrateResult {
    pub ok: bool,
    pub name: String,
}

/// Proof that the native dialog returned Allow. Constructed only in the command (and in tests).
pub struct ApprovalToken(());

pub(crate) fn gate(enabled: bool) -> Result<(), String> {
    if enabled {
        Ok(())
    } else {
        Err(DISABLED.to_string())
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Copy, read back, compare. Never writes, deletes or edits the file.
pub(crate) fn migrate_core(
    kc: &dyn Keychain,
    name: &str,
    dir: &Path,
    _approved: &ApprovalToken,
) -> Result<MigrateResult, String> {
    if !is_file_backed(name) || !super::keychain::is_allowed(name) {
        return Err(NOT_MOVABLE.to_string());
    }
    // Proceed only on a definite "not found": any value (even blank or non-UTF-8) or any Keychain
    // error refuses, so an item is never overwritten blind.
    match kc.get(&service_for(name)) {
        Err(KeychainError::NotFound) => {}
        Ok(v) => {
            drop(Zeroizing::new(v));
            return Err(ALREADY.to_string());
        }
        Err(KeychainError::Other(_)) => return Err(KEYCHAIN_UNREADABLE.to_string()),
    }
    let value = match read_file_secret(name, dir) {
        Ok(Some(v)) if !v.trim().is_empty() && v.len() <= MAX_SECRET_BYTES => v,
        _ => return Err(NO_FILE_VALUE.to_string()),
    };
    let service = service_for(name);
    kc.set(&service, value.as_bytes())
        .map_err(|_| WRITE_FAILED.to_string())?;
    let back = kc
        .get(&service)
        .map(Zeroizing::new)
        .map_err(|_| MISMATCH.to_string())?;
    if !ct_eq(back.as_slice(), value.as_bytes()) {
        return Err(MISMATCH.to_string());
    }
    Ok(MigrateResult {
        ok: true,
        name: name.to_string(),
    })
}

/// Click-only. Refuses while the gate is closed, then asks in the native dialog; the dialog names
/// the key (never a value) and says the file stays as it is.
#[tauri::command]
#[specta::specta]
pub async fn keychain_migrate_one(
    app: tauri::AppHandle,
    name: String,
) -> Result<MigrateResult, String> {
    gate(KEYCHAIN_MIGRATE_ENABLED)?;
    if !is_file_backed(&name) {
        return Err(NOT_MOVABLE.to_string());
    }
    let label = super::keychain::SECRETS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, l)| *l)
        .unwrap_or("key");
    let text = super::approval_gate::approval_text(
        &format!("Copy the {label} into the Keychain"),
        "The saved copy is checked after writing. The file is not changed or deleted. Allow only if you just asked for this.",
    )?;
    let key = super::approval_gate::service_key(&format!("keychain-migrate-{name}"));
    let _guard = super::approval_gate::require_approval(&app, &[key], text).await?;
    let token = ApprovalToken(());
    tokio::task::spawn_blocking(move || {
        migrate_core(
            &super::keychain::MacKeychain,
            &name,
            &crate::app_paths::data_dir(),
            &token,
        )
    })
    .await
    .map_err(|_| WRITE_FAILED.to_string())?
}

#[cfg(test)]
mod tests {
    use super::super::keychain::FakeKeychain;
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    const SENTINEL: &str = "SENTINEL-4b8e1d72-do-not-leak-aaaaaaaaaa";

    fn tmp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-kcm-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        d.canonicalize().unwrap()
    }

    fn write(dir: &Path, name: &str, body: &str, mode: u32) {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn put(kc: &FakeKeychain, name: &str, v: &str) {
        kc.set(&service_for(name), v.as_bytes()).unwrap();
    }

    fn token() -> ApprovalToken {
        ApprovalToken(())
    }

    #[test]
    fn keychain_wins_then_file_then_nothing() {
        let kc = FakeKeychain::default();
        let read = |kc: &FakeKeychain| {
            secret_prefer_keychain(
                kc,
                "n8n_api_key",
                |v| Some(v.to_string()),
                || Some("from-file".to_string()),
            )
        };
        assert_eq!(read(&kc).as_deref(), Some("from-file"));
        put(&kc, "n8n_api_key", "   \n");
        assert_eq!(
            read(&kc).as_deref(),
            Some("from-file"),
            "blank falls through"
        );
        put(&kc, "n8n_api_key", " from-keychain \n");
        assert_eq!(read(&kc).as_deref(), Some("from-keychain"));
        // Non-allowlisted names never read the Keychain.
        put(&kc, "not_a_secret", "x");
        assert!(keychain_value(&kc, "not_a_secret").is_none());
    }

    #[test]
    fn n8n_key_precedence() {
        let kc = FakeKeychain::default();
        let dir = tmp();
        let f = dir.join("n8n_api_key");
        // both missing: today's behaviour (None)
        assert_eq!(super::super::n8n_status::n8n_api_key_with(&kc, &f), None);
        write(&dir, "n8n_api_key", "file-key\n", 0o600);
        assert_eq!(
            super::super::n8n_status::n8n_api_key_with(&kc, &f).as_deref(),
            Some("file-key")
        );
        put(&kc, "n8n_api_key", "kc-key");
        assert_eq!(
            super::super::n8n_status::n8n_api_key_with(&kc, &f).as_deref(),
            Some("kc-key")
        );
        // a keychain value also wins over an unsafe file
        write(&dir, "n8n_api_key", "file-key\n", 0o644);
        assert_eq!(
            super::super::n8n_status::n8n_api_key_with(&kc, &f).as_deref(),
            Some("kc-key")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crew_token_precedence_keeps_env_first() {
        use super::super::crew::{crew_token_with, CREW_TOKEN_FILE};
        let kc = FakeKeychain::default();
        let dir = tmp();
        let f = dir.join(CREW_TOKEN_FILE);
        // both missing: today's error text
        let none = crew_token_with(&kc, None, &f).unwrap_err();
        assert!(none.contains("not configured"), "{none}");
        write(&dir, CREW_TOKEN_FILE, "file-tok\n", 0o600);
        assert_eq!(crew_token_with(&kc, None, &f).unwrap(), "file-tok");
        put(&kc, "crew_service_token", "   ");
        assert_eq!(crew_token_with(&kc, None, &f).unwrap(), "file-tok");
        put(&kc, "crew_service_token", "kc-tok");
        assert_eq!(crew_token_with(&kc, None, &f).unwrap(), "kc-tok");
        assert_eq!(
            crew_token_with(&kc, Some(" env-tok ".into()), &f).unwrap(),
            "env-tok",
            "env still first"
        );
        assert_eq!(
            crew_token_with(&kc, Some("  ".into()), &f).unwrap(),
            "kc-tok",
            "blank env falls to keychain"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    const HA_TOK: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCD";
    const HA_KC: &str = "KEYCHAINTOKEN0123456789abcdefghijklmnopq";

    #[test]
    fn ha_token_precedence() {
        let dir = tmp();
        let resolve = |dir: &Path| super::super::ha::test_token_for(dir);
        // both missing: today's error
        TEST_KEYCHAIN.with(|k| *k.borrow_mut() = None);
        assert!(resolve(&dir).is_err());
        write(&dir, ".env", &format!("HA_TOKEN={HA_TOK}\n"), 0o600);
        assert_eq!(resolve(&dir).unwrap(), HA_TOK);
        let kc = Arc::new(FakeKeychain::default());
        TEST_KEYCHAIN.with(|k| *k.borrow_mut() = Some(kc.clone()));
        put(&kc, "ha_token", "  ");
        assert_eq!(resolve(&dir).unwrap(), HA_TOK, "blank falls through");
        put(&kc, "ha_token", HA_KC);
        assert_eq!(resolve(&dir).unwrap(), HA_KC, "keychain wins");
        TEST_KEYCHAIN.with(|k| *k.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn review_reports_names_sources_and_mode_only() {
        let kc = FakeKeychain::default();
        let dir = tmp();
        write(&dir, "n8n_api_key", SENTINEL, 0o600);
        write(&dir, "crew_service_token", SENTINEL, 0o644);
        put(&kc, "n8n_api_key", SENTINEL);
        put(&kc, "ha_token", SENTINEL);
        let rows = review_with(&kc, &dir);
        let by = |n: &str| rows.iter().find(|r| r.name == n).unwrap().clone();
        assert_eq!(by("n8n_api_key").source, ImportSource::Both);
        assert!(by("n8n_api_key").file_mode_ok);
        assert_eq!(by("ha_token").source, ImportSource::Keychain);
        assert_eq!(by("crew_service_token").source, ImportSource::None);
        assert!(!by("crew_service_token").file_mode_ok, "0644 flagged");
        assert_eq!(rows.len(), FILE_BACKED.len());
        let json = serde_json::to_string(&rows).unwrap();
        assert!(!json.contains(SENTINEL));
        assert!(json.contains("fileModeOk") && json.contains("\"source\":\"both\""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gate_is_closed_by_default_and_command_checks_it_first() {
        const { assert!(!KEYCHAIN_MIGRATE_ENABLED) };
        assert_eq!(gate(false).unwrap_err(), DISABLED);
        assert!(gate(true).is_ok());
        let src = include_str!("keychain_migrate.rs");
        let body = src.split("mod tests {").next().unwrap();
        let cmd = &body[body.find("pub async fn keychain_migrate_one").unwrap()..];
        let g = cmd.find("gate(KEYCHAIN_MIGRATE_ENABLED)").unwrap();
        let a = cmd.find("require_approval(").unwrap();
        let t = cmd.find("ApprovalToken(())").unwrap();
        assert!(g < a && a < t, "gate, then dialog, then the token");
        assert_eq!(body.matches("ApprovalToken(())").count(), 2); // type decl + the one mint
        assert!(!body.contains("remove_file") && !body.contains("fs::write"));
    }

    #[test]
    fn copies_and_verifies_without_touching_the_file() {
        let kc = FakeKeychain::default();
        let dir = tmp();
        write(&dir, "n8n_api_key", &format!("{SENTINEL}\n"), 0o600);
        let before = std::fs::read(dir.join("n8n_api_key")).unwrap();
        let r = migrate_core(&kc, "n8n_api_key", &dir, &token()).unwrap();
        assert_eq!(
            r,
            MigrateResult {
                ok: true,
                name: "n8n_api_key".into()
            }
        );
        assert_eq!(
            keychain_value(&kc, "n8n_api_key").unwrap().as_str(),
            SENTINEL
        );
        assert_eq!(std::fs::read(dir.join("n8n_api_key")).unwrap(), before);
        assert!(!serde_json::to_string(&r).unwrap().contains(SENTINEL));
        // second run does not overwrite
        assert_eq!(
            migrate_core(&kc, "n8n_api_key", &dir, &token()).unwrap_err(),
            ALREADY
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ha_token_copies_from_env_file_and_leaves_it() {
        let kc = FakeKeychain::default();
        let dir = tmp();
        let body = format!("OTHER=1\nHA_TOKEN={HA_TOK}\n");
        write(&dir, ".env", &body, 0o600);
        migrate_core(&kc, "ha_token", &dir, &token()).unwrap();
        assert_eq!(keychain_value(&kc, "ha_token").unwrap().as_str(), HA_TOK);
        assert_eq!(std::fs::read_to_string(dir.join(".env")).unwrap(), body);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_unknown_missing_and_unsafe_with_fixed_text() {
        let kc = FakeKeychain::default();
        let dir = tmp();
        for n in ["telegram_bot_token", "nope", "weaviate_api_key"] {
            assert_eq!(
                migrate_core(&kc, n, &dir, &token()).unwrap_err(),
                NOT_MOVABLE
            );
        }
        assert_eq!(
            migrate_core(&kc, "n8n_api_key", &dir, &token()).unwrap_err(),
            NO_FILE_VALUE
        );
        write(&dir, "n8n_api_key", SENTINEL, 0o644);
        let e = migrate_core(&kc, "n8n_api_key", &dir, &token()).unwrap_err();
        assert_eq!(e, NO_FILE_VALUE);
        assert!(!e.contains(SENTINEL));
        assert!(keychain_value(&kc, "n8n_api_key").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Writes succeed but read back something else.
    struct Lying(FakeKeychain);
    impl Keychain for Lying {
        fn get(&self, s: &str) -> Result<Vec<u8>, KeychainError> {
            self.0.get(s).map(|mut v| {
                v.push(b'x');
                v
            })
        }
        fn set(&self, s: &str, v: &[u8]) -> Result<(), KeychainError> {
            self.0.set(s, v)
        }
    }
    struct Failing;
    impl Keychain for Failing {
        fn get(&self, _: &str) -> Result<Vec<u8>, KeychainError> {
            Err(KeychainError::NotFound)
        }
        fn set(&self, _: &str, _: &[u8]) -> Result<(), KeychainError> {
            Err(KeychainError::Other(format!("boom {SENTINEL}")))
        }
    }

    #[test]
    fn read_back_mismatch_and_write_failure_use_fixed_text() {
        let dir = tmp();
        write(&dir, "crew_service_token", SENTINEL, 0o600);
        let e = migrate_core(
            &Lying(FakeKeychain::default()),
            "crew_service_token",
            &dir,
            &token(),
        )
        .unwrap_err();
        assert_eq!(e, MISMATCH);
        let e = migrate_core(&Failing, "crew_service_token", &dir, &token()).unwrap_err();
        assert_eq!(e, WRITE_FAILED);
        assert!(!e.contains(SENTINEL));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_keychain_ha_token_falls_to_the_env_file_not_past_it() {
        let dir = tmp();
        write(&dir, ".env", &format!("HA_TOKEN={HA_TOK}\n"), 0o600);
        let kc = Arc::new(FakeKeychain::default());
        TEST_KEYCHAIN.with(|k| *k.borrow_mut() = Some(kc.clone()));
        put(&kc, "ha_token", "bad token\n");
        assert_eq!(super::super::ha::test_token_for(&dir).unwrap(), HA_TOK);
        TEST_KEYCHAIN.with(|k| *k.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_set_of_service_credentials_asks_and_formats_are_enforced() {
        use super::super::keychain::{needs_approval, validate_for};
        for (n, _) in FILE_BACKED {
            assert!(needs_approval(n, false), "{n}");
        }
        assert!(needs_approval("weaviate_api_key", false));
        assert!(!needs_approval("n8n_encryption_key", false));
        assert!(validate_for("ha_token", HA_TOK).is_ok());
        for bad in [
            "short",
            "has space 0123456789abcdefghijklmnopqrstu",
            "line\nbreak0123456789abcdefghijklmnopqrstuv",
        ] {
            assert!(validate_for("ha_token", bad).is_err(), "{bad:?}");
        }
        for n in ["n8n_api_key", "crew_service_token", "weaviate_api_key"] {
            assert!(validate_for(n, "tok-123_ABC.x").is_ok());
            for bad in ["a b", "a\r\nb", "tab\tx", "caf\u{e9}", ""] {
                assert!(validate_for(n, bad).is_err(), "{n} {bad:?}");
            }
        }
        assert!(validate_for("coach_keys", "anything goes {\"json\": 1}").is_ok());
    }

    struct Odd(Result<Vec<u8>, KeychainError>);
    impl Keychain for Odd {
        fn get(&self, _: &str) -> Result<Vec<u8>, KeychainError> {
            match &self.0 {
                Ok(v) => Ok(v.clone()),
                Err(KeychainError::NotFound) => Err(KeychainError::NotFound),
                Err(KeychainError::Other(m)) => Err(KeychainError::Other(m.clone())),
            }
        }
        fn set(&self, _: &str, _: &[u8]) -> Result<(), KeychainError> {
            panic!("must not write over an existing or unreadable item")
        }
    }

    #[test]
    fn migrate_refuses_when_the_keychain_item_exists_in_any_shape() {
        let dir = tmp();
        write(&dir, "n8n_api_key", SENTINEL, 0o600);
        for odd in [
            Odd(Ok(vec![0xff, 0xfe, 0xfd])),
            Odd(Ok(b"  \n".to_vec())),
            Odd(Ok(Vec::new())),
            Odd(Err(KeychainError::Other(format!("denied {SENTINEL}")))),
        ] {
            let e = migrate_core(&odd, "n8n_api_key", &dir, &token()).unwrap_err();
            assert!(e == ALREADY || e == KEYCHAIN_UNREADABLE, "{e}");
            assert!(!e.contains(SENTINEL));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn token_file_reader_checks_the_handle() {
        use super::super::crew::crew_token_from;
        let dir = tmp();
        let f = dir.join("t");
        write(&dir, "t", "ok-token\n", 0o600);
        assert_eq!(crew_token_from(None, &f).unwrap(), "ok-token");
        // oversize is refused, not truncated
        write(&dir, "t", &"a".repeat(9000), 0o600);
        assert!(crew_token_from(None, &f).unwrap_err().contains("too large"));
        // a symlink is refused even when its target is 0600
        write(&dir, "real", "ok-token", 0o600);
        let link = dir.join("link");
        std::os::unix::fs::symlink(dir.join("real"), &link).unwrap();
        assert!(crew_token_from(None, &link)
            .unwrap_err()
            .contains("not a regular file"));
        // a FIFO does not hang or read
        let fifo = dir.join("fifo");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert!(crew_token_from(None, &fifo).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_names_are_allowlisted() {
        for n in ["n8n_api_key", "crew_service_token", "weaviate_api_key"] {
            assert!(super::super::keychain::is_allowed(n), "{n}");
        }
    }
}
