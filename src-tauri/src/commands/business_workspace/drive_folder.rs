//! Business documents from the Google Drive folder synced to this Mac (decision 0064). Read-only,
//! no network: Drive for Desktop keeps real files on disk.
//!
//! - Regular files only (never through a symlink), at most 4 folders deep, 300 files, 10 MB each.
//! - PDF, Markdown, text, HTML, CSV and images are copied in as `unverified` documents with their
//!   relative path as the source. A changed file replaces its unverified copy; an approved, draft or
//!   archived copy is never changed (a conflict is counted instead).
//! - Google Docs, Sheets and Slides exist on the Mac only as small stub files that hold the account
//!   email, so the stubs are never stored. Their names are listed in one unverified item, saying their
//!   text needs Drive access to read.

use super::sync_local::{apply, Extracted, ExtractedDoc, ExtractedItem};
use super::Tenant;
use std::path::Path;

const MAX_DEPTH: usize = 4;
const MAX_FILES: usize = 300;
const COPY_EXT: &[&str] = &[
    "pdf", "md", "markdown", "txt", "csv", "html", "htm", "png", "jpg", "jpeg", "webp",
];
const GOOGLE_STUBS: &[(&str, &str)] = &[
    ("gdoc", "Google Doc"),
    ("gsheet", "Google Sheet"),
    ("gslides", "Google Slides"),
];

pub(crate) fn extract(root: &Path) -> Extracted {
    let mut x = Extracted::default();
    let mut stubs: Vec<String> = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut files = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            x.skipped.push(format!(
                "{}: can't be read",
                dir.strip_prefix(root).unwrap_or(&dir).display()
            ));
            continue;
        };
        let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || ft.is_symlink() {
                continue;
            }
            let path = e.path();
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            if ft.is_dir() {
                if depth + 1 < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let ext = name
                .rsplit_once('.')
                .map(|(_, e)| e.to_ascii_lowercase())
                .unwrap_or_default();
            if let Some((_, kind)) = GOOGLE_STUBS.iter().find(|(e, _)| *e == ext) {
                stubs.push(format!(
                    "- {} ({kind})",
                    rel.trim_end_matches(&format!(".{ext}"))
                ));
                continue;
            }
            if !COPY_EXT.contains(&ext.as_str()) {
                continue;
            }
            files += 1;
            if files > MAX_FILES {
                x.skipped.push(format!(
                    "more than {MAX_FILES} files; the rest were not read"
                ));
                break;
            }
            match std::fs::metadata(&path) {
                Ok(m) if m.len() as usize > super::MAX_DOCUMENT_BYTES => {
                    x.skipped.push(format!("{rel}: over 10 MB"))
                }
                Ok(m) if m.len() == 0 => {}
                Ok(_) => match std::fs::read(&path) {
                    Ok(bytes) => x.docs.push(ExtractedDoc {
                        title: rel.clone(),
                        file_name: name.clone(),
                        bytes,
                        source_ref: format!("drive:{rel}"),
                    }),
                    Err(_) => x.skipped.push(format!("{rel}: can't be read")),
                },
                Err(_) => x.skipped.push(format!("{rel}: can't be read")),
            }
        }
    }
    if !stubs.is_empty() {
        stubs.sort();
        x.items.push(ExtractedItem {
            section: "documents",
            title: "Google Docs, Sheets and Slides in Drive (names only)".into(),
            body: format!(
                "These are in the Drive folder, but their text isn't stored on this Mac, so it can't be read without Drive access:\n\n{}",
                stubs.join("\n")
            ),
            source_ref: "drive:google-files".into(),
        });
    }
    x
}

pub(crate) fn sync(t: &Tenant, run_id: &str) -> Result<(i64, i64, i64, String), String> {
    let _ = run_id;
    let path = t
        .connector_config_value("drive_folder", "path")?
        .ok_or("Choose a Drive folder first.")?;
    super::connectors::check_folder(&path, None)?;
    let x = extract(Path::new(&path));
    t.set_quiet(true);
    let applied = apply(t, &x);
    t.set_quiet(false);
    let a = applied?;
    let mut d = format!("{} found, {} added or updated", a.seen, a.changed);
    if a.conflicts > 0 {
        d.push_str(&format!(
            ", {} differ from approved or draft material and were left as they are",
            a.conflicts
        ));
    }
    if !x.skipped.is_empty() {
        d.push_str(&format!(". Not read: {}", x.skipped.join("; ")));
    }
    Ok((a.seen, a.changed, a.conflicts, d))
}
