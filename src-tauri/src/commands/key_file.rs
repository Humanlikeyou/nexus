//! n8n key file, passphrase backend (HANDOFF-1111s wiring step 2).
//!
//! The n8n encryption key travels in its own passphrase-locked file, never inside a backup.
//!
//! DEVIATION (for Taisun): the handoff asks for Argon2id or scrypt + an AEAD. Neither crate is
//! available offline, so this uses `ring` (already locked): PBKDF2-HMAC-SHA256 with 600,000
//! iterations (OWASP 2023 minimum for this PRF) and a random 16-byte salt, then AES-256-GCM with a
//! random 12-byte nonce. PBKDF2 is not memory-hard, so a long passphrase matters more than it
//! would with Argon2id; swap the KDF when the crate can be added (the version byte allows it).
//!
//! File layout (bytes):
//!   0..6   magic "AMN8N\0"
//!   6      version (1)
//!   7..11  PBKDF2 iterations, u32 big-endian
//!   11..27 salt
//!   27..39 nonce
//!   39..   AES-256-GCM ciphertext + 16-byte tag (header 0..39 is the AAD)
//!
//! The passphrase and the derived key are zeroized. Nothing here returns, logs or stores a secret.

use super::keychain::{service_for, Keychain, KeychainError, MacKeychain, N8N_KEY};
use ring::{aead, pbkdf2, rand::SecureRandom, rand::SystemRandom};
use serde::Serialize;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

pub(crate) const KEY_FILE_NAME: &str = "amara-n8n.key";
const MAGIC: &[u8; 6] = b"AMN8N\0";
const VERSION: u8 = 1;
const ITERATIONS: u32 = 600_000;
/// A key file made elsewhere with a weaker cost must not unlock (tests pass a lower value explicitly).
const MIN_ITERATIONS: u32 = 600_000;
/// Refuse files that ask for absurd work.
const MAX_ITERATIONS: u32 = 5_000_000;
const HEADER_LEN: usize = 39;
const MIN_PASSPHRASE_CHARS: usize = 8;
const MAX_FILE_BYTES: u64 = 4096;

#[derive(Serialize, Debug)]
pub struct KeyFileStatus {
    pub made: bool,
    #[serde(rename = "madeAt", skip_serializing_if = "Option::is_none")]
    pub made_at: Option<String>,
    #[serde(rename = "where")]
    pub where_: String,
}

/// `NEXUS_KEY_FILE_DIR` is a test/dev override only; it is not settable from the UI or IPC.
/// `~/Amara Backups`: beside the backups, never inside one (archives are files in this folder,
/// and the packager never copies this folder or any file with the key file's name).
pub(crate) fn default_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("NEXUS_KEY_FILE_DIR") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Amara Backups")
}

fn tilde(p: &Path) -> String {
    match dirs::home_dir() {
        Some(h) => match p.strip_prefix(&h) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        },
        None => p.display().to_string(),
    }
}

fn derive(passphrase: &[u8], salt: &[u8], iterations: u32) -> Result<Zeroizing<[u8; 32]>, String> {
    let n = NonZeroU32::new(iterations).ok_or("bad_file")?;
    let mut out = Zeroizing::new([0u8; 32]);
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, n, salt, passphrase, &mut *out);
    Ok(out)
}

fn aead_key(k: &[u8; 32]) -> Result<aead::LessSafeKey, String> {
    aead::UnboundKey::new(&aead::AES_256_GCM, k)
        .map(aead::LessSafeKey::new)
        .map_err(|_| "crypto error".to_string())
}

pub(crate) fn encrypt(plain: &[u8], passphrase: &[u8], iterations: u32) -> Result<Vec<u8>, String> {
    let rng = SystemRandom::new();
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    rng.fill(&mut salt).map_err(|_| "random failed")?;
    rng.fill(&mut nonce).map_err(|_| "random failed")?;
    let mut file = Vec::with_capacity(HEADER_LEN + plain.len() + 16);
    file.extend_from_slice(MAGIC);
    file.push(VERSION);
    file.extend_from_slice(&iterations.to_be_bytes());
    file.extend_from_slice(&salt);
    file.extend_from_slice(&nonce);
    let key = derive(passphrase, &salt, iterations)?;
    let sealing = aead_key(&key)?;
    let mut buf = plain.to_vec();
    let sealed = sealing.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(nonce),
        aead::Aad::from(file.clone()),
        &mut buf,
    );
    if sealed.is_ok() {
        file.extend_from_slice(&buf);
    }
    buf.zeroize();
    sealed.map_err(|_| "crypto error")?;
    Ok(file)
}

