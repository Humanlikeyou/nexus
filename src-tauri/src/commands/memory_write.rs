//! Settings > Memory (write side): edit, clear and undo for Amara's Letta core-memory blocks.
//!
//! Rules this module keeps (decision 0015: writes on Letta 0.16.8, one reviewed command):
//! - Methods: GET (read the block) and PATCH (set its value) only. Nothing here builds DELETE,
//!   POST or PUT; a test drives every flow against a fake server and asserts it, and another test
//!   scans this file's non-test source for those builders. A block is never deleted, because agents
//!   depend on their core blocks: "delete" clears the value (after a native approval dialog).
//! - Loopback only, same client as `memory_read` (no redirects, no system proxy), same timeouts.
//! - Every change writes a local backup first, `<app data>/memory-backups/<stamp>-<block id>.json`
//!   (dir 0700, file 0600, atomic). If the backup fails, nothing is sent. `memory_restore_last` puts
//!   the newest backup's old value back, writing its own backup of the current value first.
//! - Backups hold memory text only: no key, no URL, no header. Packaged backups copy the app data
//!   dir, so they include this folder (memory text, no secrets; names contain only the block id).
//! - Edit, clear and undo all need a native approval dialog (Rust-side, so a compromised webview
//!   can't skip it). It shows the old and new text as "Memory text (untrusted):", each flattened
//!   (no control characters or newlines) and cut to about 200 characters. One global lock is taken
//!   before the first read and held to the end, so only one memory dialog or write exists at a time.
//!   After approval the block is read again; if it changed meanwhile the result is `changed` and
//!   nothing is written, and the backup holds the re-read value.
//! - A PATCH that may or may not have landed (send error, timeout, unreadable or unechoed reply)
//!   is `letta_unconfirmed`, never a success or a plain failure. Each request has its own 3 s
//!   timeout; there is no outer timeout because the approval dialog waits for Taisun.
//! - The `pg_dump` of `letta_db` before the first real delete is a manual Taisun step (decision
//!   0015); the local JSON backups cover one block, not a database restore.
//! - The Letta password comes from the Keychain through `memory_read::read_key`, never appears in
//!   an error, and errors are fixed codes (`letta_*`, or `cancelled`).
//!
//! API: `GET /v1/blocks/{id}` and `PATCH /v1/blocks/{id}` (body `{"value": ...}`), verified against
//! the 0.16.4 server source in ~/projects/terrance-integrations/MemGPT (`routers/v1/blocks.py`,
//! `modify_block`, `BlockUpdate.value`). Not verified against a running 0.16.8 server.

use std::future::Future;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::keychain::MacKeychain;
use super::local_store::{now_jst, read_json, write_json_atomic};
use super::memory_read::{
    build_client, list_from, read_json_body, read_key, safe_id, MemoryError, LETTA_BASE,
    MAX_VALUE_CHARS, REQUEST_TIMEOUT,
};
use super::{approval_gate, local_store};

/// Longest value a restore may put back (an old block may predate the 2000 cap).
const RESTORE_MAX_CHARS: usize = 100_000;
const MAX_BACKUP_BYTES: u64 = 1024 * 1024;
const AGENT_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
/// `%Y%m%dT%H%M%S%3f`: fixed width, so names sort by time.
const STAMP_LEN: usize = 18;
const PREVIEW_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteError {
    Mem(MemoryError),
    BadInput,
    TooLong,
    BlockTooLong,
    BackupFailed,
    NoBackup,
    Cancelled,
    Changed,
    Busy,
    Unconfirmed,
}

impl From<MemoryError> for WriteError {
    fn from(e: MemoryError) -> Self {
        Self::Mem(e)
    }
}

impl WriteError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Mem(e) => e.code(),
            Self::BadInput => "letta_bad_input",
            Self::TooLong => "letta_value_too_long",
            Self::BlockTooLong => "letta_block_too_long",
            Self::BackupFailed => "letta_backup_failed",
            Self::NoBackup => "letta_no_backup",
            Self::Cancelled => "cancelled",
            Self::Changed => "changed",
            Self::Busy => "letta_busy",
            Self::Unconfirmed => "letta_unconfirmed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub(crate) struct Backup {
    pub block_id: String,
    pub label: String,
    pub agent: String,
    pub old_value: String,
    /// "edit" | "delete" | "restore"
    pub action: String,
    pub at: String,
}

fn backup_dir() -> PathBuf {
    crate::app_paths::data_dir().join("memory-backups")
}

/// Writes the backup. Refuses to overwrite an existing file. Any failure is `BackupFailed`.
fn write_backup(dir: &Path, b: &Backup) -> Result<(), WriteError> {
    crate::app_paths::create_private_dir_all(dir).map_err(|_| WriteError::BackupFailed)?;
    let stamp = now_jst().format("%Y%m%dT%H%M%S%3f").to_string();
    let path = dir.join(format!("{stamp}-{}.json", b.block_id));
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(WriteError::BackupFailed);
    }
    write_json_atomic(&path, b, MAX_BACKUP_BYTES).map_err(|_| WriteError::BackupFailed)
}

