//! Shared helpers for turning frontend-supplied names into file paths safely.
//!
//! Rules: a *name* may contain only `[A-Za-z0-9_-]` (1-128 chars). A *file name*
//! is a name plus one allowed extension. Joined paths are canonicalized and must
//! stay inside the (canonicalized) base directory, which also defeats symlinks
//! that point outside it.

use std::path::{Path, PathBuf};

const MAX_NAME_LEN: usize = 128;

/// Validate a bare name (no extension, no separators, no dots).
pub fn validate_name(name: &str) -> Result<&str, String> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err("Invalid name: must be 1-128 characters".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Invalid name: only letters, digits, '_' and '-' are allowed".to_string());
    }
    Ok(name)
}

/// Validate `stem.ext` where `stem` passes [`validate_name`] and `ext` is in `allowed_exts`.
pub fn validate_file_name<'a>(
    file_name: &'a str,
    allowed_exts: &[&str],
) -> Result<&'a str, String> {
    let (stem, ext) = file_name
        .rsplit_once('.')
        .ok_or_else(|| "Invalid file name: missing extension".to_string())?;
    validate_name(stem)?;
    if !allowed_exts.iter().any(|a| a.eq_ignore_ascii_case(ext)) {
        return Err(format!("Invalid file name: extension '.{ext}' not allowed"));
    }
    Ok(file_name)
}

/// Join an already-validated file name onto `base` and verify the result
/// stays inside `base` after canonicalization. `base` is created if missing.
/// The target itself need not exist.
pub fn safe_join(base: &Path, file_name: &str) -> Result<PathBuf, String> {
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name == "."
        || file_name == ".."
    {
        return Err("Invalid file name".to_string());
    }
    std::fs::create_dir_all(base).map_err(|e| format!("Failed to create directory: {e}"))?;
    let base_canon = base
        .canonicalize()
        .map_err(|e| format!("Failed to resolve directory: {e}"))?;
    let candidate = base_canon.join(file_name);
    // If the target exists (possibly a symlink), resolve it and re-check.
    let resolved = if candidate.symlink_metadata().is_ok() {
        candidate
            .canonicalize()
            .map_err(|e| format!("Failed to resolve path: {e}"))?
    } else {
        candidate
    };
    if resolved.parent() != Some(base_canon.as_path()) {
        return Err("Path escapes its base directory".to_string());
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(validate_name("daily-brief_2").is_ok());
        for bad in [
            "",
            "..",
            "a/b",
            "a\\b",
            "a.b",
            "a b",
            "../etc",
            "é",
            &"x".repeat(129),
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn file_names() {
        assert!(validate_file_name("eeg_20261021_180000.ndjson", &["ndjson"]).is_ok());
        assert!(validate_file_name("a.json", &["ndjson"]).is_err());
        assert!(validate_file_name("../x.ndjson", &["ndjson"]).is_err());
        assert!(validate_file_name("x", &["ndjson"]).is_err());
        assert!(validate_file_name("a.b.ndjson", &["ndjson"]).is_err());
    }

    #[test]
    fn join_stays_inside_base() {
        let base = std::env::temp_dir().join(format!("nexus-ps-{}", uuid::Uuid::new_v4().simple()));
        let p = safe_join(&base, "ok.json").unwrap();
        assert!(p.starts_with(base.canonicalize().unwrap()));
        assert!(safe_join(&base, "../x").is_err());
        assert!(safe_join(&base, "..").is_err());
        assert!(safe_join(&base, "/etc/passwd").is_err());
        #[cfg(unix)]
        {
            let outside = std::env::temp_dir()
                .join(format!("nexus-ps-out-{}", uuid::Uuid::new_v4().simple()));
            std::fs::write(&outside, b"x").unwrap();
            std::os::unix::fs::symlink(&outside, base.join("link.json")).unwrap();
            assert!(
                safe_join(&base, "link.json").is_err(),
                "symlink escape must be rejected"
            );
            let _ = std::fs::remove_file(&outside);
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
