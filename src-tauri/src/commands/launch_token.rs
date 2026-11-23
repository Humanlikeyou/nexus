//! Per-launch token for Amara-owned loopback services (router, crew, voice).
//!
//! CONTRACT (readers: amara-router, crew-service, voice bridge, the app's own HTTP clients):
//! - Path: `<app data dir>/run/launch_token` (data dir = `app_paths::data_dir()`, i.e.
//!   `$NEXUS_DATA_DIR` or `~/Library/Application Support/com.taisun.nexus`).
//! - `run/` is mode 0700, the file is mode 0600 (owner only).
//! - Content: exactly 64 lowercase hex characters (~244 bits: two v4 UUIDs, 122 random bits each, which draw on the OS
//!   CSPRNG) followed by one `\n`. Readers must trim whitespace and reject anything that is not 64
//!   hex chars.
//! - Minted fresh at every app start (the previous value is replaced atomically via rename, so a
//!   reader never sees a partial file). Removed best-effort on `RunEvent::Exit`.
//! - Clients send it as the `X-Amara-Launch-Token` header. Servers re-read the file per request (or
//!   on mtime change) so a rotation takes effect without a restart, compare in constant time, and
//!   FAIL CLOSED (401) if the file is absent or malformed. `/health` stays open.
//! - The app attaches the header only when the request base is loopback AND the call site opted in
//!   via `HttpSpec::with_launch_token()`. Never opt in for Hermes, HA, Letta, n8n, Weaviate or any
//!   off-Mac host.

// Readers/clients land in D1-D3/D9; until a call site opts in, parts are unused.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub(crate) const HEADER_NAME: &str = "X-Amara-Launch-Token";
const DIR_NAME: &str = "run";
const FILE_NAME: &str = "launch_token";
const TOKEN_LEN: usize = 64;

pub(crate) fn token_path_in(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR_NAME).join(FILE_NAME)
}

fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn is_valid(t: &str) -> bool {
    t.len() == TOKEN_LEN && t.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Mint a fresh token into `<data_dir>/run/launch_token`, replacing any previous one atomically.
pub(crate) fn mint_in(data_dir: &Path) -> std::io::Result<String> {
    let dir = data_dir.join(DIR_NAME);
    crate::app_paths::create_private_dir_all(&dir)?;
    let token = new_token();
    let tmp = dir.join(format!(".{FILE_NAME}.tmp"));
    let _ = std::fs::remove_file(&tmp);
    crate::app_paths::write_private(&tmp, format!("{token}\n"))?;
    std::fs::rename(&tmp, dir.join(FILE_NAME))?;
    Ok(token)
}

/// Remove the token file, best effort.
pub(crate) fn remove_in(data_dir: &Path) {
    let _ = std::fs::remove_file(token_path_in(data_dir));
}

/// The current token, or None when the file is absent or malformed.
pub(crate) fn read_in(data_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(token_path_in(data_dir)).ok()?;
    let t = raw.trim();
    is_valid(t).then(|| t.to_string())
}

/// Mint at app start. Failure is logged without the value; clients then send no header.
pub(crate) fn mint() {
    if let Err(e) = mint_fresh_in(&crate::app_paths::data_dir()) {
        eprintln!("launch_token: could not write token file: {}", e.kind());
    }
}

/// No stale token for clients to pick up: clear first, and again if minting fails.
pub(crate) fn mint_fresh_in(data_dir: &Path) -> std::io::Result<String> {
    remove_in(data_dir);
    mint_in(data_dir).inspect_err(|_| remove_in(data_dir))
}

/// Remove at app exit.
pub(crate) fn remove() {
    remove_in(&crate::app_paths::data_dir());
}

/// The current token from the real data dir, for client helpers. None = send no header.
pub(crate) fn current() -> Option<String> {
    read_in(&crate::app_paths::data_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("launch-token-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn mints_64_hex_with_0600_file_in_0700_dir() {
        let d = tmp("mode");
        let t = mint_in(&d).unwrap();
        assert!(is_valid(&t));
        let p = token_path_in(&d);
        assert_eq!(mode(&p), 0o600);
        assert_eq!(mode(p.parent().unwrap()), 0o700);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), format!("{t}\n"));
        assert_eq!(read_in(&d), Some(t));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn each_mint_rotates_and_leaves_no_temp_file() {
        let d = tmp("rot");
        let a = mint_in(&d).unwrap();
        let b = mint_in(&d).unwrap();
        assert_ne!(a, b);
        assert_eq!(read_in(&d), Some(b));
        let n = std::fs::read_dir(d.join("run")).unwrap().count();
        assert_eq!(n, 1);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn failed_mint_leaves_no_stale_token() {
        let d = tmp("stale");
        mint_in(&d).unwrap();
        assert!(read_in(&d).is_some());
        // Block the temp path with a directory so the next mint fails.
        std::fs::create_dir(d.join("run").join(format!(".{FILE_NAME}.tmp"))).unwrap();
        assert!(mint_fresh_in(&d).is_err());
        assert_eq!(read_in(&d), None);
        assert!(!token_path_in(&d).exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn remove_deletes_and_absent_or_malformed_reads_none() {
        let d = tmp("rm");
        assert_eq!(read_in(&d), None);
        mint_in(&d).unwrap();
        remove_in(&d);
        assert_eq!(read_in(&d), None);
        remove_in(&d); // idempotent
        crate::app_paths::write_private(&token_path_in(&d), "short\n").unwrap();
        assert_eq!(read_in(&d), None);
        std::fs::remove_dir_all(d).unwrap();
    }
}
