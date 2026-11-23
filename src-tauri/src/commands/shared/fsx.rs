//! One atomic file write. Moved verbatim from `continuity.rs` (its tests stay there and still cover
//! it through the re-export).

use std::fs;

/// Write `bytes` to `path` atomically: write a uniquely named temp file in the
/// same directory, fsync it, then rename over the target (and fsync the dir on
/// Unix). A crash mid-write leaves either the old file or the new one, never a
/// truncated mix.
pub(crate) fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    atomic_write_with_mode(path, bytes, None)
}

/// Like [`atomic_write`], but the result always has exactly `mode` (Unix)
/// instead of inheriting the original file's mode. Use for files holding
/// secrets, e.g. `atomic_write_with_mode(p, b, Some(0o600))`.
pub(crate) fn atomic_write_with_mode(
    path: &std::path::Path,
    bytes: &[u8],
    forced_mode: Option<u32>,
) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")
        })?
        .to_string_lossy()
        .to_string();
    // Refuse to replace a symlink (it could point anywhere); callers must
    // resolve and validate real paths themselves.
    if fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to replace symlink {}", path.display()),
        ));
    }
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        file_name,
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        // Never widen permissions: create the temp file 0600, then match the
        // original's mode (if one exists) before it replaces the target.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = match forced_mode {
                Some(m) => m & 0o777,
                None => fs::metadata(path)
                    // Only rwx bits: never carry setuid/setgid/sticky over.
                    .map(|m| m.permissions().mode() & 0o777)
                    .unwrap_or(0o600),
            };
            f.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        #[cfg(unix)]
        {
            if let Ok(d) = fs::File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
