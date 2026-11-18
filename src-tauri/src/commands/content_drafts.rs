//! 0037: save a reviewed Content draft next to its pipeline item. Two commands, local files only.
//!
//! - `content_draft_save {title, text, via?}` writes `<workspace>/drafts/<slug>-<hash>.md` for an item that is
//!   really in `CONTENT-PIPELINE.md` (any stage). Goes through `adds::logged_write`: one History line and 8 s
//!   Undo (`add_undo("content", id)`). Saving posts nothing; it is a file on this Mac.
//! - `content_draft_get {title}` reads it back (`None` when there is none).
//!
//! Rules this file keeps (each has a test):
//! - The file name is built only from the title: ASCII letters and digits, lowercased, other runs become one
//!   `-`, at most 48 characters, plus 8 hex of the title's SHA-256. So `../x`, `/etc` and unicode titles stay
//!   inside `drafts/`, and two titles that slug alike get different files. The path is then joined with
//!   `safe_join`. If an existing file under that name belongs to a different title it is refused.
//! - `drafts/` must be a real folder (no symlink); a symlinked file is never read or replaced.
//! - Text goes through the same strip as a draft proposal (zero-width, bidi and control characters out,
//!   newlines kept). Over 16 KiB after the strip is refused, never cut. Empty is refused.
//! - Atomic write, 0600 (`write_text_atomic`).
//! - Undo restores the previous text (or removes the file it created) only if the file still hashes to what the
//!   save wrote (`content: changed since`).
//! - No network, no process, nothing posted. Never logs titles or draft text.

use super::adds::{self, Dirs, UndoRef};
use super::content_pipeline::{clean_draft, hash_hex, item_exists};
use super::local_store::{clean_text, now_jst, write_text_atomic};
use super::path_safety::safe_join;
use super::shared::task::{blocking, JoinErr};
use chrono::{DateTime, FixedOffset};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const PIPELINE: &str = "CONTENT-PIPELINE.md";
const DIR: &str = "drafts";
/// Largest saved draft (bytes of the text after the strip).
pub(crate) const MAX_SAVED_BYTES: usize = 16 * 1024;
/// Input larger than this is refused before any work.
const MAX_INPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "ContentDraftSaved", rename_all = "camelCase")]
pub struct DraftSaved {
    /// The History id: what `add_undo("content", id)` takes.
    pub id: String,
    pub title: String,
    /// `drafts/<name>.md`, relative to the content workspace.
    pub file: String,
    pub bytes: u32,
    pub toast: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "ContentDraftSavedText", rename_all = "camelCase")]
pub struct DraftText {
    pub title: String,
    pub text: String,
    pub file: String,
}

/// `<slug>-<8 hex>.md`, from the title alone.
fn file_name(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.chars().take(48).collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "draft" } else { slug };
    let h = Sha256::digest(title.as_bytes());
    let hex: String = h.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{slug}-{hex}.md")
}

/// The drafts folder, created 0700 when missing. A symlink or a non-folder there is refused.
fn drafts_dir(dirs: &Dirs, create: bool) -> Result<Option<PathBuf>, String> {
    let dir = dirs.workspace.join(DIR);
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.file_type().is_dir() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                // SAFETY: geteuid has no preconditions and cannot fail.
                let me = unsafe { libc::geteuid() };
                if m.uid() != me || m.mode() & 0o022 != 0 {
                    return Err("content: unsafe storage".into());
                }
                if m.mode() & 0o777 != 0o700 {
                    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                        .map_err(|_| "content: unsafe storage".to_string())?;
                }
            }
            Ok(Some(dir))
        }
        Ok(_) => Err("content: unsafe storage".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !create {
                return Ok(None);
            }
            crate::app_paths::create_private_dir_all(&dir)
                .map_err(|_| "content: can't write the draft".to_string())?;
            Ok(Some(dir))
        }
        Err(_) => Err("content: can't read the draft".into()),
    }
}

fn target(dir: &std::path::Path, name: &str) -> Result<PathBuf, String> {
    safe_join(dir, name).map_err(|_| "content: unsafe storage".to_string())
}

