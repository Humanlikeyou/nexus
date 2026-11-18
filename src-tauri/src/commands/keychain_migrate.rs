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

// ── Stage G6: Oura, coach and the legacy Telegram token (decision 0033 Q6, Q7) ──
//
// Where they live today (names only):
//   oura_tokens      NOOP.app's Keychain item  service `com.noop.oura`,   account `oauth-tokens`
//   coach_api_key    NOOP.app's Keychain item  service `com.noop.aicoach`, account `api-key`
//   telegram_bot_token  plaintext key `telegram_bot_token` in `<data>/config.json`
//   hevy_api_key     nowhere: NOOP only imports Hevy CSV files and stores no Hevy key
// `keychain_move_one` copies one of them into Amara's own item (`com.taisun.nexus.<name>`), reads it
// back and compares, and only for the config.json token removes the plaintext, after that check.
// NOOP.app's own Keychain items are NOT deleted: they are not plaintext and NOOP.app still needs
// them until Taisun retires it.

/// The gate for these moves. Taisun's words are logged in decision 0033 (Q6, Q7); each move still
/// needs its own native approval.
pub const SECRET_MOVE_ENABLED: bool = true;
const NO_SOURCE_FOUND: &str = "Nothing to move was found for that key; nothing was changed.";
const REMOVE_FAILED: &str =
    "The Keychain copy is verified, but the old plaintext could not be removed.";
const PLAINTEXT_STILL_THERE: &str =
    "The Keychain copy is verified, but the plaintext is still in the file.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoveSource {
    /// Another app's Keychain item (service, account). Copied, never deleted.
    NoopKeychain(&'static str, &'static str),
    /// The plaintext Telegram token in `config.json`. Removed after the copy is verified.
    ConfigTelegram,
}

pub(crate) const MOVES: &[(&str, MoveSource)] = &[
    (
        "oura_tokens",
        MoveSource::NoopKeychain("com.noop.oura", "oauth-tokens"),
    ),
    (
        "coach_api_key",
        MoveSource::NoopKeychain("com.noop.aicoach", "api-key"),
    ),
    ("telegram_bot_token", MoveSource::ConfigTelegram),
];