/// Newest backup for this block, or `None`.
fn latest_backup(dir: &Path, id: &str) -> Option<Backup> {
    let suffix = format!("-{id}.json");
    let best = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            n.len() == STAMP_LEN + suffix.len()
                && n.ends_with(&suffix)
                && n.is_char_boundary(STAMP_LEN)
                && n[..STAMP_LEN].bytes().all(|b| b.is_ascii_alphanumeric())
        })
        .max()?;
    read_json::<Backup>(&dir.join(best), MAX_BACKUP_BYTES)
        .ok()
        .filter(|b| b.block_id == id)
}

fn block_url(base: &str, id: &str) -> String {
    format!("{base}/v1/blocks/{id}")
}

/// Current block: (label, value). The value is not clipped.
async fn get_block(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    id: &str,
) -> Result<(String, String), MemoryError> {
    let resp = client
        .get(block_url(base, id))
        .bearer_auth(key)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|_| MemoryError::Off)?;
    let v = read_json_body(resp).await?;
    let value = v
        .get("value")
        .and_then(Value::as_str)
        .ok_or(MemoryError::BadResponse)?;
    let label = v.get("label").and_then(Value::as_str).unwrap_or("block");
    Ok((label.chars().take(80).collect(), value.to_string()))
}

/// The only write: PATCH `{"value": ...}`. The server must echo the value back. Anything short of
/// a confirmed echo (except a plain 401/403) is `Unconfirmed`: the write may have landed.
async fn patch_value(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    id: &str,
    value: &str,
) -> Result<(), WriteError> {
    let resp = client
        .patch(block_url(base, id))
        .bearer_auth(key)
        .json(&json!({ "value": value }))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|_| WriteError::Unconfirmed)?;
    let v = match read_json_body(resp).await {
        Ok(v) => v,
        Err(MemoryError::Unauthorized) => return Err(MemoryError::Unauthorized.into()),
        Err(_) => return Err(WriteError::Unconfirmed),
    };
    if v.get("value").and_then(Value::as_str) == Some(value) {
        Ok(())
    } else {
        Err(WriteError::Unconfirmed)
    }
}

/// Best-effort agent name for the backup record (extra GETs, 2 s in total).
async fn agent_name(base: &str, key: &str, id: &str) -> String {
    tokio::time::timeout(AGENT_LOOKUP_TIMEOUT, list_from(base, key))
        .await
        .ok()
        .and_then(Result::ok)
        .and_then(|items| items.into_iter().find(|m| m.id == id))
        .map(|m| m.agent)
        .unwrap_or_else(|| "unknown".to_string())
}

/// Where and with what to talk to Letta.
pub(crate) struct Ctx<'a> {
    pub base: &'a str,
    pub key: &'a str,
    pub dir: &'a Path,
}

/// One block change: what the backup records and what the PATCH sets.
struct Change<'a> {
    label: String,
    old_value: String,
    action: &'a str,
    new_value: &'a str,
}

async fn backup_then_patch(
    client: &reqwest::Client,
    c: &Ctx<'_>,
    id: &str,
    ch: Change<'_>,
) -> Result<(), WriteError> {
    let agent = agent_name(c.base, c.key, id).await;
    write_backup(
        c.dir,
        &Backup {
            block_id: id.to_string(),
            label: ch.label,
            agent,
            old_value: ch.old_value,
            action: ch.action.to_string(),
            at: local_store::iso(&now_jst()),
        },
    )?;
    patch_value(client, c.base, c.key, id, ch.new_value).await
}

/// Letta text for a dialog: control characters and newlines become spaces, then cut to `max`
/// characters (with an ellipsis when cut).
/// Invisible format characters that could reorder or hide dialog text: zero-width, bidi marks,
/// embeddings, overrides and isolates, word joiners, BOM, soft hyphen, Arabic letter mark, Mongolian
/// vowel separator, and tag characters.
fn is_hidden_format(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x061C | 0x180E | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064
        | 0x2066..=0x2069 | 0xFEFF | 0xE0000..=0xE007F)
}