/// One `O_NOFOLLOW` open, then fstat and a capped read on that same handle. `None` when the file is not there;
/// a symlink, a non-file or an oversized file is `content: unsafe storage`.
fn read_nofollow(path: &std::path::Path) -> Result<Option<String>, String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let cap = (MAX_SAVED_BYTES * 2) as u64;
    let f = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("content: unsafe storage".into()),
    };
    let m = f
        .metadata()
        .map_err(|_| "content: can't read the draft".to_string())?;
    if !m.file_type().is_file() || m.len() > cap {
        return Err("content: unsafe storage".into());
    }
    let mut out = String::new();
    f.take(cap)
        .read_to_string(&mut out)
        .map_err(|_| "content: can't read the draft".to_string())?;
    Ok(Some(out))
}

fn body(title: &str, text: &str) -> String {
    format!("# {title}\n\n{text}\n")
}

/// The text of a file this module wrote for `title`; `None` if the header is a different title's.
fn text_of(file: &str, title: &str) -> Option<String> {
    let rest = file.strip_prefix(&format!("# {title}\n\n"))?;
    Some(rest.strip_suffix('\n').unwrap_or(rest).to_string())
}

pub(crate) fn save_in(
    dirs: &Dirs,
    title: &str,
    text: &str,
    via: Option<&str>,
    now: DateTime<FixedOffset>,
) -> Result<DraftSaved, String> {
    let title = clean_text(title, 1, 160).map_err(|_| "content: invalid input".to_string())?;
    if text.len() > MAX_INPUT_BYTES {
        return Err("content: too large".into());
    }
    let text = clean_draft(text, MAX_INPUT_BYTES);
    if text.is_empty() {
        return Err("content: invalid input".into());
    }
    if text.len() > MAX_SAVED_BYTES {
        return Err("content: too large".into());
    }
    let name = file_name(&title);
    let history = format!("Saved a draft · {title}");
    let (id, bytes) = adds::logged_write("content", history, via, dirs, now, || {
        let md = dirs.md_source(PIPELINE)?;
        if !item_exists(&md, &title) {
            return Err("content: not found".to_string());
        }
        let dir = drafts_dir(dirs, true)?.ok_or("content: can't write the draft")?;
        let path = target(&dir, &name)?;
        let prev = match read_nofollow(&path)? {
            Some(old) => {
                if text_of(&old, &title).is_none() {
                    // Another title's file (a hash clash) or one edited by hand: never overwritten.
                    return Err("content: name taken".to_string());
                }
                Some(old)
            }
            None => None,
        };
        let next = body(&title, &text);
        write_text_atomic(&path, &next, (MAX_SAVED_BYTES * 2) as u64)?;
        Ok((
            next.len() as u32,
            UndoRef::Draft {
                file: name.clone(),
                had_prev: prev.is_some(),
                prev,
                after_hash: hash_hex(&next),
            },
        ))
    })?;
    Ok(DraftSaved {
        id,
        title,
        file: format!("{DIR}/{name}"),
        bytes,
        toast: "Saved on this Mac. Undo is open for a few seconds.".into(),
    })
}

pub(crate) fn get_in(dirs: &Dirs, title: &str) -> Result<Option<DraftText>, String> {
    let title = clean_text(title, 1, 160).map_err(|_| "content: invalid input".to_string())?;
    let Some(dir) = drafts_dir(dirs, false)? else {
        return Ok(None);
    };
    let name = file_name(&title);
    let path = target(&dir, &name)?;
    let Some(raw) = read_nofollow(&path)? else {
        return Ok(None);
    };
    Ok(text_of(&raw, &title).map(|text| DraftText {
        title,
        text,
        file: format!("{DIR}/{name}"),
    }))
}

/// Undo of a save: the previous text back, or the new file removed; only if the file is as the save left it.
pub(crate) fn revert(
    dirs: &Dirs,
    file: &str,
    prev: Option<&str>,
    had_prev: bool,
    after_hash: &str,
) -> Result<(), String> {
    let dir = drafts_dir(dirs, false)?.ok_or("content: not found")?;
    let path = target(&dir, file)?;
    let now_text = read_nofollow(&path)?.ok_or("content: not found")?;
    if hash_hex(&now_text) != after_hash {
        return Err("content: changed since".into());
    }
    if had_prev {
        let prev = prev.ok_or("content: expired")?;
        // Only text this module could have written for this file goes back.
        let title = prev
            .strip_prefix("# ")
            .and_then(|r| r.split_once("\n\n"))
            .map(|(t, _)| t)
            .ok_or("content: expired")?;
        let ok = file_name(title) == file
            && text_of(prev, title).is_some_and(|t| t.len() <= MAX_SAVED_BYTES);
        if !ok {
            return Err("content: expired".into());
        }
        write_text_atomic(&path, prev, (MAX_SAVED_BYTES * 2) as u64)
    } else {
        std::fs::remove_file(&path).map_err(|_| "content: can't write the draft".to_string())
    }
}