/// `Err("wrong_passphrase")` for a bad passphrase or tampered file, `Err("bad_file")` for a file
/// that is not ours.
pub(crate) fn decrypt(
    file: &[u8],
    passphrase: &[u8],
    min_iters: u32,
) -> Result<Zeroizing<Vec<u8>>, String> {
    if file.len() < HEADER_LEN + 16 || &file[..6] != MAGIC || file[6] != VERSION {
        return Err("bad_file".into());
    }
    let iterations = u32::from_be_bytes([file[7], file[8], file[9], file[10]]);
    if iterations < min_iters || iterations > MAX_ITERATIONS {
        return Err("bad_file".into());
    }
    let salt = &file[11..27];
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&file[27..39]);
    let key = derive(passphrase, salt, iterations)?;
    let opening = aead_key(&key)?;
    let mut buf = Zeroizing::new(file[HEADER_LEN..].to_vec());
    let n = opening
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(&file[..HEADER_LEN]),
            buf.as_mut_slice(),
        )
        .map_err(|_| "wrong_passphrase".to_string())?
        .len();
    buf.truncate(n);
    Ok(buf)
}

pub(crate) fn status_in(dir: &Path) -> KeyFileStatus {
    let path = dir.join(KEY_FILE_NAME);
    let meta = std::fs::symlink_metadata(&path)
        .ok()
        .filter(|m| m.is_file());
    KeyFileStatus {
        made: meta.is_some(),
        made_at: meta
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339()),
        where_: tilde(dir),
    }
}

fn write_private_atomic(dir: &Path, bytes: &[u8], replace: bool) -> Result<(), String> {
    use std::io::Write;
    crate::app_paths::create_private_dir_all(dir)
        .map_err(|e| format!("cannot create folder: {e}"))?;
    let dest = dir.join(KEY_FILE_NAME);
    match std::fs::symlink_metadata(&dest) {
        Ok(m) if m.file_type().is_symlink() => return Err("key file path is a symlink".into()),
        Ok(_) if !replace => return Err("exists".into()),
        _ => {}
    }
    let tmp = dir.join(format!(
        ".{KEY_FILE_NAME}.{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let write = || -> std::io::Result<()> {
        // create_new: never reuse or truncate a pre-planted file; 0600 at creation.
        let mut f = crate::app_paths::private_open_options()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &dest)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot write key file: {e}")
    })
}

pub(crate) fn make_inner(
    kc: &dyn Keychain,
    dir: &Path,
    mut passphrase: String,
    iterations: u32,
    replace: bool,
) -> Result<(), String> {
    let result = (|| {
        if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
            return Err("passphrase_too_short".to_string());
        }
        let key = match kc.get(&service_for(N8N_KEY)) {
            Ok(k) => Zeroizing::new(k),
            Err(KeychainError::NotFound) => return Err("no_key".to_string()),
            Err(KeychainError::Other(e)) => return Err(e),
        };
        let file = encrypt(&key, passphrase.as_bytes(), iterations)?;
        write_private_atomic(dir, &file, replace)
    })();
    passphrase.zeroize();
    result
}

/// Reads the key file through one no-follow handle (fstat on the opened file, no
/// check-then-open gap). Missing, unreadable, not a regular file, oversize or not ours all return
/// `bad_file`, so the command is not an existence or format oracle.
fn read_key_file(p: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    let f = o.open(p).map_err(|_| "bad_file".to_string())?;
    let meta = f.metadata().map_err(|_| "bad_file".to_string())?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err("bad_file".into());
    }
    let mut bytes = Vec::new();
    f.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "bad_file".to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("bad_file".into());
    }
    Ok(bytes)
}

pub(crate) fn unlock_inner(
    kc: &dyn Keychain,
    path: &str,
    mut passphrase: String,
    min_iters: u32,
) -> Result<(), String> {
    let result = (|| {
        let p = super::packager::expand_home(path);
        // A directory is accepted (the default folder): use the key file inside it.
        let p = if p.is_dir() { p.join(KEY_FILE_NAME) } else { p };
        let bytes = read_key_file(&p)?;
        let key = decrypt(&bytes, passphrase.as_bytes(), min_iters)?;
        match kc.get(&service_for(N8N_KEY)) {
            Ok(existing) => {
                let existing = Zeroizing::new(existing);
                if *existing == *key {
                    return Ok(());
                }
                // Never overwrite a different key that is already in use.
                return Err("key_exists".to_string());
            }
            Err(KeychainError::NotFound) => {}
            Err(KeychainError::Other(e)) => return Err(e),
        }
        kc.set(&service_for(N8N_KEY), &key).map_err(|e| match e {
            KeychainError::NotFound => "keychain item missing".to_string(),
            KeychainError::Other(m) => m,
        })
    })();
    passphrase.zeroize();
    result
}

#[tauri::command]
pub async fn key_file_status() -> Result<KeyFileStatus, String> {
    Ok(status_in(&default_dir()))
}