fn move_source(name: &str) -> Option<MoveSource> {
    MOVES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Reads another app's Keychain item. The value is only ever held in `Zeroizing` memory.
pub(crate) trait LegacyReader {
    fn read(&self, service: &str, account: &str) -> Result<Vec<u8>, KeychainError>;
}

pub(crate) struct MacLegacyReader;

#[cfg(target_os = "macos")]
impl LegacyReader for MacLegacyReader {
    fn read(&self, service: &str, account: &str) -> Result<Vec<u8>, KeychainError> {
        use security_framework::passwords::get_generic_password;
        get_generic_password(service, account).map_err(|e| {
            if e.code() == -25300 {
                KeychainError::NotFound
            } else {
                KeychainError::Other(format!("keychain error {}", e.code()))
            }
        })
    }
}

#[cfg(not(target_os = "macos"))]
impl LegacyReader for MacLegacyReader {
    fn read(&self, _: &str, _: &str) -> Result<Vec<u8>, KeychainError> {
        Err(KeychainError::Other("Keychain is macOS only".into()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MoveRow {
    pub name: String,
    /// `noopKeychain` | `configJson` | `none`. Whether a source exists, never its value.
    pub source: String,
    /// Whether Amara's own Keychain item is set.
    pub amara_set: bool,
}

pub(crate) fn move_review_with(
    kc: &dyn Keychain,
    legacy: &dyn super::noop_freeze::KeychainNames,
    config_path: &Path,
) -> Vec<MoveRow> {
    let mut rows: Vec<MoveRow> = MOVES
        .iter()
        .map(|(name, src)| {
            let has = match src {
                MoveSource::NoopKeychain(service, _) => legacy.exists(service),
                MoveSource::ConfigTelegram => {
                    super::telegram::plaintext_token_at(config_path).is_some()
                }
            };
            MoveRow {
                name: (*name).to_string(),
                source: match (has, src) {
                    (false, _) => "none",
                    (true, MoveSource::NoopKeychain(..)) => "noopKeychain",
                    (true, MoveSource::ConfigTelegram) => "configJson",
                }
                .to_string(),
                amara_set: keychain_value(kc, name).is_some(),
            }
        })
        .collect();
    // Hevy has no stored key anywhere; listed so the screen can say so.
    rows.push(MoveRow {
        name: "hevy_api_key".into(),
        source: "none".into(),
        amara_set: keychain_value(kc, "hevy_api_key").is_some(),
    });
    rows
}

/// Presence only; never a value. Attribute-only Keychain search for NOOP's items, no prompt.
#[tauri::command]
#[specta::specta]
pub async fn keychain_move_review() -> Result<Vec<MoveRow>, String> {
    tokio::task::spawn_blocking(|| {
        move_review_with(
            &super::keychain::MacKeychain,
            &super::noop_freeze::MacKeychainNames,
            &super::telegram::config_path(),
        )
    })
    .await
    .map_err(|_| "The review could not run.".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MoveResult {
    pub ok: bool,
    pub name: String,
    /// True only when a plaintext copy existed and was removed after the Keychain copy verified.
    pub plaintext_removed: bool,
}

/// Does the source exist at all? Checked before the dialog so Taisun is never asked about nothing.
/// NOOP's Keychain items by attribute search only (no value read, no prompt).
pub(crate) fn move_source_exists(
    src: MoveSource,
    names: &dyn super::noop_freeze::KeychainNames,
    config_path: &Path,
) -> bool {
    match src {
        MoveSource::NoopKeychain(service, _) => names.exists(service),
        MoveSource::ConfigTelegram => super::telegram::plaintext_token_at(config_path).is_some(),
    }
}

fn read_move_value(
    src: MoveSource,
    legacy: &dyn LegacyReader,
    config_path: &Path,
) -> Option<Zeroizing<String>> {
    let v = match src {
        MoveSource::NoopKeychain(service, account) => {
            let bytes = Zeroizing::new(legacy.read(service, account).ok()?);
            let t = std::str::from_utf8(&bytes).ok()?.trim().to_string();
            Zeroizing::new(t)
        }
        MoveSource::ConfigTelegram => super::telegram::plaintext_token_at(config_path)?,
    };
    (!v.is_empty() && v.len() <= MAX_SECRET_BYTES).then_some(v)
}

/// Copy, read back and compare; then, for the config.json token only, remove the plaintext and
/// check it is gone. An Amara item that already holds a DIFFERENT value is never overwritten; one
/// that holds the SAME value lets an interrupted move finish (verify, then remove).
pub(crate) fn move_core(
    kc: &dyn Keychain,
    legacy: &dyn LegacyReader,
    name: &str,
    config_path: &Path,
    _approved: &ApprovalToken,
) -> Result<MoveResult, String> {
    let src = move_source(name).ok_or_else(|| NOT_MOVABLE.to_string())?;
    if !super::keychain::is_allowed(name) {
        return Err(NOT_MOVABLE.to_string());
    }
    let value =
        read_move_value(src, legacy, config_path).ok_or_else(|| NO_SOURCE_FOUND.to_string())?;
    let service = service_for(name);
    match kc.get(&service) {
        Err(KeychainError::NotFound) => {
            kc.set(&service, value.as_bytes())
                .map_err(|_| WRITE_FAILED.to_string())?;
        }
        Ok(existing) => {
            let existing = Zeroizing::new(existing);
            if !ct_eq(existing.as_slice(), value.as_bytes()) {
                return Err(ALREADY.to_string());
            }
        }
        Err(KeychainError::Other(_)) => return Err(KEYCHAIN_UNREADABLE.to_string()),
    }
    let back = kc
        .get(&service)
        .map(Zeroizing::new)
        .map_err(|_| MISMATCH.to_string())?;
    if !ct_eq(back.as_slice(), value.as_bytes()) {
        return Err(MISMATCH.to_string());
    }
    let mut plaintext_removed = false;
    if src == MoveSource::ConfigTelegram {
        // Only now, after the verified read-back.
        super::telegram::clear_plaintext_token_at(config_path)
            .map_err(|_| REMOVE_FAILED.to_string())?;
        if super::telegram::plaintext_token_at(config_path).is_some() {
            return Err(PLAINTEXT_STILL_THERE.to_string());
        }
        plaintext_removed = true;
    }
    Ok(MoveResult {
        ok: true,
        name: name.to_string(),
        plaintext_removed,
    })
}

/// Click-only; one native approval per item. The dialog names the key and says what happens to the
/// old copy; it never shows a value.
#[tauri::command]
#[specta::specta]
pub async fn keychain_move_one(app: tauri::AppHandle, name: String) -> Result<MoveResult, String> {
    gate(SECRET_MOVE_ENABLED)?;
    let src = move_source(&name).ok_or_else(|| NOT_MOVABLE.to_string())?;
    let label = super::keychain::SECRETS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, l)| *l)
        .unwrap_or("key");
    let after = match src {
        MoveSource::NoopKeychain(..) => {
            "NOOP.app's own copy is left as it is. macOS may ask once to let Amara read it."
        }
        MoveSource::ConfigTelegram => {
            "The saved copy is checked, then the plaintext token is removed from config.json."
        }
    };
    // Nothing to move: say so without a dialog.
    let exists_src = tokio::task::spawn_blocking(move || {
        move_source_exists(
            src,
            &super::noop_freeze::MacKeychainNames,
            &super::telegram::config_path(),
        )
    })
    .await
    .map_err(|_| NO_SOURCE_FOUND.to_string())?;
    if !exists_src {
        return Err(NO_SOURCE_FOUND.to_string());
    }
    let text = super::approval_gate::approval_text(
        &format!("Move the {label} into Amara's Keychain"),
        &format!("{after} Choose Allow, not Always Allow. Allow only if you just asked for this."),
    )?;
    let key = super::approval_gate::service_key(&format!("keychain-move-{name}"));
    let _guard = super::approval_gate::require_approval(&app, &[key], text).await?;
    let token = ApprovalToken(());
    tokio::task::spawn_blocking(move || {
        move_core(
            &super::keychain::MacKeychain,
            &MacLegacyReader,
            &name,
            &super::telegram::config_path(),
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
        // The file-backed copier only; the Stage G6 section below has its own pinned test.
        let body = src.split("// ── Stage G6").next().unwrap();
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

    // ── Stage G6 moves ──

    struct FakeLegacy(std::collections::HashMap<(String, String), Vec<u8>>);
    impl LegacyReader for FakeLegacy {
        fn read(&self, service: &str, account: &str) -> Result<Vec<u8>, KeychainError> {
            self.0
                .get(&(service.to_string(), account.to_string()))
                .cloned()
                .ok_or(KeychainError::NotFound)
        }
    }
    struct Exists(Vec<&'static str>);
    impl super::super::noop_freeze::KeychainNames for Exists {
        fn exists(&self, s: &str) -> bool {
            self.0.contains(&s)
        }
    }
    fn legacy_with(service: &str, account: &str, v: &str) -> FakeLegacy {
        let mut m = std::collections::HashMap::new();
        m.insert(
            (service.to_string(), account.to_string()),
            v.as_bytes().to_vec(),
        );
        FakeLegacy(m)
    }
    fn cfg_with_token(dir: &Path, tok: &str) -> PathBuf {
        let p = dir.join("config.json");
        std::fs::write(
            &p,
            format!(
                r#"{{"telegram_bot_token":"{tok}","telegram_chat_id":"42","unknown_future_key":{{"a":1}},"allow_claude":false}}"#
            ),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        p
    }
    use std::path::PathBuf;

    #[test]
    fn new_names_are_on_the_allowlist_under_the_nexus_prefix() {
        for n in ["oura_tokens", "hevy_api_key", "coach_api_key"] {
            assert!(super::super::keychain::is_allowed(n));
            assert_eq!(service_for(n), format!("com.taisun.nexus.{n}"));
        }
    }

    #[test]
    fn noop_keychain_items_are_copied_verified_and_left_in_place() {
        let kc = FakeKeychain::default();
        let leg = legacy_with("com.noop.oura", "oauth-tokens", SENTINEL);
        let d = tmp();
        let r = move_core(&kc, &leg, "oura_tokens", &d.join("config.json"), &token()).unwrap();
        assert_eq!(
            r,
            MoveResult {
                ok: true,
                name: "oura_tokens".into(),
                plaintext_removed: false
            }
        );
        assert_eq!(
            keychain_value(&kc, "oura_tokens").unwrap().as_str(),
            SENTINEL
        );
        assert!(!format!("{r:?}").contains(SENTINEL));
        // The other app's item is only ever read: the trait has no write or delete.
        assert_eq!(leg.0.len(), 1);
        let c = legacy_with("com.noop.aicoach", "api-key", "  coach-secret-value \n");
        move_core(&kc, &c, "coach_api_key", &d.join("config.json"), &token()).unwrap();
        assert_eq!(
            keychain_value(&kc, "coach_api_key").unwrap().as_str(),
            "coach-secret-value"
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn nothing_to_move_or_unmovable_names_change_nothing() {
        let kc = FakeKeychain::default();
        let d = tmp();
        let none = FakeLegacy(Default::default());
        let cfg = d.join("config.json");
        for name in ["oura_tokens", "coach_api_key", "telegram_bot_token"] {
            assert_eq!(
                move_core(&kc, &none, name, &cfg, &token()).unwrap_err(),
                NO_SOURCE_FOUND
            );
            assert!(keychain_value(&kc, name).is_none());
        }
        // Hevy stores no key anywhere, so it is not movable; neither is an arbitrary name.
        for name in ["hevy_api_key", "ha_token", "weaviate_api_key", "../x"] {
            assert_eq!(
                move_core(&kc, &none, name, &cfg, &token()).unwrap_err(),
                NOT_MOVABLE
            );
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_different_existing_keychain_value_is_never_overwritten() {
        let kc = FakeKeychain::default();
        put(&kc, "oura_tokens", "existing-value");
        let leg = legacy_with("com.noop.oura", "oauth-tokens", SENTINEL);
        let d = tmp();
        assert_eq!(
            move_core(&kc, &leg, "oura_tokens", &d.join("c.json"), &token()).unwrap_err(),
            ALREADY
        );
        assert_eq!(
            keychain_value(&kc, "oura_tokens").unwrap().as_str(),
            "existing-value"
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn telegram_plaintext_is_removed_only_after_the_keychain_copy_verifies() {
        let kc = FakeKeychain::default();
        let d = tmp();
        let cfg = cfg_with_token(&d, SENTINEL);
        let none = FakeLegacy(Default::default());
        let r = move_core(&kc, &none, "telegram_bot_token", &cfg, &token()).unwrap();
        assert!(r.plaintext_removed);
        assert_eq!(
            keychain_value(&kc, "telegram_bot_token").unwrap().as_str(),
            SENTINEL
        );
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(!text.contains(SENTINEL), "plaintext gone");
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v["telegram_bot_token"].is_null());
        assert_eq!(v["telegram_chat_id"], "42");
        assert_eq!(v["unknown_future_key"]["a"], 1, "other keys are kept");
        assert_eq!(
            std::fs::metadata(&cfg).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // A second run has no plaintext left.
        assert_eq!(
            move_core(&kc, &none, "telegram_bot_token", &cfg, &token()).unwrap_err(),
            NO_SOURCE_FOUND
        );
        let _ = std::fs::remove_dir_all(d);
    }

    /// A Keychain whose write is accepted but reads back something else.
    struct LyingKeychain;
    impl Keychain for LyingKeychain {
        fn get(&self, _: &str) -> Result<Vec<u8>, KeychainError> {
            Err(KeychainError::NotFound)
        }
        fn set(&self, _: &str, _: &[u8]) -> Result<(), KeychainError> {
            Ok(())
        }
    }

    #[test]
    fn a_failed_verification_leaves_the_plaintext_in_place() {
        let d = tmp();
        let cfg = cfg_with_token(&d, SENTINEL);
        let before = std::fs::read(&cfg).unwrap();
        let none = FakeLegacy(Default::default());
        let r = move_core(&LyingKeychain, &none, "telegram_bot_token", &cfg, &token());
        assert_eq!(r.unwrap_err(), MISMATCH);
        assert_eq!(std::fs::read(&cfg).unwrap(), before, "file untouched");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn an_interrupted_move_finishes_when_the_keychain_already_has_the_same_value() {
        let kc = FakeKeychain::default();
        put(&kc, "telegram_bot_token", SENTINEL);
        let d = tmp();
        let cfg = cfg_with_token(&d, SENTINEL);
        let none = FakeLegacy(Default::default());
        let r = move_core(&kc, &none, "telegram_bot_token", &cfg, &token()).unwrap();
        assert!(r.plaintext_removed);
        // A different Keychain value keeps the plaintext and refuses.
        let kc2 = FakeKeychain::default();
        put(&kc2, "telegram_bot_token", "other");
        let cfg2 = cfg_with_token(&d.join(".."), SENTINEL);
        let before = std::fs::read(&cfg2).unwrap();
        assert_eq!(
            move_core(&kc2, &none, "telegram_bot_token", &cfg2, &token()).unwrap_err(),
            ALREADY
        );
        assert_eq!(std::fs::read(&cfg2).unwrap(), before);
        let _ = std::fs::remove_file(cfg2);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_corrupt_config_is_not_rewritten() {
        let d = tmp();
        let cfg = d.join("config.json");
        std::fs::write(&cfg, "{ not json").unwrap();
        assert!(super::super::telegram::clear_plaintext_token_at(&cfg).is_err());
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn source_existence_is_checked_without_reading_values() {
        let d = tmp();
        let cfg = cfg_with_token(&d, SENTINEL);
        let has = Exists(vec!["com.noop.oura"]);
        let oura = move_source("oura_tokens").unwrap();
        let coach = move_source("coach_api_key").unwrap();
        let tg = move_source("telegram_bot_token").unwrap();
        assert!(move_source_exists(oura, &has, &cfg));
        assert!(!move_source_exists(coach, &has, &cfg));
        assert!(move_source_exists(tg, &has, &cfg));
        assert!(!move_source_exists(tg, &has, &d.join("none.json")));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn review_reports_presence_and_names_only() {
        let kc = FakeKeychain::default();
        put(&kc, "coach_api_key", SENTINEL);
        let d = tmp();
        let cfg = cfg_with_token(&d, SENTINEL);
        let rows = move_review_with(&kc, &Exists(vec!["com.noop.oura"]), &cfg);
        let got: Vec<_> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.source.as_str(), r.amara_set))
            .collect();
        assert_eq!(
            got,
            vec![
                ("oura_tokens", "noopKeychain", false),
                ("coach_api_key", "none", true),
                ("telegram_bot_token", "configJson", false),
                ("hevy_api_key", "none", false),
            ]
        );
        assert!(!serde_json::to_string(&rows).unwrap().contains(SENTINEL));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn move_command_is_gated_asks_once_per_item_and_never_logs() {
        let src = include_str!("keychain_migrate.rs");
        let prod = &src[..src.find("#[cfg(test)]\nmod tests {").unwrap()];
        let cmd = &prod[prod.find("pub async fn keychain_move_one").unwrap()..];
        let i_gate = cmd.find("gate(SECRET_MOVE_ENABLED)").unwrap();
        let i_ask = cmd.find("require_approval").unwrap();
        let i_move = cmd.find("move_core(").unwrap();
        assert!(
            i_gate < i_ask && i_ask < i_move,
            "gate, then approval, then the move"
        );
        assert!(cmd.contains("keychain-move-{name}"), "a lock key per item");
        let mv = &prod[prod.find("pub(crate) fn move_core").unwrap()
            ..prod.find("pub async fn keychain_move_one").unwrap()];
        let i_back = mv.find("MISMATCH").unwrap();
        let i_clear = mv.find("clear_plaintext_token_at").unwrap();
        assert!(
            i_back < i_clear,
            "the plaintext is removed after the read-back check"
        );
        assert!(
            !prod.contains("println!") && !prod.contains("eprintln!") && !prod.contains("log::")
        );
    }
}