/// Save a reviewed draft for a pipeline item. Local file, History + 8 s Undo. Posts nothing.
#[tauri::command]
#[specta::specta]
pub async fn content_draft_save(
    title: String,
    text: String,
    via: Option<String>,
) -> Result<DraftSaved, String> {
    blocking(JoinErr::Plain, move || {
        save_in(&Dirs::real(), &title, &text, via.as_deref(), now_jst())
    })
    .await
}

/// The saved draft for a title, or `None`.
#[tauri::command]
#[specta::specta]
pub async fn content_draft_get(title: String) -> Result<Option<DraftText>, String> {
    blocking(JoinErr::Plain, move || get_in(&Dirs::real(), &title)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::local_store::tests::temp_dir;

    const MD: &str = "# Content\n## Ideas\n- Clothing store like Uniqlo #type:post\n- ../x\n- /etc\n- 日本語の下書き\n- A B\n- a-b\n## Published\n- Done thing\n";

    fn dirs() -> Dirs {
        let root = temp_dir("content-drafts");
        let d = Dirs {
            data: root.join("data"),
            workspace: root.join("ws"),
            legacy: root.join("legacy"),
        };
        std::fs::create_dir_all(&d.data).unwrap();
        std::fs::create_dir_all(&d.workspace).unwrap();
        std::fs::write(d.workspace.join(PIPELINE), MD).unwrap();
        d
    }

    fn files(d: &Dirs) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d.workspace.join(DIR))
            .map(|r| {
                r.map(|e| e.unwrap().file_name().into_string().unwrap())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn save_get_and_undo_within_8_seconds() {
        let d = dirs();
        let now = now_jst();
        let t = "Clothing store like Uniqlo";
        assert_eq!(get_in(&d, t).unwrap(), None);
        let r = save_in(&d, t, "First line\n\nSecond line", Some("typed"), now).unwrap();
        assert!(
            r.file.starts_with("drafts/clothing-store-like-uniqlo-") && r.file.ends_with(".md")
        );
        let got = get_in(&d, t).unwrap().unwrap();
        assert_eq!(got.text, "First line\n\nSecond line");
        let h = adds::history_in("content", &d).unwrap();
        assert_eq!((h.len(), h[0].id.as_str()), (1, r.id.as_str()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let p = d.workspace.join(&r.file);
            assert_eq!(
                std::fs::metadata(p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // Undo of the first save removes the file.
        assert!(adds::undo_in("content", &r.id, &d, now + chrono::Duration::seconds(3)).unwrap());
        assert_eq!(get_in(&d, t).unwrap(), None);
        assert!(files(&d).is_empty());
        // A second save over an earlier one: Undo brings the earlier text back.
        save_in(&d, t, "Old", None, now).unwrap();
        let r2 = save_in(&d, t, "New", None, now).unwrap();
        assert_eq!(get_in(&d, t).unwrap().unwrap().text, "New");
        assert!(adds::undo_in("content", &r2.id, &d, now + chrono::Duration::seconds(2)).unwrap());
        assert_eq!(get_in(&d, t).unwrap().unwrap().text, "Old");
        assert_eq!(files(&d).len(), 1);
    }

    #[test]
    fn undo_is_closed_after_8_seconds_and_refuses_after_a_change() {
        let d = dirs();
        let now = now_jst();
        let t = "Clothing store like Uniqlo";
        let r = save_in(&d, t, "Text", None, now).unwrap();
        assert!(!adds::undo_in("content", &r.id, &d, now + chrono::Duration::seconds(9)).unwrap());
        assert_eq!(get_in(&d, t).unwrap().unwrap().text, "Text");
        let r = save_in(&d, t, "Text two", None, now).unwrap();
        let p = d.workspace.join(&r.file);
        let edited = format!("{}more\n", std::fs::read_to_string(&p).unwrap());
        std::fs::write(&p, &edited).unwrap();
        let e =
            adds::undo_in("content", &r.id, &d, now + chrono::Duration::seconds(2)).unwrap_err();
        assert_eq!(e, "content: changed since");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), edited);
    }

    #[test]
    fn only_a_real_pipeline_item_can_be_saved_and_nothing_is_written_otherwise() {
        let d = dirs();
        let e = save_in(&d, "Not in the file", "Text", None, now_jst()).unwrap_err();
        assert_eq!(e, "content: not found");
        assert!(files(&d).is_empty());
        assert!(adds::history_in("content", &d).unwrap().is_empty());
        // Any stage counts, Published included.
        save_in(&d, "Done thing", "Text", None, now_jst()).unwrap();
        // Empty text and an empty title are refused.
        assert!(save_in(&d, "Done thing", " \u{200B}\n ", None, now_jst()).is_err());
        assert!(save_in(&d, "  ", "Text", None, now_jst()).is_err());
    }

    #[test]
    fn odd_titles_stay_inside_drafts_and_never_collide() {
        let d = dirs();
        let now = now_jst();
        for t in ["../x", "/etc", "日本語の下書き", "A B", "a-b"] {
            let r = save_in(&d, t, "Body", None, now).unwrap();
            let name = r.file.strip_prefix("drafts/").unwrap();
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.'),
                "{name}"
            );
            assert!(!name.contains(".."));
            assert!(d.workspace.join(DIR).join(name).is_file());
        }
        // "A B" and "a-b" slug alike but are separate files; nothing escaped the folder.
        assert_eq!(files(&d).len(), 5);
        assert_eq!(get_in(&d, "A B").unwrap().unwrap().title, "A B");
        assert_eq!(get_in(&d, "a-b").unwrap().unwrap().title, "a-b");
        assert!(
            !d.workspace.join("x").exists() && !d.workspace.parent().unwrap().join("x").exists()
        );
        let in_ws: Vec<_> = std::fs::read_dir(&d.workspace)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(in_ws.iter().all(|n| n == DIR || n == PIPELINE), "{in_ws:?}");
        // A file that belongs to another title is not overwritten.
        let p = d.workspace.join(DIR).join(file_name("A B"));
        std::fs::write(&p, "# someone else\n\ntext\n").unwrap();
        assert_eq!(
            save_in(&d, "A B", "Body", None, now).unwrap_err(),
            "content: name taken"
        );
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "# someone else\n\ntext\n"
        );
    }

    #[test]
    fn text_is_sanitised_like_a_proposal_and_capped_at_16_kib() {
        let d = dirs();
        let t = "Clothing store like Uniqlo";
        save_in(
            &d,
            t,
            "Hi\u{200B} there\u{202E}\u{7}!\n\n\n\nSecond\u{2066} line",
            None,
            now_jst(),
        )
        .unwrap();
        assert_eq!(
            get_in(&d, t).unwrap().unwrap().text,
            "Hi there !\n\nSecond line"
        );
        let ok = "x".repeat(MAX_SAVED_BYTES);
        assert!(save_in(&d, t, &ok, None, now_jst()).is_ok());
        let big = "x".repeat(MAX_SAVED_BYTES + 1);
        assert_eq!(
            save_in(&d, t, &big, None, now_jst()).unwrap_err(),
            "content: too large"
        );
        let huge = "x".repeat(MAX_INPUT_BYTES + 1);
        assert_eq!(
            save_in(&d, t, &huge, None, now_jst()).unwrap_err(),
            "content: too large"
        );
        // The refused save left the earlier file alone.
        assert_eq!(get_in(&d, t).unwrap().unwrap().text.len(), MAX_SAVED_BYTES);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_folder_or_file_is_refused() {
        let d = dirs();
        let outside = temp_dir("content-drafts-out");
        std::os::unix::fs::symlink(&outside, d.workspace.join(DIR)).unwrap();
        let t = "Clothing store like Uniqlo";
        assert_eq!(
            save_in(&d, t, "Body", None, now_jst()).unwrap_err(),
            "content: unsafe storage"
        );
        assert!(get_in(&d, t).is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        std::fs::remove_file(d.workspace.join(DIR)).unwrap();
        // A symlink in place of the draft file.
        std::fs::create_dir_all(d.workspace.join(DIR)).unwrap();
        let victim = outside.join("victim.md");
        std::fs::write(&victim, "keep").unwrap();
        std::os::unix::fs::symlink(&victim, d.workspace.join(DIR).join(file_name(t))).unwrap();
        assert!(save_in(&d, t, "Body", None, now_jst()).is_err());
        assert!(get_in(&d, t).is_err());
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    }

    fn hist_prev(d: &Dirs) -> serde_json::Value {
        let v: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(d.data.join("adds").join("history.json")).unwrap(),
        )
        .unwrap();
        v["entries"][0]["undo"]["prev"].clone()
    }

    #[test]
    fn expired_undo_text_is_trimmed_on_read_and_at_startup() {
        let d = dirs();
        let t = "Clothing store like Uniqlo";
        let old = now_jst() - chrono::Duration::seconds(60);
        save_in(&d, t, "Old", None, old).unwrap();
        save_in(&d, t, "New", None, old).unwrap();
        assert!(hist_prev(&d).is_string(), "kept until something reads it");
        adds::history_in("content", &d).unwrap();
        assert!(hist_prev(&d).is_null(), "a read drops the expired text");
        // Startup pass does the same, and writes nothing when there is nothing to drop.
        save_in(&d, t, "Newer", None, old).unwrap();
        assert!(hist_prev(&d).is_string());
        adds::trim_expired_drafts_in(&d, now_jst());
        assert!(hist_prev(&d).is_null());
        // A fresh entry keeps its text for Undo.
        save_in(&d, t, "Fresh", None, now_jst()).unwrap();
        adds::history_in("content", &d).unwrap();
        assert!(hist_prev(&d).is_string());
    }

    #[cfg(unix)]
    #[test]
    fn drafts_folder_that_is_writable_by_others_is_refused_and_loose_modes_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let d = dirs();
        let t = "Clothing store like Uniqlo";
        let dir = d.workspace.join(DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            save_in(&d, t, "Body", None, now_jst()).unwrap_err(),
            "content: unsafe storage"
        );
        assert!(get_in(&d, t).is_err());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        save_in(&d, t, "Body", None, now_jst()).unwrap();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn undo_never_writes_back_text_that_is_not_this_files_draft() {
        let d = dirs();
        let t = "Clothing store like Uniqlo";
        let r = save_in(&d, t, "Text", None, now_jst()).unwrap();
        let name = r.file.strip_prefix("drafts/").unwrap();
        let after = hash_hex(&std::fs::read_to_string(d.workspace.join(&r.file)).unwrap());
        for bad in [
            "not a draft".to_string(),
            "# Other title\n\nText\n".to_string(),
            format!("# {t}\n\n{}\n", "x".repeat(MAX_SAVED_BYTES + 1)),
        ] {
            let e = revert(&d, name, Some(&bad), true, &after).unwrap_err();
            assert_eq!(e, "content: expired");
        }
        assert_eq!(get_in(&d, t).unwrap().unwrap().text, "Text");
        revert(&d, name, Some(&format!("# {t}\n\nOlder\n")), true, &after).unwrap();
        assert_eq!(get_in(&d, t).unwrap().unwrap().text, "Older");
    }

    fn production(src: &str) -> &str {
        &src[..src.find("#[cfg(test)]").unwrap()]
    }

    #[test]
    fn there_is_no_posting_path_and_no_outside_call() {
        let src = production(include_str!("content_drafts.rs"));
        for needle in [
            "reqwest",
            "hyper",
            "TcpStream",
            "Command::new",
            "http://",
            "https://",
            "smtp",
            "telegram",
            "notify_relay",
            "oauth",
            "instagram",
            "tiktok",
            "api_key",
            "llm_router",
        ] {
            assert!(
                !src.to_lowercase().contains(needle),
                "production source contains {needle}"
            );
        }
        for name in ["content_draft_save", "content_draft_get"] {
            assert!(src.contains(&format!("pub async fn {name}")));
            for w in [
                "post", "publish", "send", "tweet", "share", "submit", "schedule",
            ] {
                assert!(!name.contains(w), "{name} contains {w}");
            }
        }
    }
}
