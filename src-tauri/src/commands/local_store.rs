//! Small shared helpers for the 1111v local stores (decisions log, life dates, check-ins, add history).
//!
//! Every store is one JSON file under the app data dir: 0600 on creation, written to a temp file in the
//! same folder and renamed over the old one (a crash never leaves half a file), capped in size, and
//! read through `symlink_metadata` so a symlink or an oversized file is refused, not followed.

use chrono::{DateTime, FixedOffset, NaiveDate, SecondsFormat, Utc};
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::Path;

/// Taisun's clock: every stored time and every "today" is JST.
pub(crate) fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("valid offset")
}

pub(crate) fn now_jst() -> DateTime<FixedOffset> {
    Utc::now().with_timezone(&jst())
}

pub(crate) fn iso(t: &DateTime<FixedOffset>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, false)
}

pub(crate) fn day_of(t: &DateTime<FixedOffset>) -> NaiveDate {
    t.date_naive()
}

/// Trimmed text with control characters turned into spaces; `Err` outside `min..=max` characters.
pub(crate) fn clean_text(raw: &str, min: usize, max: usize) -> Result<String, String> {
    let flat: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let text = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = text.chars().count();
    if n < min || n > max {
        return Err("invalid input".to_string());
    }
    Ok(text)
}

/// Short ids only: `[a-z0-9_/-]`, used for route views and venture ids.
pub(crate) fn clean_slug(raw: &str, max: usize) -> Result<String, String> {
    let ok = !raw.is_empty()
        && raw.chars().count() <= max
        && raw
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '/'));
    if ok {
        Ok(raw.to_string())
    } else {
        Err("invalid input".to_string())
    }
}

pub(crate) fn new_id(prefix: &str) -> String {
    let u = uuid::Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &u[..12])
}

/// Reads a JSON store. A missing file is `T::default()`; a symlink, a non-file, an oversized file or bad
/// JSON is an error that never quotes the file's contents.
pub(crate) fn read_json<T: DeserializeOwned + Default>(
    path: &Path,
    max_bytes: u64,
) -> Result<T, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(_) => return Err("store: can't read the file".to_string()),
    };
    if !meta.file_type().is_file() {
        return Err("store: the file must be a regular file (no symlink)".to_string());
    }
    if meta.len() > max_bytes {
        return Err("store: the file is over its size limit".to_string());
    }
    let text = fs::read_to_string(path).map_err(|_| "store: can't read the file".to_string())?;
    serde_json::from_str(&text).map_err(|_| "store: the file is not valid".to_string())
}

/// Temp file + rename, 0600. Refuses a value that would pass `max_bytes`.
pub(crate) fn write_text_atomic(path: &Path, text: &str, max_bytes: u64) -> Result<(), String> {
    if text.len() as u64 > max_bytes {
        return Err("store: full".to_string());
    }
    let dir = path.parent().ok_or("store: bad path")?;
    crate::app_paths::create_private_dir_all(dir)
        .map_err(|_| "store: can't create the folder".to_string())?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("store: bad path")?;
    let tmp = dir.join(format!(".{name}.tmp"));
    if let Err(_e) = crate::app_paths::write_private(&tmp, text) {
        let _ = fs::remove_file(&tmp);
        return Err("store: can't write the file".to_string());
    }
    fs::rename(&tmp, path).map_err(|_| {
        let _ = fs::remove_file(&tmp);
        "store: can't write the file".to_string()
    })
}

pub(crate) fn write_json_atomic<T: Serialize>(
    path: &Path,
    value: &T,
    max_bytes: u64,
) -> Result<(), String> {
    let text =
        serde_json::to_string_pretty(value).map_err(|_| "store: can't encode".to_string())?;
    write_text_atomic(path, &text, max_bytes)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("amara-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn atomic_write_is_0600_capped_and_leaves_no_temp() {
        let d = temp_dir("ls");
        let p = d.join("sub").join("a.json");
        write_json_atomic(&p, &vec![1, 2, 3], 1000).unwrap();
        assert_eq!(read_json::<Vec<i32>>(&p, 1000).unwrap(), vec![1, 2, 3]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(write_json_atomic(&p, &vec![0; 2000], 100).is_err());
        assert_eq!(read_json::<Vec<i32>>(&p, 1000).unwrap(), vec![1, 2, 3]);
        let names: Vec<_> = fs::read_dir(p.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["a.json"]);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn read_refuses_oversize_bad_json_and_symlinks() {
        let d = temp_dir("lsr");
        let p = d.join("a.json");
        assert!(read_json::<Vec<i32>>(&p, 10).unwrap().is_empty());
        fs::write(&p, "not json").unwrap();
        assert!(read_json::<Vec<i32>>(&p, 100).is_err());
        fs::write(&p, "[1,2,3,4,5,6,7,8,9]").unwrap();
        assert!(read_json::<Vec<i32>>(&p, 5).is_err());
        #[cfg(unix)]
        {
            let link = d.join("link.json");
            std::os::unix::fs::symlink(&p, &link).unwrap();
            assert!(read_json::<Vec<i32>>(&link, 1000).is_err());
        }
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn clean_text_flattens_control_characters_and_bounds_length() {
        assert_eq!(clean_text("  a\n\tb  ", 1, 10).unwrap(), "a b");
        assert!(clean_text("   ", 1, 10).is_err());
        assert!(clean_text("abcdef", 1, 5).is_err());
        assert!(clean_slug("ventures/taiyo-skin", 64).is_ok());
        assert!(clean_slug("../x", 64).is_err());
    }
}