#[tauri::command]
pub async fn key_file_make(
    app: tauri::AppHandle,
    passphrase: String,
    replace: Option<bool>,
) -> Result<(), String> {
    // Replacing an existing key file discards the old one: only after the native dialog.
    let _held = if replace.unwrap_or(false) && default_dir().join(KEY_FILE_NAME).exists() {
        let text = super::approval_gate::approval_text(
            "Replace the n8n key file",
            "The existing amara-n8n.key is overwritten; the old passphrase will no longer open anything. Allow only if you just asked for this.",
        )?;
        let key = super::approval_gate::service_key("n8n-key-file");
        Some(super::approval_gate::require_approval(&app, &[key], text).await?)
    } else {
        None
    };
    tokio::task::spawn_blocking(move || {
        make_inner(
            &MacKeychain,
            &default_dir(),
            passphrase,
            ITERATIONS,
            replace.unwrap_or(false),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn key_file_unlock(path: String, passphrase: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        unlock_inner(&MacKeychain, &path, passphrase, MIN_ITERATIONS)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::super::keychain::FakeKeychain;
    use super::*;

    // Few iterations in tests; the production constant is asserted separately.
    const FAST: u32 = 1000;
    const PASS: &str = "correct horse battery";

    fn make_with(kc: &dyn Keychain, dir: &Path, pass: String, it: u32) -> Result<(), String> {
        make_inner(kc, dir, pass, it, false)
    }
    /// Tests lower the minimum explicitly (production uses MIN_ITERATIONS).
    fn unlock_with(kc: &dyn Keychain, path: &str, pass: String) -> Result<(), String> {
        unlock_inner(kc, path, pass, FAST)
    }

    fn tmp(tag: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("nexus-kf-{tag}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn seeded(key: &[u8]) -> FakeKeychain {
        let kc = FakeKeychain::default();
        kc.set(&service_for(N8N_KEY), key).unwrap();
        kc
    }

    #[test]
    fn production_cost_is_at_least_600k() {
        let iterations = std::hint::black_box(ITERATIONS);
        assert!(iterations >= 600_000);
    }

    #[test]
    fn status_before_and_after_make() {
        let dir = tmp("status");
        assert!(!status_in(&dir).made);
        make_with(&seeded(b"k"), &dir, PASS.into(), FAST).unwrap();
        let s = status_in(&dir);
        assert!(s.made && s.made_at.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trip_is_byte_identical_and_file_is_0600() {
        let dir = tmp("rt");
        let key: Vec<u8> = (0u8..=255).collect(); // not valid UTF-8 on purpose
        make_with(&seeded(&key), &dir, PASS.into(), FAST).unwrap();
        let f = dir.join(KEY_FILE_NAME);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let bytes = std::fs::read(&f).unwrap();
        assert!(!bytes.windows(16).any(|w| w == &key[..16]));
        let fresh = FakeKeychain::default();
        unlock_with(&fresh, f.to_str().unwrap(), PASS.into()).unwrap();
        assert_eq!(fresh.get(&service_for(N8N_KEY)).unwrap(), key);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_passphrase_not_found_and_tamper() {
        let dir = tmp("err");
        make_with(&seeded(b"secret-key"), &dir, PASS.into(), FAST).unwrap();
        let f = dir.join(KEY_FILE_NAME);
        let fresh = FakeKeychain::default();
        assert_eq!(
            unlock_with(&fresh, f.to_str().unwrap(), "wrong passphrase".into()).unwrap_err(),
            "wrong_passphrase"
        );
        assert_eq!(
            fresh.get(&service_for(N8N_KEY)),
            Err(KeychainError::NotFound)
        );
        assert_eq!(
            unlock_with(&fresh, "/nonexistent/amara-n8n.key", PASS.into()).unwrap_err(),
            "bad_file"
        );
        // Flip a header bit (AAD) and a ciphertext bit.
        for idx in [8usize, 45] {
            let mut b = std::fs::read(&f).unwrap();
            b[idx] ^= 1;
            let t = dir.join("t.key");
            std::fs::write(&t, &b).unwrap();
            assert!(unlock_with(&fresh, t.to_str().unwrap(), PASS.into()).is_err());
        }
        let junk = dir.join("junk.key");
        std::fs::write(&junk, vec![7u8; 100]).unwrap();
        assert_eq!(
            unlock_with(&fresh, junk.to_str().unwrap(), PASS.into()).unwrap_err(),
            "bad_file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn short_passphrase_and_missing_key_are_refused_and_nothing_is_written() {
        let dir = tmp("refuse");
        assert_eq!(
            make_with(&seeded(b"k"), &dir, "short".into(), FAST).unwrap_err(),
            "passphrase_too_short"
        );
        assert_eq!(
            make_with(&FakeKeychain::default(), &dir, PASS.into(), FAST).unwrap_err(),
            "no_key"
        );
        assert!(!status_in(&dir).made);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unlock_never_overwrites_a_different_existing_key() {
        let dir = tmp("exists");
        make_with(&seeded(b"old"), &dir, PASS.into(), FAST).unwrap();
        let f = dir.join(KEY_FILE_NAME);
        let other = seeded(b"different");
        assert_eq!(
            unlock_with(&other, f.to_str().unwrap(), PASS.into()).unwrap_err(),
            "key_exists"
        );
        assert_eq!(other.get(&service_for(N8N_KEY)).unwrap(), b"different");
        let same = seeded(b"old");
        unlock_with(&same, f.to_str().unwrap(), PASS.into()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_secret_or_passphrase_in_errors_and_file() {
        let dir = tmp("leak");
        let err = make_with(
            &FakeKeychain::default(),
            &dir,
            "PASSPHRASE-SENTINEL-1".into(),
            FAST,
        )
        .unwrap_err();
        assert!(!err.contains("SENTINEL"));
        make_with(
            &seeded(b"KEYSENTINEL-xyz-123456"),
            &dir,
            "PASSPHRASE-SENTINEL-1".into(),
            FAST,
        )
        .unwrap();
        let b = std::fs::read(dir.join(KEY_FILE_NAME)).unwrap();
        assert!(!b.windows(8).any(|w| w == b"SENTINEL"));
        let err = unlock_with(
            &FakeKeychain::default(),
            dir.join(KEY_FILE_NAME).to_str().unwrap(),
            "x".into(),
        )
        .unwrap_err();
        assert!(!err.contains("SENTINEL"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn weak_key_files_are_refused_in_production_mode() {
        let dir = tmp("weak");
        make_with(&seeded(b"k"), &dir, PASS.into(), FAST).unwrap();
        let f = dir.join(KEY_FILE_NAME);
        let fresh = FakeKeychain::default();
        assert_eq!(
            unlock_inner(&fresh, f.to_str().unwrap(), PASS.into(), MIN_ITERATIONS).unwrap_err(),
            "bad_file"
        );
        assert_eq!(
            fresh.get(&service_for(N8N_KEY)),
            Err(KeychainError::NotFound)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn unlock_gives_one_code_and_refuses_symlinks_and_dirs() {
        let dir = tmp("oracle");
        make_with(&seeded(b"k"), &dir, PASS.into(), FAST).unwrap();
        let real = dir.join(KEY_FILE_NAME);
        let link = dir.join("link.key");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let junk = dir.join("junk.key");
        std::fs::write(&junk, vec![1u8; 80]).unwrap();
        let big = dir.join("big.key");
        std::fs::write(&big, vec![1u8; 5000]).unwrap();
        let fresh = FakeKeychain::default();
        for p in [dir.join("missing.key"), link, sub, junk, big] {
            assert_eq!(
                unlock_with(&fresh, p.to_str().unwrap(), PASS.into()).unwrap_err(),
                "bad_file",
                "{}",
                p.display()
            );
        }
        unlock_with(&fresh, real.to_str().unwrap(), PASS.into()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn make_never_silently_replaces_an_existing_key_file() {
        let dir = tmp("replace");
        make_with(&seeded(b"first"), &dir, PASS.into(), FAST).unwrap();
        let before = std::fs::read(dir.join(KEY_FILE_NAME)).unwrap();
        assert_eq!(
            make_with(&seeded(b"second"), &dir, PASS.into(), FAST).unwrap_err(),
            "exists"
        );
        assert_eq!(std::fs::read(dir.join(KEY_FILE_NAME)).unwrap(), before);
        make_inner(&seeded(b"second"), &dir, PASS.into(), FAST, true).unwrap();
        assert_ne!(std::fs::read(dir.join(KEY_FILE_NAME)).unwrap(), before);
        // No temp files left behind.
        assert!(std::fs::read_dir(&dir).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The packager must never put a key file in an archive, wherever it sits in the data dir.
    #[tokio::test]
    async fn packager_archive_never_contains_the_key_file() {
        let home = tmp("pk-home");
        std::fs::create_dir_all(home.join("Desktop")).unwrap();
        let data = tmp("pk-data");
        std::fs::write(data.join("goals.json"), "[]").unwrap();
        make_with(&seeded(b"k"), &data, PASS.into(), FAST).unwrap();
        make_with(&seeded(b"k"), &data.join("sessions"), PASS.into(), FAST).unwrap();
        let info = super::super::packager::pack_at(&home, &data)
            .await
            .unwrap()
            .path;
        let out = std::process::Command::new("tar")
            .args(["-tzf", &info])
            .output()
            .unwrap();
        let listing = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(listing.contains("goals.json"), "{listing}");
        assert!(!listing.contains(KEY_FILE_NAME), "{listing}");
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&data);
    }
}