/// One flat line for the approval dialog: hidden format characters removed, control characters
/// flattened, cut to `max` characters with a count of what isn't shown.
fn flat(raw: &str, max: usize) -> String {
    // Hidden characters are shown, not dropped, so the dialog never differs from what's stored.
    let visible: String = raw
        .chars()
        .map(|c| {
            if is_hidden_format(c) {
                format!("⟨U+{:04X}⟩", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect();
    let text = local_store::clean_text(&visible, 0, usize::MAX).unwrap_or_default();
    let total = text.chars().count();
    match text.char_indices().nth(max) {
        Some((i, _)) => format!(
            "{}… (+{} more characters not shown)",
            &text[..i],
            total - max
        ),
        None => text,
    }
}

/// Dialog body: always six lines, whatever the memory text holds.
fn dialog_detail(label: &str, old: &str, new: &str) -> String {
    format!(
        "Memory text (untrusted):\nBlock: {}\nOld: {}\nNew: {}\n\nThe old text is saved first, so it can be put back.",
        flat(label, 40),
        flat(old, PREVIEW_CHARS),
        flat(new, PREVIEW_CHARS)
    )
}

/// The shared flow. `begin` takes the global lock before the first request; `approve` shows the
/// dialog (operation, detail) and returns a guard held to the end. After approval the block is read
/// again: a different value means `Changed` and no write.
async fn guarded<L, A, Fut, G>(
    c: &Ctx<'_>,
    id: &str,
    begin: impl FnOnce() -> Result<L, WriteError>,
    approve: A,
    plan: impl FnOnce(&str, &str) -> Result<Option<Plan>, WriteError>,
) -> Result<(), WriteError>
where
    A: FnOnce(&'static str, String) -> Fut,
    Fut: Future<Output = Result<G, String>>,
{
    if !safe_id(id) {
        return Err(WriteError::BadInput);
    }
    let _lock = begin()?;
    let client = build_client()?;
    let (label, current) = get_block(&client, c.base, c.key, id).await?;
    let Some(p) = plan(&label, &current)? else {
        return Ok(());
    };
    let _held = approve(p.operation, dialog_detail(&label, &current, &p.new_value))
        .await
        .map_err(|_| WriteError::Cancelled)?;
    let (label2, again) = get_block(&client, c.base, c.key, id).await?;
    if again != current {
        return Err(WriteError::Changed);
    }
    backup_then_patch(
        &client,
        c,
        id,
        Change {
            label: label2,
            old_value: again,
            action: p.action,
            new_value: &p.new_value,
        },
    )
    .await
}

struct Plan {
    operation: &'static str,
    action: &'static str,
    new_value: String,
}

pub(crate) async fn edit_from<L, A, Fut, G>(
    c: &Ctx<'_>,
    id: &str,
    value: &str,
    begin: impl FnOnce() -> Result<L, WriteError>,
    approve: A,
) -> Result<(), WriteError>
where
    A: FnOnce(&'static str, String) -> Fut,
    Fut: Future<Output = Result<G, String>>,
{
    // New text may not carry hidden format characters: what's approved is exactly what's written.
    if !safe_id(id) || value.trim().is_empty() || value.chars().any(is_hidden_format) {
        return Err(WriteError::BadInput);
    }
    if value.chars().count() > MAX_VALUE_CHARS {
        return Err(WriteError::TooLong);
    }
    guarded(c, id, begin, approve, |_, old| {
        // The list view clips long blocks; saving over one would silently lose the tail.
        if old.chars().count() > MAX_VALUE_CHARS {
            return Err(WriteError::BlockTooLong);
        }
        Ok(Some(Plan {
            operation: "Change one of Amara's memories",
            action: "edit",
            new_value: value.to_string(),
        }))
    })
    .await
}

/// "Delete" = approve, back up, then clear the value. The block itself stays.
pub(crate) async fn clear_from<L, A, Fut, G>(
    c: &Ctx<'_>,
    id: &str,
    begin: impl FnOnce() -> Result<L, WriteError>,
    approve: A,
) -> Result<(), WriteError>
where
    A: FnOnce(&'static str, String) -> Fut,
    Fut: Future<Output = Result<G, String>>,
{
    guarded(c, id, begin, approve, |_, old| {
        Ok((!old.is_empty()).then(|| Plan {
            operation: "Clear one of Amara's memories",
            action: "delete",
            new_value: String::new(),
        }))
    })
    .await
}

pub(crate) async fn restore_from<L, A, Fut, G>(
    c: &Ctx<'_>,
    id: &str,
    begin: impl FnOnce() -> Result<L, WriteError>,
    approve: A,
) -> Result<(), WriteError>
where
    A: FnOnce(&'static str, String) -> Fut,
    Fut: Future<Output = Result<G, String>>,
{
    if !safe_id(id) {
        return Err(WriteError::BadInput);
    }
    let b = latest_backup(c.dir, id).ok_or(WriteError::NoBackup)?;
    if b.old_value.chars().count() > RESTORE_MAX_CHARS {
        return Err(WriteError::BackupFailed);
    }
    guarded(c, id, begin, approve, |_, _| {
        Ok(Some(Plan {
            operation: "Put back an earlier version of one of Amara's memories",
            action: "restore",
            new_value: b.old_value.clone(),
        }))
    })
    .await
}

async fn key() -> Result<zeroize::Zeroizing<String>, WriteError> {
    tokio::task::spawn_blocking(|| read_key(&MacKeychain))
        .await
        .map_err(|_| WriteError::Mem(MemoryError::KeyError))?
        .map_err(WriteError::from)
}

/// One lock for every memory write: only one memory dialog or write at a time.
const GLOBAL_LOCK: &str = "letta-memory-write";

fn global_lock() -> Result<approval_gate::InFlightGuard, WriteError> {
    approval_gate::InFlightGuard::acquire_all(&[approval_gate::service_key(GLOBAL_LOCK)])
        .map_err(|_| WriteError::Busy)
}

/// The native dialog. It uses its own key; the global lock already guarantees it is free.
async fn show(
    app: tauri::AppHandle,
    operation: &'static str,
    detail: String,
) -> Result<approval_gate::InFlightGuard, String> {
    let text = approval_gate::approval_text(operation, &detail)?;
    approval_gate::require_approval(
        &app,
        &[approval_gate::service_key("letta-memory-dialog")],
        text,
    )
    .await
}

fn check_id(id: &str) -> Result<(), String> {
    if safe_id(id) {
        Ok(())
    } else {
        Err(WriteError::BadInput.code().to_string())
    }
}

/// Replace a block's value (max 2000 characters) after a native approval. The old value is saved first.
#[tauri::command]
#[specta::specta]
pub async fn memory_edit(
    app: tauri::AppHandle,
    block_id: String,
    value: String,
) -> Result<(), String> {
    check_id(&block_id)?;
    let k = key().await.map_err(|e| e.code().to_string())?;
    let c = Ctx {
        base: LETTA_BASE,
        key: &k,
        dir: &backup_dir(),
    };
    edit_from(&c, &block_id, &value, global_lock, |o, d| show(app, o, d))
        .await
        .map_err(|e| e.code().to_string())
}

/// Clears a block's value after a native approval dialog. The block itself is never deleted.
#[tauri::command]
#[specta::specta]
pub async fn memory_delete(app: tauri::AppHandle, block_id: String) -> Result<(), String> {
    check_id(&block_id)?;
    let k = key().await.map_err(|e| e.code().to_string())?;
    let c = Ctx {
        base: LETTA_BASE,
        key: &k,
        dir: &backup_dir(),
    };
    clear_from(&c, &block_id, global_lock, |o, d| show(app, o, d))
        .await
        .map_err(|e| e.code().to_string())
}

/// Undo: puts back the old value from the newest backup of this block, after a native approval.
#[tauri::command]
#[specta::specta]
pub async fn memory_restore_last(app: tauri::AppHandle, block_id: String) -> Result<(), String> {
    check_id(&block_id)?;
    let k = key().await.map_err(|e| e.code().to_string())?;
    let c = Ctx {
        base: LETTA_BASE,
        key: &k,
        dir: &backup_dir(),
    };
    restore_from(&c, &block_id, global_lock, |o, d| show(app, o, d))
        .await
        .map_err(|e| e.code().to_string())
}

#[cfg(test)]
mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::*;
    use crate::commands::approval_gate;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const SENTINEL: &str = "SENTINEL-letta-pw-do-not-leak";

    struct Fake {
        base: String,
        lines: Arc<Mutex<Vec<String>>>,
        blocks: Arc<Mutex<HashMap<String, String>>>,
        /// For each PATCH: did the backup dir already hold a file at that moment?
        backup_at_patch: Arc<Mutex<Vec<bool>>>,
    }

    impl Fake {
        fn value(&self, id: &str) -> String {
            self.blocks.lock().unwrap().get(id).cloned().unwrap()
        }
        fn patches(&self) -> usize {
            self.lines
                .lock()
                .unwrap()
                .iter()
                .filter(|l| l.starts_with("PATCH "))
                .count()
        }
        fn assert_only_get_and_patch(&self) {
            for l in self.lines.lock().unwrap().iter() {
                assert!(
                    l.starts_with("GET ") || l.starts_with("PATCH "),
                    "unexpected method: {l}"
                );
            }
        }
    }

    fn count_files(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .map(|d| d.filter_map(|e| e.ok()).count())
            .unwrap_or(0)
    }

    /// Fake Letta: `b1` (value given), `bad-long` (over the cap), one agent `agent-1` owning `b1`.
    async fn fake(dir: PathBuf, initial: &str) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let lines = Arc::new(Mutex::new(Vec::new()));
        let blocks = Arc::new(Mutex::new(HashMap::from([
            ("b1".to_string(), initial.to_string()),
            ("bad-long".to_string(), "x".repeat(MAX_VALUE_CHARS + 10)),
            ("slow".to_string(), "v".to_string()),
            (
                "evil".to_string(),
                format!("line1\nline2\r\u{1b}[31m\u{7}red {}", "x".repeat(500)),
            ),
        ])));
        let backup_at_patch = Arc::new(Mutex::new(Vec::new()));
        let (l2, b2, p2) = (lines.clone(), blocks.clone(), backup_at_patch.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let (lines, blocks, probe, dir) = (l2.clone(), b2.clone(), p2.clone(), dir.clone());
                tokio::spawn(async move {
                    let mut buf: Vec<u8> = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let head_end = loop {
                        let Ok(r) = sock.read(&mut tmp).await else {
                            return;
                        };
                        if r == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..r]);
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break p + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let want: usize = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse().ok())
                        })
                        .unwrap_or(0);
                    while buf.len() < head_end + want {
                        let Ok(r) = sock.read(&mut tmp).await else {
                            return;
                        };
                        if r == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..r]);
                    }
                    let body = String::from_utf8_lossy(&buf[head_end..]).to_string();
                    let line = head.lines().next().unwrap_or("").to_string();
                    let auth_ok = head.lines().any(|l| {
                        l.to_ascii_lowercase().starts_with("authorization:")
                            && l.ends_with(&format!("Bearer {SENTINEL}"))
                    });
                    lines.lock().unwrap().push(line.clone());
                    let (status, out) = if !auth_ok {
                        (401, "{}".to_string())
                    } else if line.starts_with("GET /v1/agents/?") {
                        (200, json!([{"id":"agent-1","name":"Amara"}]).to_string())
                    } else if line.starts_with("GET /v1/agents/agent-1/core-memory/blocks") {
                        (
                            200,
                            json!([{"id":"b1","label":"human","value":"v"}]).to_string(),
                        )
                    } else if let Some(id) = line
                        .strip_prefix("GET /v1/blocks/")
                        .and_then(|r| r.split(' ').next())
                    {
                        match blocks.lock().unwrap().get(id) {
                            Some(v) => {
                                (200, json!({"id":id,"label":"human","value":v}).to_string())
                            }
                            None => (404, "{}".to_string()),
                        }
                    } else if let Some(id) = line
                        .strip_prefix("PATCH /v1/blocks/")
                        .and_then(|r| r.split(' ').next())
                    {
                        probe.lock().unwrap().push(count_files(&dir) > 0);
                        if id == "slow" {
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                        let v: Value = serde_json::from_str(&body).unwrap_or_default();
                        let new = v
                            .get("value")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        blocks.lock().unwrap().insert(id.to_string(), new.clone());
                        // "echo" can be sabotaged by a block named like this
                        let echoed = if id != "other-block" {
                            new
                        } else {
                            "other".to_string()
                        };
                        (
                            200,
                            json!({"id":id,"label":"human","value":echoed}).to_string(),
                        )
                    } else {
                        (404, "{}".to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                        out.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Fake {
            base,
            lines,
            blocks,
            backup_at_patch,
        }
    }

    fn read_backups(dir: &Path) -> Vec<Backup> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
            .iter()
            .map(|n| read_json::<Backup>(&dir.join(n), MAX_BACKUP_BYTES).unwrap())
            .collect()
    }

    type Seen = Arc<Mutex<Vec<String>>>;

    async fn approve_ok(_o: &'static str, _t: String) -> Result<(), String> {
        Ok(())
    }
    async fn approve_no(_o: &'static str, _t: String) -> Result<(), String> {
        Err("declined: nope".to_string())
    }
    fn no_lock() -> Result<(), WriteError> {
        Ok(())
    }
    fn ctx<'a>(f: &'a Fake, key: &'a str, dir: &'a Path) -> Ctx<'a> {
        Ctx {
            base: &f.base,
            key,
            dir,
        }
    }
    /// Approver that records the dialog text.
    fn recorder(
        seen: Arc<Mutex<Vec<String>>>,
    ) -> impl FnOnce(&'static str, String) -> std::future::Ready<Result<(), String>> {
        move |o, d| {
            seen.lock().unwrap().push(format!("{o}\n{d}"));
            std::future::ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn edit_writes_the_backup_before_the_patch_then_patches() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "Lives in Tokyo").await;
        let seen: Seen = Arc::default();
        edit_from(
            &ctx(&f, SENTINEL, &dir),
            "b1",
            "Lives in Kyoto",
            no_lock,
            recorder(seen.clone()),
        )
        .await
        .unwrap();
        assert_eq!(f.value("b1"), "Lives in Kyoto");
        assert_eq!(*f.backup_at_patch.lock().unwrap(), vec![true]);
        let b = read_backups(&dir);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].block_id, "b1");
        assert_eq!(b[0].label, "human");
        assert_eq!(b[0].agent, "Amara");
        assert_eq!(b[0].old_value, "Lives in Tokyo");
        assert_eq!(b[0].action, "edit");
        assert!(b[0].at.starts_with("20"));
        // the dialog showed both texts, labelled untrusted
        let d = seen.lock().unwrap()[0].clone();
        assert!(
            d.contains("Memory text (untrusted):")
                && d.contains("Old: Lives in Tokyo")
                && d.contains("New: Lives in Kyoto"),
            "{d}"
        );
        let name = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap();
        let text = std::fs::read_to_string(name.path()).unwrap();
        assert!(!text.contains(SENTINEL) && !text.to_lowercase().contains("bearer"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(name.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
        f.assert_only_get_and_patch();
    }

    #[tokio::test]
    async fn the_lock_is_taken_before_the_first_request() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "v").await;
        let lines = f.lines.clone();
        let begin = move || {
            assert!(
                lines.lock().unwrap().is_empty(),
                "a request went out before the lock"
            );
            Ok(())
        };
        edit_from(&ctx(&f, SENTINEL, &dir), "b1", "w", begin, approve_ok)
            .await
            .unwrap();
        let lines = f.lines.clone();
        let begin = move || {
            assert_eq!(lines.lock().unwrap().len(), 0);
            Err::<(), _>(WriteError::Busy)
        };
        let n = f.lines.lock().unwrap().len();
        let e = clear_from(
            &ctx(&f, SENTINEL, &dir),
            "b1",
            || -> Result<(), WriteError> { Err(WriteError::Busy) },
            approve_ok,
        )
        .await
        .unwrap_err();
        assert_eq!(e, WriteError::Busy);
        assert_eq!(
            f.lines.lock().unwrap().len(),
            n,
            "a busy lock must stop before any request"
        );
        let _ = begin;
    }

    #[tokio::test]
    async fn only_one_memory_dialog_at_a_time() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "v").await;
        let key = format!("test-lock-{}", uuid::Uuid::new_v4());
        let mk = {
            let key = key.clone();
            move || {
                approval_gate::InFlightGuard::acquire_all(std::slice::from_ref(&key))
                    .map_err(|_| WriteError::Busy)
            }
        };
        let (base, dir2) = (f.base.clone(), dir.clone());
        let mk2 = mk.clone();
        let inner = std::sync::Arc::new(Mutex::new(None));
        let inner2 = inner.clone();
        // while the first dialog is "open", a second memory write must be refused
        let approve = move |_o: &'static str, _t: String| async move {
            let c = Ctx {
                base: &base,
                key: SENTINEL,
                dir: &dir2,
            };
            let r = clear_from(&c, "b1", mk2, approve_ok).await;
            *inner2.lock().unwrap() = Some(r);
            Ok::<(), String>(())
        };
        edit_from(&ctx(&f, SENTINEL, &dir), "b1", "w", mk, approve)
            .await
            .unwrap();
        assert_eq!(inner.lock().unwrap().unwrap(), Err(WriteError::Busy));
        // and the lock is free again afterwards
        assert!(approval_gate::InFlightGuard::acquire_all(std::slice::from_ref(&key)).is_ok());
        // the production lock is one global key
        let g = global_lock().unwrap();
        assert_eq!(global_lock().err(), Some(WriteError::Busy));
        drop(g);
        assert!(global_lock().is_ok());
    }

    #[tokio::test]
    async fn a_failed_backup_aborts_the_write() {
        let root = temp_dir("memw");
        let blocker = root.join("blocker");
        std::fs::write(&blocker, "a file, not a folder").unwrap();
        let dir = blocker.join("memory-backups");
        let f = fake(dir.clone(), "old").await;
        let c = ctx(&f, SENTINEL, &dir);
        let e = edit_from(&c, "b1", "new", no_lock, approve_ok)
            .await
            .unwrap_err();
        assert_eq!(e, WriteError::BackupFailed);
        assert_eq!(f.patches(), 0);
        assert_eq!(f.value("b1"), "old");
        let e = clear_from(&c, "b1", no_lock, approve_ok).await.unwrap_err();
        assert_eq!(e, WriteError::BackupFailed);
        assert_eq!(f.patches(), 0);
        assert_eq!(f.value("b1"), "old");
    }

    #[tokio::test]
    async fn delete_is_a_clear_never_a_delete_and_is_backed_up_first() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "Allergic to pollen").await;
        let seen: Seen = Arc::default();
        clear_from(
            &ctx(&f, SENTINEL, &dir),
            "b1",
            no_lock,
            recorder(seen.clone()),
        )
        .await
        .unwrap();
        assert_eq!(f.value("b1"), "");
        assert_eq!(*f.backup_at_patch.lock().unwrap(), vec![true]);
        let b = read_backups(&dir);
        assert_eq!(
            (b[0].action.as_str(), b[0].old_value.as_str()),
            ("delete", "Allergic to pollen")
        );
        assert!(seen.lock().unwrap()[0].contains("Old: Allergic to pollen"));
        f.assert_only_get_and_patch();
    }

    #[tokio::test]
    async fn a_denied_approval_is_cancelled_for_edit_delete_and_restore() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "keep me").await;
        let c = ctx(&f, SENTINEL, &dir);
        let e = edit_from(&c, "b1", "x", no_lock, approve_no)
            .await
            .unwrap_err();
        assert_eq!(e, WriteError::Cancelled);
        assert_eq!(e.code(), "cancelled");
        let e = clear_from(&c, "b1", no_lock, approve_no).await.unwrap_err();
        assert_eq!(e, WriteError::Cancelled);
        assert_eq!(f.patches(), 0);
        assert_eq!(f.value("b1"), "keep me");
        assert_eq!(count_files(&dir), 0);
        // restore: needs a backup first
        edit_from(&c, "b1", "second", no_lock, approve_ok)
            .await
            .unwrap();
        let before = count_files(&dir);
        let patches = f.patches();
        let e = restore_from(&c, "b1", no_lock, approve_no)
            .await
            .unwrap_err();
        assert_eq!(e, WriteError::Cancelled);
        assert_eq!((f.patches(), count_files(&dir)), (patches, before));
        assert_eq!(f.value("b1"), "second");
    }

    #[tokio::test]
    async fn a_value_that_changed_while_the_dialog_was_open_is_changed_and_not_written() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "original").await;
        let c = ctx(&f, SENTINEL, &dir);
        let bump = |f: &Fake| {
            let blocks = f.blocks.clone();
            move |_o: &'static str, _t: String| {
                blocks.lock().unwrap().insert(
                    "b1".into(),
                    format!("someone else wrote this {}", uuid::Uuid::new_v4()),
                );
                std::future::ready(Ok::<(), String>(()))
            }
        };
        let e = edit_from(&c, "b1", "mine", no_lock, bump(&f))
            .await
            .unwrap_err();
        assert_eq!((e, e.code()), (WriteError::Changed, "changed"));
        assert_eq!((f.patches(), count_files(&dir)), (0, 0));
        assert!(f.value("b1").starts_with("someone else wrote this"));
        let e = clear_from(&c, "b1", no_lock, bump(&f)).await.unwrap_err();
        assert_eq!(e, WriteError::Changed);
        assert_eq!(f.patches(), 0);
        // restore
        f.blocks.lock().unwrap().insert("b1".into(), "x1".into());
        edit_from(&c, "b1", "x2", no_lock, approve_ok)
            .await
            .unwrap();
        let (p, n) = (f.patches(), count_files(&dir));
        let e = restore_from(&c, "b1", no_lock, bump(&f)).await.unwrap_err();
        assert_eq!(e, WriteError::Changed);
        assert_eq!((f.patches(), count_files(&dir)), (p, n));
    }

    #[tokio::test]
    async fn restore_puts_the_last_old_value_back_with_its_own_backup() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "first").await;
        let c = ctx(&f, SENTINEL, &dir);
        assert_eq!(
            restore_from(&c, "b1", no_lock, approve_ok)
                .await
                .unwrap_err(),
            WriteError::NoBackup
        );
        assert_eq!(f.patches(), 0);
        edit_from(&c, "b1", "second", no_lock, approve_ok)
            .await
            .unwrap();
        let seen: Seen = Arc::default();
        restore_from(&c, "b1", no_lock, recorder(seen.clone()))
            .await
            .unwrap();
        assert_eq!(f.value("b1"), "first");
        let d = seen.lock().unwrap()[0].clone();
        assert!(d.contains("Old: second") && d.contains("New: first"), "{d}");
        let b = read_backups(&dir);
        assert_eq!(b.len(), 2);
        assert_eq!(
            (b[1].action.as_str(), b[1].old_value.as_str()),
            ("restore", "second")
        );
        clear_from(&c, "b1", no_lock, approve_ok).await.unwrap();
        assert_eq!(f.value("b1"), "");
        restore_from(&c, "b1", no_lock, approve_ok).await.unwrap();
        assert_eq!(f.value("b1"), "first");
        f.assert_only_get_and_patch();
    }

    #[tokio::test]
    async fn dialog_text_is_flattened_and_cut() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "v").await;
        let seen: Seen = Arc::default();
        edit_from(
            &ctx(&f, SENTINEL, &dir),
            "evil",
            &format!("n\new\u{1b}[0m{}", "y".repeat(400)),
            no_lock,
            recorder(seen.clone()),
        )
        .await
        .unwrap();
        let d = seen.lock().unwrap()[0].clone();
        let (op, detail) = d.split_once('\n').unwrap();
        assert!(op.starts_with("Change"));
        assert_eq!(
            detail.lines().count(),
            6,
            "memory text added lines: {detail:?}"
        );
        assert!(
            !detail.chars().any(|c| c.is_control() && c != '\n'),
            "{detail:?}"
        );
        for l in detail
            .lines()
            .filter(|l| l.starts_with("Old: ") || l.starts_with("New: "))
        {
            // "Old: " + the preview + "… (+N more characters not shown)"
            assert!(
                l.chars().count() <= 5 + PREVIEW_CHARS + 40,
                "{}",
                l.chars().count()
            );
            assert!(l.ends_with("more characters not shown)"), "{l}");
        }
        assert!(detail.contains("Old: line1 line2"), "{detail:?}");
        assert_eq!(flat("a\r\nb\tc", 50), "a b c");
        let cut = flat(&"é".repeat(300), 200);
        assert!(
            cut.starts_with(&"é".repeat(200))
                && cut.ends_with("… (+100 more characters not shown)"),
            "{cut}"
        );
    }

    #[tokio::test]
    async fn ids_and_values_are_validated_before_any_request() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "v").await;
        let c = ctx(&f, SENTINEL, &dir);
        for bad in ["", "../x", "a/b", "a b", "b1?x=1", &"a".repeat(81)] {
            assert_eq!(
                edit_from(&c, bad, "x", no_lock, approve_ok)
                    .await
                    .unwrap_err(),
                WriteError::BadInput,
                "{bad}"
            );
            assert_eq!(
                clear_from(&c, bad, no_lock, approve_ok).await.unwrap_err(),
                WriteError::BadInput
            );
            assert_eq!(
                restore_from(&c, bad, no_lock, approve_ok)
                    .await
                    .unwrap_err(),
                WriteError::BadInput
            );
        }
        // A new value with a hidden format character is refused before any request.
        for sneaky in ["safe\u{202E}txt", "a\u{200B}b", "x\u{E0041}"] {
            assert_eq!(
                edit_from(&c, "block-1", sneaky, no_lock, approve_ok)
                    .await
                    .unwrap_err(),
                WriteError::BadInput,
                "{sneaky:?}"
            );
        }
        assert_eq!(
            edit_from(&c, "b1", "  ", no_lock, approve_ok)
                .await
                .unwrap_err(),
            WriteError::BadInput
        );
        assert!(f.lines.lock().unwrap().is_empty());
        assert_eq!(count_files(&dir), 0);
    }

    #[tokio::test]
    async fn the_value_is_capped_at_2000_characters() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "v").await;
        let c = ctx(&f, SENTINEL, &dir);
        let e = edit_from(
            &c,
            "b1",
            &"é".repeat(MAX_VALUE_CHARS + 1),
            no_lock,
            approve_ok,
        )
        .await
        .unwrap_err();
        assert_eq!(e, WriteError::TooLong);
        assert!(f.lines.lock().unwrap().is_empty());
        edit_from(&c, "b1", &"é".repeat(MAX_VALUE_CHARS), no_lock, approve_ok)
            .await
            .unwrap();
        assert_eq!(f.value("b1").chars().count(), MAX_VALUE_CHARS);
    }

    #[tokio::test]
    async fn a_block_over_the_cap_is_not_edited_so_nothing_is_lost() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "v").await;
        let e = edit_from(
            &ctx(&f, SENTINEL, &dir),
            "bad-long",
            "short",
            no_lock,
            approve_ok,
        )
        .await
        .unwrap_err();
        assert_eq!(e, WriteError::BlockTooLong);
        assert_eq!(f.patches(), 0);
        assert_eq!(count_files(&dir), 0);
    }

    #[tokio::test]
    async fn an_unechoed_patch_reply_is_unconfirmed() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "v").await;
        f.blocks
            .lock()
            .unwrap()
            .insert("other-block".into(), "v".into());
        let e = edit_from(
            &ctx(&f, SENTINEL, &dir),
            "other-block",
            "new",
            no_lock,
            approve_ok,
        )
        .await
        .unwrap_err();
        assert_eq!(e, WriteError::Unconfirmed);
        assert_eq!(e.code(), "letta_unconfirmed");
    }

    #[tokio::test]
    async fn a_patch_that_times_out_is_unconfirmed() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "v").await;
        let e = edit_from(&ctx(&f, SENTINEL, &dir), "slow", "new", no_lock, approve_ok)
            .await
            .unwrap_err();
        assert_eq!(e, WriteError::Unconfirmed);
        assert_eq!(f.patches(), 1);
        // a closed port on the PATCH leg (reads succeed first) is unconfirmed too; a closed port on the first read is plain off
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = format!("http://{}", l.local_addr().unwrap());
        drop(l);
        let c = Ctx {
            base: &closed,
            key: "k",
            dir: &dir,
        };
        assert_eq!(
            edit_from(&c, "b1", "x", no_lock, approve_ok)
                .await
                .unwrap_err(),
            WriteError::Mem(MemoryError::Off)
        );
    }

    #[tokio::test]
    async fn errors_are_fixed_codes_and_never_carry_the_password() {
        let dir = temp_dir("memw").join("memory-backups");
        let f = fake(dir.clone(), "v").await;
        let e = edit_from(
            &ctx(&f, "wrong-pw-xyz", &dir),
            "b1",
            "x",
            no_lock,
            approve_ok,
        )
        .await
        .unwrap_err();
        assert_eq!(e, WriteError::Mem(MemoryError::Unauthorized));
        let shown = format!("{e:?}{}", e.code());
        assert!(!shown.contains("wrong-pw-xyz") && !shown.contains(SENTINEL));
        for (e, c) in [
            (WriteError::BadInput, "letta_bad_input"),
            (WriteError::TooLong, "letta_value_too_long"),
            (WriteError::BlockTooLong, "letta_block_too_long"),
            (WriteError::BackupFailed, "letta_backup_failed"),
            (WriteError::NoBackup, "letta_no_backup"),
            (WriteError::Cancelled, "cancelled"),
            (WriteError::Changed, "changed"),
            (WriteError::Busy, "letta_busy"),
            (WriteError::Unconfirmed, "letta_unconfirmed"),
            (WriteError::Mem(MemoryError::NoKey), "letta_no_key"),
        ] {
            assert_eq!(e.code(), c);
        }
        assert_eq!(f.patches(), 0);
    }

    #[test]
    fn this_module_never_builds_delete_post_or_put() {
        let src = include_str!("memory_write.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        for banned in [
            ".delete(",
            ".post(",
            ".put(",
            "Method::DELETE",
            "Method::POST",
            "Method::PUT",
        ] {
            assert!(!prod.contains(banned), "found {banned}");
        }
        assert!(prod.contains(".patch(") && prod.contains(".get("));
    }
}

#[cfg(test)]
mod dialog_text_tests {
    use super::*;

    #[test]
    fn dialog_text_drops_hidden_format_characters_and_counts_what_is_cut() {
        let sneaky = "ok\u{202E}evil\u{200B}\u{2066}x\u{E0041}\u{00AD}y";
        let f = flat(sneaky, 200);
        assert_eq!(f, "ok⟨U+202E⟩evil⟨U+200B⟩⟨U+2066⟩x⟨U+E0041⟩⟨U+00AD⟩y");
        assert!(f.chars().all(|c| !is_hidden_format(c)));
        let long = "a".repeat(250);
        let cut = flat(&long, 200);
        assert!(cut.ends_with("… (+50 more characters not shown)"), "{cut}");
        let d = dialog_detail("hu\u{202E}man", "old", &long);
        assert!(d.contains("Block: hu⟨U+202E⟩man"));
        assert!(d.contains("(+50 more characters not shown)"));
        assert_eq!(d.lines().count(), 6);
    }
}
