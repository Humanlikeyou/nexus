//! Business ledger (decision 0005): `<data_dir>/business/ledger.json`.
//!
//! Amounts are stored in yen (integers). USD is never invented: it exists only when the file holds a
//! manually entered rate with an as-of date. A missing file returns `None` (the UI stays "Needs
//! wiring"); a malformed one is an error, never a partial ledger.
//!
//! Writes (decision 0045 item 8): add, edit and archive a venture, and set the hand-entered FX rate.
//! Each write edits the file as raw JSON so fields this code does not know survive, re-validates the whole
//! ledger before touching disk, copies the old file to `ledger.json.bak`, writes atomically, and appends a
//! change record (with the old value) to `business/ledger-history.json`. Nothing is ever deleted: archiving
//! sets `archived`. Undo is allowed for 8 s and only for the newest change; it restores the old value
//! (an undone add is archived, not removed). No money moves and nothing leaves this machine.

use super::local_store::{clean_text, new_id, read_json, write_json_atomic, write_text_atomic};
use super::shared::task::{blocking, JoinErr};
use super::shared::time::now_jst;
use chrono::{DateTime, FixedOffset, NaiveDate, SecondsFormat};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_LEDGER_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FxRate {
    /// US dollars per 1 yen, entered by hand.
    pub usd_per_jpy: f64,
    /// `YYYY-MM-DD`.
    pub as_of: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LedgerFund {
    pub name: String,
    pub balance_jpy: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: u32,
    pub total: u32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChecklistItem {
    pub label: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub label: String,
    pub due: String,
    pub health: Health,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NextAction {
    pub label: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub action: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    OnTrack,
    AtRisk,
    OffTrack,
    Done,
    Waiting,
    Searching,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VentureDetail {
    #[serde(default)]
    pub headline: String,
    #[serde(default)]
    pub checklist: Vec<ChecklistItem>,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    #[serde(default)]
    pub next_actions: Vec<NextAction>,
    pub budget_used_jpy: Option<i64>,
    pub fund_jpy: Option<i64>,
    #[serde(default)]
    pub open_votes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LedgerVenture {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub stage: String,
    pub health: Health,
    pub progress: Option<Progress>,
    pub next_step: Option<String>,
    #[serde(default)]
    pub note: String,
    pub target_jpy: Option<i64>,
    pub detail: Option<VentureDetail>,
    /// Monthly target when the venture is kept in US dollars (cents). `target_jpy` stays for yen.
    #[serde(default)]
    pub target_usd_cents: Option<i64>,
    /// `JPY` or `USD`; `None` on ledgers written before the write path existed (read as JPY).
    #[serde(default)]
    pub currency: Option<String>,
    /// Archived ventures stay in the file and in History; the UI hides them by default.
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RequestStatus {
    Open,
    Approved,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FundingRequest {
    pub id: String,
    pub requester: String,
    pub purpose: String,
    pub amount_jpy: Option<i64>,
    pub status: RequestStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TaxFlag {
    pub id: String,
    pub label: String,
    pub area: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VoteRules {
    pub needed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BusinessLedger {
    pub version: u32,
    pub fx: Option<FxRate>,
    #[serde(default)]
    pub funds: Vec<LedgerFund>,
    #[serde(default)]
    pub ventures: Vec<LedgerVenture>,
    #[serde(default)]
    pub funding_requests: Vec<FundingRequest>,
    #[serde(default)]
    pub tax_flags: Vec<TaxFlag>,
    pub vote_rules: Option<VoteRules>,
    pub revenue_target_jpy: Option<i64>,
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

impl BusinessLedger {
    /// Rejects values that would make the UI show a made-up or misleading figure.
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!("ledger: unsupported version {}", self.version));
        }
        if let Some(fx) = &self.fx {
            if !(fx.usd_per_jpy.is_finite() && fx.usd_per_jpy > 0.0 && fx.usd_per_jpy < 1.0) {
                return Err("ledger: fx.usdPerJpy must be a positive dollars-per-yen rate".into());
            }
            if !is_date(&fx.as_of) {
                return Err("ledger: fx.asOf must be YYYY-MM-DD".into());
            }
        }
        if let Some(v) = &self.vote_rules {
            if v.needed == 0 {
                return Err("ledger: voteRules.needed must be at least 1".into());
            }
        }
        let amounts = self
            .ventures
            .iter()
            .flat_map(|v| {
                let d = v.detail.as_ref();
                [
                    v.target_jpy,
                    v.target_usd_cents,
                    d.and_then(|d| d.budget_used_jpy),
                    d.and_then(|d| d.fund_jpy),
                ]
            })
            .chain(self.funding_requests.iter().map(|r| r.amount_jpy))
            .chain([self.revenue_target_jpy])
            .flatten();
        if amounts.into_iter().any(|a| a < 0) {
            return Err("ledger: amounts can't be negative (fund balances may be)".into());
        }
        let mut ids: Vec<&str> = self.ventures.iter().map(|v| v.id.as_str()).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|w| w[0] == w[1]) {
            return Err("ledger: duplicate venture id".into());
        }
        Ok(())
    }
}

pub(crate) fn ledger_path_in(data_dir: &Path) -> PathBuf {
    data_dir.join("business").join("ledger.json")
}

/// Reads and validates the ledger under `data_dir`. `Ok(None)` when there is no file.
pub(crate) fn read_ledger_in(data_dir: &Path) -> Result<Option<BusinessLedger>, String> {
    let path = ledger_path_in(data_dir);
    match std::fs::symlink_metadata(data_dir.join("business")) {
        Ok(m) if !m.file_type().is_dir() => {
            return Err("ledger: business/ must be a real folder (no symlink)".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        _ => {}
    }
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("ledger: can't read business/ledger.json".into()),
    };
    if !meta.file_type().is_file() {
        return Err("ledger: business/ledger.json must be a regular file (no symlink)".into());
    }
    if meta.len() > MAX_LEDGER_BYTES {
        return Err("ledger: business/ledger.json is over 1 MiB".into());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|_| "ledger: can't read business/ledger.json".to_string())?;
    let ledger: BusinessLedger = serde_json::from_str(&text)
        // Line/column only: the serde message can quote ledger text.
        .map_err(|e| {
            format!(
                "ledger: business/ledger.json is not valid (line {}, column {})",
                e.line(),
                e.column()
            )
        })?;
    ledger.validate()?;
    Ok(Some(ledger))
}

#[tauri::command]
#[specta::specta]
pub async fn read_business_ledger() -> Result<Option<BusinessLedger>, String> {
    let dir = crate::app_paths::data_dir();
    tokio::task::spawn_blocking(move || read_ledger_in(&dir))
        .await
        .map_err(|_| "ledger: read task failed".to_string())?
}

// ───────────────────────────── writes ─────────────────────────────

const UNDO_MS: i64 = 8_000;
const MAX_HISTORY: usize = 1000;
const MAX_HISTORY_BYTES: u64 = 2 * 1024 * 1024;
/// One monthly target may not exceed this many minor units (same cap Treasury uses per event).
const MAX_TARGET_MINOR: i64 = 1_000_000_000_000;

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// One line of ledger History. `subject` is a venture id, or `fx` for the rate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LedgerChange {
    pub id: String,
    /// RFC 3339, JST.
    pub at: String,
    /// `venture_add`, `venture_edit`, `venture_archive`, `venture_restore` or `fx_set`.
    pub action: String,
    pub subject: String,
    /// Plain words for the History row, e.g. the venture name.
    pub label: String,
    #[serde(default)]
    pub undone: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredChange {
    change: Option<LedgerChange>,
    /// The venture object (or fx object) as it was before; `None` when it did not exist.
    before: Option<Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct History {
    #[serde(default)]
    records: Vec<StoredChange>,
}

/// What the form sends. Money is integer minor units (yen, or US cents), never a float.
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VentureDraft {
    /// `None` adds a venture; `Some` edits that one.
    pub id: Option<String>,
    pub name: String,
    pub stage: String,
    /// One line: what it is.
    pub one_liner: String,
    pub next_step: Option<String>,
    pub monthly_target_minor: Option<i64>,
    /// `JPY` or `USD`.
    pub currency: String,
}

fn history_path(data_dir: &Path) -> PathBuf {
    data_dir.join("business").join("ledger-history.json")
}

pub(crate) fn history_list_in(data_dir: &Path) -> Result<Vec<LedgerChange>, String> {
    let h: History = read_json(&history_path(data_dir), MAX_HISTORY_BYTES)?;
    let mut out: Vec<LedgerChange> = h.records.into_iter().filter_map(|r| r.change).collect();
    out.reverse();
    Ok(out)
}

fn skeleton() -> Value {
    json!({ "version": 1, "funds": [], "ventures": [], "fundingRequests": [], "taxFlags": [] })
}

/// The ledger as raw JSON (so unknown fields survive an edit) after the normal read checks passed.
fn load_raw(data_dir: &Path) -> Result<Value, String> {
    if read_ledger_in(data_dir)?.is_none() {
        return Ok(skeleton());
    }
    let text = std::fs::read_to_string(ledger_path_in(data_dir))
        .map_err(|_| "ledger: can't read business/ledger.json".to_string())?;
    serde_json::from_str(&text).map_err(|_| "ledger: business/ledger.json is not valid".to_string())
}

/// Validates the whole edited ledger, keeps a copy of the old file, then writes atomically.
fn commit_raw(data_dir: &Path, raw: &Value) -> Result<(), String> {
    let ledger: BusinessLedger = serde_json::from_value(raw.clone())
        .map_err(|_| "ledger: that change isn't valid".to_string())?;
    ledger.validate()?;
    let path = ledger_path_in(data_dir);
    if let Ok(old) = std::fs::read_to_string(&path) {
        write_text_atomic(
            &data_dir.join("business").join("ledger.json.bak"),
            &old,
            MAX_LEDGER_BYTES,
        )?;
    }
    let text = serde_json::to_string_pretty(raw).map_err(|_| "ledger: can't encode".to_string())?;
    write_text_atomic(&path, &text, MAX_LEDGER_BYTES)
}

fn push_history(
    data_dir: &Path,
    action: &str,
    subject: &str,
    label: &str,
    before: Option<Value>,
    at: DateTime<FixedOffset>,
) -> Result<LedgerChange, String> {
    let path = history_path(data_dir);
    let mut h: History = read_json(&path, MAX_HISTORY_BYTES)?;
    let change = LedgerChange {
        id: new_id("lch"),
        at: at.to_rfc3339_opts(SecondsFormat::Millis, false),
        action: action.into(),
        subject: subject.into(),
        label: label.into(),
        undone: false,
    };
    h.records.push(StoredChange {
        change: Some(change.clone()),
        before,
    });
    // Oldest records age out of History only; the ledger itself never loses a venture.
    if h.records.len() > MAX_HISTORY {
        let extra = h.records.len() - MAX_HISTORY;
        h.records.drain(..extra);
    }
    write_json_atomic(&path, &h, MAX_HISTORY_BYTES)?;
    Ok(change)
}

fn slug(name: &str, taken: &[String]) -> String {
    let mut base = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            base.push(c.to_ascii_lowercase());
        } else if !base.ends_with('-') && !base.is_empty() {
            base.push('-');
        }
    }
    let mut base: String = base.trim_end_matches('-').chars().take(40).collect();
    if base.is_empty() {
        base = "venture".into();
    }
    let mut id = base.clone();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn venture_index(raw: &Value, id: &str) -> Option<usize> {
    raw.get("ventures")?
        .as_array()?
        .iter()
        .position(|v| v.get("id").and_then(Value::as_str) == Some(id))
}

pub(crate) fn venture_save_in(
    data_dir: &Path,
    d: &VentureDraft,
    at: DateTime<FixedOffset>,
) -> Result<LedgerChange, String> {
    let name = clean_text(&d.name, 1, 60)
        .map_err(|_| "ledger: a venture needs a name (up to 60 characters)".to_string())?;
    let stage = clean_text(&d.stage, 1, 30).map_err(|_| "ledger: choose a stage".to_string())?;
    let one_liner = clean_text(&d.one_liner, 0, 140)
        .map_err(|_| "ledger: keep the description to 140 characters".to_string())?;
    let next_step = match d.next_step.as_deref() {
        Some(n) => Some(
            clean_text(n, 0, 140)
                .map_err(|_| "ledger: keep the next step to 140 characters".to_string())?,
        )
        .filter(|n| !n.is_empty()),
        None => None,
    };
    if d.currency != "JPY" && d.currency != "USD" {
        return Err("ledger: currency must be JPY or USD".into());
    }
    if let Some(t) = d.monthly_target_minor {
        if !(0..=MAX_TARGET_MINOR).contains(&t) {
            return Err("ledger: the monthly target can't be negative".into());
        }
    }
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut raw = load_raw(data_dir)?;
    let ids: Vec<String> = raw["ventures"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let (idx, before, id, action) = match d.id.as_deref() {
        Some(id) => {
            let i = venture_index(&raw, id).ok_or("ledger: that venture doesn't exist")?;
            (
                i,
                Some(raw["ventures"][i].clone()),
                id.to_string(),
                "venture_edit",
            )
        }
        None => {
            let id = slug(&name, &ids);
            let health = if stage.eq_ignore_ascii_case("idea") {
                "searching"
            } else {
                "on_track"
            };
            let fresh = json!({ "id": id, "name": name, "stage": stage, "health": health,
                "progress": null, "nextStep": null, "note": "", "targetJpy": null, "detail": null, "archived": false });
            let arr = raw["ventures"]
                .as_array_mut()
                .ok_or("ledger: ventures must be a list")?;
            arr.push(fresh);
            (arr.len() - 1, None, id, "venture_add")
        }
    };
    let (jpy, usd) = match (d.currency.as_str(), d.monthly_target_minor) {
        ("JPY", t) => (t, None),
        (_, t) => (None, t),
    };
    let v = raw["ventures"][idx]
        .as_object_mut()
        .ok_or("ledger: bad venture")?;
    v.insert("name".into(), json!(name));
    v.insert("stage".into(), json!(stage));
    v.insert("note".into(), json!(one_liner));
    v.insert("nextStep".into(), json!(next_step));
    v.insert("currency".into(), json!(d.currency));
    v.insert("targetJpy".into(), json!(jpy));
    v.insert("targetUsdCents".into(), json!(usd));
    commit_raw(data_dir, &raw)?;
    push_history(data_dir, action, &id, &name, before, at)
}

pub(crate) fn venture_archive_in(
    data_dir: &Path,
    id: &str,
    archived: bool,
    at: DateTime<FixedOffset>,
) -> Result<LedgerChange, String> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut raw = load_raw(data_dir)?;
    let i = venture_index(&raw, id).ok_or("ledger: that venture doesn't exist")?;
    let before = raw["ventures"][i].clone();
    let name = before
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .to_string();
    raw["ventures"][i]
        .as_object_mut()
        .ok_or("ledger: bad venture")?
        .insert("archived".into(), json!(archived));
    commit_raw(data_dir, &raw)?;
    push_history(
        data_dir,
        if archived {
            "venture_archive"
        } else {
            "venture_restore"
        },
        id,
        &name,
        Some(before),
        at,
    )
}

pub(crate) fn fx_set_in(
    data_dir: &Path,
    jpy_per_usd: f64,
    as_of: &str,
    at: DateTime<FixedOffset>,
) -> Result<LedgerChange, String> {
    if !(jpy_per_usd.is_finite() && (20.0..=1000.0).contains(&jpy_per_usd)) {
        return Err("ledger: enter yen per one US dollar (for example 150.25)".into());
    }
    if !is_date(as_of) || NaiveDate::parse_from_str(as_of, "%Y-%m-%d").is_err() {
        return Err("ledger: the date must be a real day, YYYY-MM-DD".into());
    }
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut raw = load_raw(data_dir)?;
    let before = raw.get("fx").filter(|v| !v.is_null()).cloned();
    raw["fx"] = json!({ "usdPerJpy": 1.0 / jpy_per_usd, "asOf": as_of });
    commit_raw(data_dir, &raw)?;
    push_history(data_dir, "fx_set", "fx", "Exchange rate", before, at)
}

/// Restores the old value. Only the newest change, only inside 8 s, only once.
pub(crate) fn undo_in(
    data_dir: &Path,
    id: &str,
    now: DateTime<FixedOffset>,
) -> Result<bool, String> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = history_path(data_dir);
    let mut h: History = read_json(&path, MAX_HISTORY_BYTES)?;
    let Some(last) = h.records.last_mut() else {
        return Ok(false);
    };
    let Some(change) = last.change.as_mut() else {
        return Ok(false);
    };
    if change.id != id || change.undone {
        return Ok(false);
    }
    let at =
        DateTime::parse_from_rfc3339(&change.at).map_err(|_| "ledger: bad history".to_string())?;
    if (now - at).num_milliseconds() > UNDO_MS {
        return Ok(false);
    }
    let mut raw = load_raw(data_dir)?;
    if change.subject == "fx" {
        raw["fx"] = last.before.clone().unwrap_or(Value::Null);
    } else {
        let i = venture_index(&raw, &change.subject).ok_or("ledger: that venture doesn't exist")?;
        match &last.before {
            Some(b) => raw["ventures"][i] = b.clone(),
            // An undone add is archived, never removed.
            None => {
                raw["ventures"][i]
                    .as_object_mut()
                    .ok_or("ledger: bad venture")?
                    .insert("archived".into(), json!(true));
            }
        }
    }
    commit_raw(data_dir, &raw)?;
    change.undone = true;
    write_json_atomic(&path, &h, MAX_HISTORY_BYTES)?;
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub async fn ledger_venture_save(draft: VentureDraft) -> Result<LedgerChange, String> {
    blocking(JoinErr::Code("ledger: save task failed"), move || {
        venture_save_in(&crate::app_paths::data_dir(), &draft, now_jst())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn ledger_venture_archive(id: String, archived: bool) -> Result<LedgerChange, String> {
    blocking(JoinErr::Code("ledger: save task failed"), move || {
        venture_archive_in(&crate::app_paths::data_dir(), &id, archived, now_jst())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn ledger_fx_set(jpy_per_usd: f64, as_of: String) -> Result<LedgerChange, String> {
    blocking(JoinErr::Code("ledger: save task failed"), move || {
        fx_set_in(
            &crate::app_paths::data_dir(),
            jpy_per_usd,
            &as_of,
            now_jst(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn ledger_history_list() -> Result<Vec<LedgerChange>, String> {
    blocking(JoinErr::Code("ledger: read task failed"), move || {
        history_list_in(&crate::app_paths::data_dir())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn ledger_undo(id: String) -> Result<bool, String> {
    blocking(JoinErr::Code("ledger: save task failed"), move || {
        undo_in(&crate::app_paths::data_dir(), &id, now_jst())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh temp dir, removed on drop (never the real app data).
    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn dir() -> TempDir {
        let p = std::env::temp_dir().join(format!("nexus-ledger-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    fn write(d: &Path, body: &str) {
        std::fs::create_dir_all(d.join("business")).unwrap();
        std::fs::write(ledger_path_in(d), body).unwrap();
    }

    const OK: &str = r#"{
      "version": 1,
      "fx": { "usdPerJpy": 0.0066, "asOf": "2026-11-11" },
      "funds": [{ "name": "Unallocated", "balanceJpy": 150000 }],
      "ventures": [{ "id": "taiyo-skin", "name": "Taiyo Skin", "stage": "Pre-launch", "health": "at_risk",
                     "progress": { "done": 5, "total": 9, "label": "launch checklist" }, "nextStep": "Label approval",
                     "targetJpy": 3000000, "detail": { "headline": "h", "budgetUsedJpy": 420000, "fundJpy": 600000, "openVotes": 1 } }],
      "fundingRequests": [{ "id": "f1", "requester": "Taiyo Skin", "purpose": "First batch", "amountJpy": 250000, "status": "open" }],
      "taxFlags": [], "voteRules": { "needed": 1 }, "revenueTargetJpy": null
    }"#;

    #[test]
    fn no_file_is_none_not_an_empty_ledger() {
        assert_eq!(read_ledger_in(dir().path()).unwrap(), None);
    }

    #[test]
    fn reads_a_valid_ledger_with_yen_and_a_dated_rate() {
        let d = dir();
        write(d.path(), OK);
        let l = read_ledger_in(d.path()).unwrap().unwrap();
        assert_eq!(l.funds[0].balance_jpy, 150000);
        assert_eq!(l.fx.unwrap().as_of, "2026-11-11");
        assert_eq!(l.ventures[0].health, Health::AtRisk);
        assert_eq!(l.vote_rules, Some(VoteRules { needed: 1 }));
    }

    #[test]
    fn rejects_bad_rates_votes_negatives_duplicates_and_versions() {
        for (from, to) in [
            (r#""usdPerJpy": 0.0066"#, r#""usdPerJpy": 150"#),
            (r#""asOf": "2026-11-11""#, r#""asOf": "yesterday""#),
            (r#""needed": 1"#, r#""needed": 0"#),
            (r#""amountJpy": 250000"#, r#""amountJpy": -1"#),
            (r#""version": 1"#, r#""version": 2"#),
            (r#""status": "open""#, r#""status": "maybe""#),
        ] {
            let d = dir();
            write(d.path(), &OK.replace(from, to));
            assert!(read_ledger_in(d.path()).is_err(), "{to} should be rejected");
        }
        let d = dir();
        let dup = OK.replace(
            r#""ventures": [{"#,
            r#""ventures": [{ "id": "taiyo-skin", "name": "x", "health": "done", "progress": null, "nextStep": null, "targetJpy": null, "detail": null }, {"#,
        );
        write(d.path(), &dup);
        assert!(read_ledger_in(d.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_business_folder_and_never_quotes_the_file() {
        let d = dir();
        let elsewhere = d.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("ledger.json"), OK).unwrap();
        std::os::unix::fs::symlink(&elsewhere, d.path().join("business")).unwrap();
        assert!(read_ledger_in(d.path()).is_err());
        let d2 = dir();
        write(
            d2.path(),
            r#"{"version": 1, "funds": [{"name": "SECRET-NOTE", "balanceJpy": "x"}]}"#,
        );
        let err = read_ledger_in(d2.path()).unwrap_err();
        assert!(
            !err.contains("SECRET-NOTE") && err.contains("line"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlinked_ledger() {
        let d = dir();
        let real = d.path().join("elsewhere.json");
        std::fs::write(&real, OK).unwrap();
        std::fs::create_dir_all(d.path().join("business")).unwrap();
        std::os::unix::fs::symlink(&real, ledger_path_in(d.path())).unwrap();
        assert!(read_ledger_in(d.path()).is_err());
    }

    // ── writes ──

    fn t(sec: i64) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-19T10:00:00+09:00").unwrap()
            + chrono::Duration::seconds(sec)
    }

    fn draft(id: Option<&str>, name: &str) -> VentureDraft {
        VentureDraft {
            id: id.map(str::to_string),
            name: name.into(),
            stage: "Building".into(),
            one_liner: "Skin care".into(),
            next_step: Some("Pick a supplier".into()),
            monthly_target_minor: Some(300_000),
            currency: "JPY".into(),
        }
    }

    fn raw(d: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(ledger_path_in(d)).unwrap()).unwrap()
    }

    #[test]
    fn first_add_creates_a_valid_ledger_and_logs_history() {
        let d = dir();
        let c = venture_save_in(d.path(), &draft(None, "Taiyo Skin"), t(0)).unwrap();
        assert_eq!(
            (c.action.as_str(), c.subject.as_str()),
            ("venture_add", "taiyo-skin")
        );
        let l = read_ledger_in(d.path()).unwrap().unwrap();
        assert_eq!(l.version, 1);
        assert_eq!(l.ventures[0].note, "Skin care");
        assert_eq!(l.ventures[0].target_jpy, Some(300_000));
        assert_eq!(l.ventures[0].currency.as_deref(), Some("JPY"));
        assert!(!l.ventures[0].archived);
        assert_eq!(history_list_in(d.path()).unwrap().len(), 1);
    }

    #[test]
    fn edit_keeps_unknown_fields_and_every_other_record_and_backs_up() {
        let d = dir();
        let with_extra = OK
            .replace(
                r#""version": 1,"#,
                r#""version": 1, "futureTop": {"a": [1, 2]},"#,
            )
            .replace(
                r#""stage": "Pre-launch","#,
                r#""stage": "Pre-launch", "futureField": "keep","#,
            );
        write(d.path(), &with_extra);
        let c =
            venture_save_in(d.path(), &draft(Some("taiyo-skin"), "Taiyo Skin 2"), t(0)).unwrap();
        assert_eq!(c.action, "venture_edit");
        let r = raw(d.path());
        assert_eq!(r["futureTop"], json!({"a": [1, 2]}));
        assert_eq!(r["ventures"][0]["futureField"], "keep");
        assert_eq!(r["ventures"][0]["name"], "Taiyo Skin 2");
        // Fields the form doesn't own are untouched.
        assert_eq!(r["ventures"][0]["detail"]["budgetUsedJpy"], 420000);
        assert_eq!(r["fundingRequests"][0]["id"], "f1");
        assert_eq!(r["funds"][0]["balanceJpy"], 150000);
        let bak: Value = serde_json::from_str(
            &std::fs::read_to_string(d.path().join("business").join("ledger.json.bak")).unwrap(),
        )
        .unwrap();
        assert_eq!(bak["ventures"][0]["name"], "Taiyo Skin");
    }

    #[test]
    fn duplicate_names_get_distinct_ids_and_bad_input_writes_nothing() {
        let d = dir();
        venture_save_in(d.path(), &draft(None, "Shop"), t(0)).unwrap();
        let c = venture_save_in(d.path(), &draft(None, "Shop"), t(1)).unwrap();
        assert_eq!(c.subject, "shop-2");
        let before = std::fs::read_to_string(ledger_path_in(d.path())).unwrap();
        for bad in [
            VentureDraft {
                name: "  ".into(),
                ..draft(None, "x")
            },
            VentureDraft {
                currency: "EUR".into(),
                ..draft(None, "x")
            },
            VentureDraft {
                monthly_target_minor: Some(-1),
                ..draft(None, "x")
            },
            VentureDraft {
                stage: "".into(),
                ..draft(None, "x")
            },
            draft(Some("nope"), "x"),
        ] {
            assert!(venture_save_in(d.path(), &bad, t(2)).is_err());
        }
        assert_eq!(
            std::fs::read_to_string(ledger_path_in(d.path())).unwrap(),
            before
        );
        assert_eq!(history_list_in(d.path()).unwrap().len(), 2);
    }

    #[test]
    fn usd_target_is_kept_in_cents_and_yen_is_cleared() {
        let d = dir();
        let u = VentureDraft {
            currency: "USD".into(),
            monthly_target_minor: Some(50_000),
            ..draft(None, "Store")
        };
        venture_save_in(d.path(), &u, t(0)).unwrap();
        let l = read_ledger_in(d.path()).unwrap().unwrap();
        assert_eq!(
            (l.ventures[0].target_jpy, l.ventures[0].target_usd_cents),
            (None, Some(50_000))
        );
    }

    #[test]
    fn archive_flags_and_never_removes_then_restores() {
        let d = dir();
        write(d.path(), OK);
        venture_archive_in(d.path(), "taiyo-skin", true, t(0)).unwrap();
        assert!(read_ledger_in(d.path()).unwrap().unwrap().ventures[0].archived);
        venture_archive_in(d.path(), "taiyo-skin", false, t(1)).unwrap();
        let l = read_ledger_in(d.path()).unwrap().unwrap();
        assert_eq!(l.ventures.len(), 1);
        assert!(!l.ventures[0].archived);
        assert!(venture_archive_in(d.path(), "ghost", true, t(2)).is_err());
    }

    #[test]
    fn fx_set_stores_a_dated_inverse_rate_and_rejects_nonsense() {
        let d = dir();
        write(d.path(), OK);
        fx_set_in(d.path(), 150.0, "2026-11-19", t(0)).unwrap();
        let fx = read_ledger_in(d.path()).unwrap().unwrap().fx.unwrap();
        assert_eq!(fx.as_of, "2026-11-19");
        assert!((fx.usd_per_jpy - 1.0 / 150.0).abs() < 1e-12);
        for (r, day) in [
            (0.0, "2026-11-19"),
            (f64::NAN, "2026-11-19"),
            (5000.0, "2026-11-19"),
            (150.0, "2026-13-40"),
            (150.0, "soon"),
        ] {
            assert!(fx_set_in(d.path(), r, day, t(1)).is_err(), "{r} {day}");
        }
        assert!(
            (read_ledger_in(d.path())
                .unwrap()
                .unwrap()
                .fx
                .unwrap()
                .usd_per_jpy
                - 1.0 / 150.0)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn undo_restores_inside_eight_seconds_for_the_newest_change_only() {
        let d = dir();
        write(d.path(), OK);
        let c = venture_save_in(d.path(), &draft(Some("taiyo-skin"), "Renamed"), t(0)).unwrap();
        assert!(!undo_in(d.path(), &c.id, t(9)).unwrap(), "too late");
        assert!(undo_in(d.path(), &c.id, t(3)).unwrap());
        assert_eq!(
            read_ledger_in(d.path()).unwrap().unwrap().ventures[0].name,
            "Taiyo Skin"
        );
        assert!(!undo_in(d.path(), &c.id, t(4)).unwrap(), "only once");
        assert!(history_list_in(d.path()).unwrap()[0].undone);
        // An older change can't be undone once a newer one exists.
        let a = fx_set_in(d.path(), 150.0, "2026-11-19", t(10)).unwrap();
        let b = venture_archive_in(d.path(), "taiyo-skin", true, t(11)).unwrap();
        assert!(!undo_in(d.path(), &a.id, t(12)).unwrap());
        assert!(undo_in(d.path(), &b.id, t(12)).unwrap());
        assert!(!read_ledger_in(d.path()).unwrap().unwrap().ventures[0].archived);
    }

    #[test]
    fn undone_add_is_archived_not_deleted() {
        let d = dir();
        let c = venture_save_in(d.path(), &draft(None, "Oops"), t(0)).unwrap();
        assert!(undo_in(d.path(), &c.id, t(2)).unwrap());
        let l = read_ledger_in(d.path()).unwrap().unwrap();
        assert_eq!(l.ventures.len(), 1);
        assert!(l.ventures[0].archived);
    }

    #[test]
    fn undo_of_the_first_rate_removes_the_rate() {
        let d = dir();
        venture_save_in(d.path(), &draft(None, "A"), t(0)).unwrap();
        let a = fx_set_in(d.path(), 150.0, "2026-11-19", t(10)).unwrap();
        assert!(undo_in(d.path(), &a.id, t(11)).unwrap());
        assert!(read_ledger_in(d.path()).unwrap().unwrap().fx.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn writes_refuse_a_symlinked_ledger_and_leave_its_target_alone() {
        let d = dir();
        let real = d.path().join("elsewhere.json");
        std::fs::write(&real, OK).unwrap();
        std::fs::create_dir_all(d.path().join("business")).unwrap();
        std::os::unix::fs::symlink(&real, ledger_path_in(d.path())).unwrap();
        assert!(venture_save_in(d.path(), &draft(None, "X"), t(0)).is_err());
        assert_eq!(std::fs::read_to_string(&real).unwrap(), OK);
    }

    #[test]
    fn the_write_path_has_no_network_or_money_code() {
        let src = include_str!("business_ledger.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        for banned in ["reqwest", "TcpStream", "treasury_", "personal_money"] {
            assert!(!prod.contains(banned), "{banned}");
        }
    }
}
