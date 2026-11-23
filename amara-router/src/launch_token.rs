//! Per-launch token check for Amara-owned loopback services (contract: the app's
//! `src-tauri/src/commands/launch_token.rs`).
//!
//! The file holds exactly 64 lowercase hex characters plus `\n`. Clients send it as
//! `X-Amara-Launch-Token`. The file is re-read on every request, so a rotation (the app mints a new
//! token at each start) takes effect without restarting the router. An absent or malformed file
//! rejects everything (fail closed). Never log the token.

use std::path::{Path, PathBuf};

pub const HEADER_NAME: &str = "x-amara-launch-token";
const TOKEN_LEN: usize = 64;
const MAX_READ: u64 = 128;

/// Default token file: `$NEXUS_DATA_DIR/run/launch_token`, else
/// `~/Library/Application Support/com.taisun.nexus/run/launch_token`.
pub fn default_path() -> PathBuf {
    let base = match std::env::var_os("NEXUS_DATA_DIR").filter(|v| !v.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Library/Application Support/com.taisun.nexus"),
    };
    base.join("run").join("launch_token")
}

fn is_valid(t: &str) -> bool {
    t.len() == TOKEN_LEN && t.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The current token, or None when the file is absent or malformed.
pub async fn read(path: &Path) -> Option<String> {
    use tokio::io::AsyncReadExt;
    // Fail closed on a symlink or non-regular file, and never read more than MAX_READ bytes.
    let meta = tokio::fs::symlink_metadata(path).await.ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let mut buf = Vec::with_capacity(MAX_READ as usize);
    tokio::fs::File::open(path)
        .await
        .ok()?
        .take(MAX_READ)
        .read_to_end(&mut buf)
        .await
        .ok()?;
    let raw = String::from_utf8(buf).ok()?;
    let t = raw.trim();
    is_valid(t).then(|| t.to_string())
}

/// True only when the file holds a valid token and `given` equals it (constant time).
pub async fn verify(path: &Path, given: Option<&str>) -> bool {
    let Some(expected) = read(path).await else {
        return false;
    };
    let given = given.unwrap_or("").trim();
    crate::server::constant_time_eq(given.as_bytes(), expected.as_bytes())
}

/// Paths that need the token when enforcement is on: `/amara/*` and `/api/*`. `/v1/*` (Hermes)
/// and `/health` are not covered.
pub fn path_needs_token(path: &str) -> bool {
    ["/amara", "/api"]
        .iter()
        .any(|p| path == *p || path.strip_prefix(p).is_some_and(|r| r.starts_with('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rt-lt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[tokio::test]
    async fn refuses_symlink_and_oversized_files() {
        let d = tmpdir("sym");
        let good = "a".repeat(64);
        std::fs::write(d.join("real"), format!("{good}\n")).unwrap();
        assert_eq!(read(&d.join("real")).await.as_deref(), Some(good.as_str()));
        std::os::unix::fs::symlink(d.join("real"), d.join("link")).unwrap();
        assert_eq!(read(&d.join("link")).await, None);
        // 64 valid chars then padding past the cap: not a valid token file.
        std::fs::write(d.join("big"), format!("{good}{}", " ".repeat(10_000))).unwrap();
        assert_eq!(read(&d.join("big")).await.as_deref(), Some(good.as_str()));
        std::fs::write(d.join("big2"), format!("{}{good}", " ".repeat(200))).unwrap();
        assert_eq!(read(&d.join("big2")).await, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn scope_is_amara_and_api_only() {
        assert!(path_needs_token("/amara/route"));
        assert!(path_needs_token("/api/tags"));
        assert!(!path_needs_token("/v1/chat/completions"));
        assert!(!path_needs_token("/health"));
        assert!(!path_needs_token("/apix"));
    }
}
