//! First-run flag (Phase 9 F3): `<data_dir>/setup.json` `{done_at, version, tier}`.
//!
//! Separate from the older `onboarding.json` flow, which this module never reads or writes.
//! `setup_reset` deletes only `setup.json`. Core logic is pure over an injected dir.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SETUP_FILE: &str = "setup.json";
const MAX_BYTES: u64 = 4096;
const TIERS: [&str; 3] = ["compact", "standard", "large"];
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize)]
struct SetupFile {
    done_at: String,
    version: String,
    tier: String,
}

fn setup_path(dir: &Path) -> PathBuf {
    dir.join(SETUP_FILE)
}

/// True only for a regular, non-symlink, <= 4 KiB, well-formed setup.json with a valid tier.
fn is_done_in(dir: &Path) -> bool {
    let mut o = fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    let Ok(f) = o.open(setup_path(dir)) else {
        return false;
    };
    match f.metadata() {
        Ok(m) if m.is_file() && m.len() <= MAX_BYTES => {}
        _ => return false,
    }
    let mut buf = Vec::new();
    if f.take(MAX_BYTES + 1).read_to_end(&mut buf).is_err() || buf.len() as u64 > MAX_BYTES {
        return false;
    }
    match serde_json::from_slice::<SetupFile>(&buf) {
        Ok(s) => !s.done_at.is_empty() && TIERS.contains(&s.tier.as_str()),
        Err(_) => false,
    }
}

fn complete_in(dir: &Path, tier: &str) -> Result<(), String> {
    if !TIERS.contains(&tier) {
        return Err("invalid_tier".into());
    }
    let body = serde_json::to_vec_pretty(&SetupFile {
        done_at: chrono::Local::now().to_rfc3339(),
        version: APP_VERSION.into(),
        tier: tier.into(),
    })
    .map_err(|_| "io_error".to_string())?;
    crate::app_paths::create_private_dir_all(dir).map_err(|_| "io_error".to_string())?;
    let tmp = dir.join(format!(".setup.json.{}.tmp", uuid::Uuid::new_v4().simple()));
    let res = (|| -> std::io::Result<()> {
        let mut o = crate::app_paths::private_open_options();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.custom_flags(libc::O_NOFOLLOW);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(&body)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, setup_path(dir))?;
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res.map_err(|_| "io_error".to_string())
}

fn reset_in(dir: &Path) -> Result<(), String> {
    match fs::remove_file(setup_path(dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("io_error".into()),
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce(PathBuf) -> T + Send + 'static,
) -> Result<T, String> {
    let dir = crate::app_paths::data_dir();
    tokio::task::spawn_blocking(move || f(dir))
        .await
        .map_err(|_| "io_error".to_string())
}

/// Existing installs without setup.json report false (first run shows once).
#[tauri::command]
#[specta::specta]
pub async fn setup_done() -> Result<bool, String> {
    blocking(|d| is_done_in(&d)).await
}

#[tauri::command]
#[specta::specta]
pub async fn setup_complete(tier: String) -> Result<(), String> {
    blocking(move |d| complete_in(&d, &tier)).await?
}

#[tauri::command]
#[specta::specta]
pub async fn setup_reset() -> Result<(), String> {
    blocking(|d| reset_in(&d)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-setup-state-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn missing_malformed_oversize_false() {
        let d = tmp("bad");
        assert!(!is_done_in(&d));
        fs::write(d.join(SETUP_FILE), "{nope").unwrap();
        assert!(!is_done_in(&d));
        fs::write(
            d.join(SETUP_FILE),
            r#"{"done_at":"x","version":"1","tier":"huge"}"#,
        )
        .unwrap();
        assert!(!is_done_in(&d));
        let big = format!(
            r#"{{"done_at":"x","version":"1","tier":"large","pad":"{}"}}"#,
            "a".repeat(5000)
        );
        fs::write(d.join(SETUP_FILE), big).unwrap();
        assert!(!is_done_in(&d));
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_false() {
        let d = tmp("sym");
        let target = d.join("real.json");
        fs::write(
            &target,
            r#"{"done_at":"2026-01-01T00:00:00+00:00","version":"1","tier":"large"}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&target, d.join(SETUP_FILE)).unwrap();
        assert!(!is_done_in(&d));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn complete_then_done_and_mode() {
        let d = tmp("ok");
        for t in TIERS {
            complete_in(&d, t).unwrap();
            assert!(is_done_in(&d));
        }
        let v: SetupFile = serde_json::from_slice(&fs::read(d.join(SETUP_FILE)).unwrap()).unwrap();
        assert_eq!(v.version, APP_VERSION);
        assert!(chrono::DateTime::parse_from_rfc3339(&v.done_at).is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = fs::metadata(d.join(SETUP_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(m & 0o777, 0o600);
        }
        let leftovers: Vec<_> = fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn bad_tier_refused() {
        let d = tmp("tier");
        assert_eq!(complete_in(&d, "Huge").unwrap_err(), "invalid_tier");
        assert_eq!(complete_in(&d, "").unwrap_err(), "invalid_tier");
        assert!(!d.join(SETUP_FILE).exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn reset_removes_only_setup_json() {
        let d = tmp("reset");
        reset_in(&d).unwrap(); // missing is ok
        complete_in(&d, "standard").unwrap();
        fs::write(d.join("onboarding.json"), "{}").unwrap();
        fs::create_dir_all(d.join("models")).unwrap();
        fs::write(d.join("models/blob"), "x").unwrap();
        reset_in(&d).unwrap();
        assert!(!is_done_in(&d));
        assert!(!d.join(SETUP_FILE).exists());
        assert!(d.join("onboarding.json").exists());
        assert!(d.join("models/blob").exists());
        let _ = fs::remove_dir_all(&d);
    }
}
