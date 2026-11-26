//! Personal money entries (decision 0045 item 4): what Taisun types in by hand, so the dashboard works with no
//! bank connection at all. Nothing here moves, pays, trades or converts money; it only records and adds up.
//!
//! Store: `<data>/money/entries.jsonl`, append-only (0600, one JSON object per line, 8 MB cap). A line is either an
//! entry or a void marker `{"void_of": "<entry id>", ...}`. An edit is a void plus a new row; an Undo is a void.
//! History is never rewritten. `balances.json` keeps its shape and is still written for manual accounts, so old
//! readers keep working. Unreadable lines are skipped and counted, never guessed.
//!
//! Personal only: an entry may name a personal account and nothing else. Venture and Enterprise stores are not
//! read or written here.
//!
//! Roll-up rules (`month_view_in`), per currency (JPY and USD are never added together without a stored rate):
//! - income / expense / savings come from entries dated in that month, plus imported statement rows (JPY CSV
//!   accounts: credits are income, debits are expense, uncategorised).
//! - No double count: a manual income/expense entry that names a CSV account is ignored for dates the statement
//!   already covers (date <= that account's last imported row). Entries with no account (cash) always count.
//!   Manual and statement amounts are reported separately as well as summed.
//! - Available now = the latest balance entry (else the stored personal account balances) + income - expense -
//!   savings dated AFTER that balance's date. No balance on record = no figure ("–"), never 0.
//! - Bills are obligations, not cash movements: they appear under "upcoming" and "commitments" and reach the
//!   totals only when an expense is recorded.

use super::local_store::{clean_text, iso, new_id, now_jst};
use super::money::{
    accounts_list_in, balance_set_manual_in, balances_list_scoped_in, fx_get_in,
    personal_statements_in, FxRate,
};
use super::shared::task::{blocking, JoinErr};
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 4096;
const MAX_AMOUNT: i64 = 10_000_000_000_000;
const MAX_MONTHS: usize = 24;
const KINDS: [&str; 6] = [
    "income",
    "expense",
    "savings",
    "bill",
    "commitment",
    "balance",
];
const CURRENCIES: [&str; 2] = ["JPY", "USD"];
const RECURRENCES: [&str; 4] = ["none", "monthly", "weekly", "yearly"];
const SOURCES: [&str; 3] = ["app", "chat", "telegram"];
pub(crate) const CATEGORIES: [&str; 12] = [
    "housing",
    "food",
    "transport",
    "utilities",
    "health",
    "fun",
    "shopping",
    "subscriptions",
    "debt",
    "gifts",
    "travel",
    "other",
];
const UNCATEGORISED: &str = "uncategorised";
const SAVINGS_GOAL_ID: &str = "money-savings";

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn money_dir(data: &Path) -> PathBuf {
    data.join("money")
}

fn entries_path(data: &Path) -> PathBuf {
    money_dir(data).join("entries.jsonl")
}

/// What the UI sends.
#[derive(Debug, Clone, Deserialize, specta::Type)]
pub struct EntryDraft {
    pub kind: String,
    pub account_id: Option<String>,
    pub amount_minor: i64,
    pub currency: String,
    pub date: String,
    pub due: Option<String>,
    pub recurrence: Option<String>,
    pub category: Option<String>,
    pub note: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct MoneyEntry {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    pub amount_minor: i64,
    pub currency: String,
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub source: String,
    pub created_at: String,
    /// Filled when read: the id of the void marker that cancelled this row. Never written to the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voided_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VoidLine {
    void_of: String,
    id: String,
    created_at: String,
}

#[derive(Debug, Clone, Default, Deserialize, specta::Type)]
pub struct EntryRange {
    pub from: Option<String>,
    pub to: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct EntriesList {
    /// Newest first (by when it was recorded). Voided rows are included, marked with `voided_by`.
    pub entries: Vec<MoneyEntry>,
    pub skipped_lines: u32,
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    if s.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn dec(currency: &str) -> u32 {
    if currency == "USD" {
        2
    } else {
        0
    }
}

fn pow10(n: u32) -> i64 {
    10_i64.pow(n)
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

// ───────────────────────────── file ─────────────────────────────

fn read_text(data: &Path) -> Result<String, String> {
    let path = entries_path(data);
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(_) => return Err("store_error".to_string()),
    };
    if !meta.file_type().is_file() || meta.len() > MAX_FILE_BYTES {
        return Err("store_error".to_string());
    }
    // Lossy: a stray byte costs one line, not the whole store.
    fs::read(&path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|_| "store_error".to_string())
}

/// Every readable entry with its void marker applied, in file order, plus the count of lines that were not readable.
pub(crate) fn load(data: &Path) -> Result<(Vec<MoneyEntry>, u32), String> {
    let text = read_text(data)?;
    let mut entries: Vec<MoneyEntry> = Vec::new();
    let mut voids: HashMap<String, String> = HashMap::new();
    let mut skipped = 0u32;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        if line.len() > MAX_LINE_BYTES {
            skipped += 1;
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            skipped += 1;
            continue;
        };
        if v.get("void_of").is_some() {
            match serde_json::from_value::<VoidLine>(v) {
                Ok(m) => {
                    voids.entry(m.void_of).or_insert(m.id);
                }
                Err(_) => skipped += 1,
            }
        } else {
            match serde_json::from_value::<MoneyEntry>(v) {
                Ok(e) if valid_stored(&e) => entries.push(e),
                _ => skipped += 1,
            }
        }
    }
    for e in &mut entries {
        e.voided_by = voids.get(&e.id).cloned();
    }
    Ok((entries, skipped))
}

/// A stored row is checked again on read: a hand-edited or damaged line is skipped, not summed.
fn valid_stored(e: &MoneyEntry) -> bool {
    KINDS.contains(&e.kind.as_str())
        && CURRENCIES.contains(&e.currency.as_str())
        && parse_day(&e.date).is_some()
        && e.amount_minor.abs() < MAX_AMOUNT
        && (e.kind == "balance" || e.amount_minor > 0)
        && e.due.as_deref().is_none_or(|d| parse_day(d).is_some())
        && e.recurrence
            .as_deref()
            .is_none_or(|r| RECURRENCES.contains(&r))
}

fn append_line(data: &Path, line: &str) -> Result<(), String> {
    crate::app_paths::create_private_dir_all(&money_dir(data))
        .map_err(|_| "store_error".to_string())?;
    let path = entries_path(data);
    let len = match fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_file() => m.len(),
        Ok(_) => return Err("store_error".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => return Err("store_error".to_string()),
    };
    if len + line.len() as u64 + 1 > MAX_FILE_BYTES {
        return Err("store_full".to_string());
    }
    // A file that did not end in a newline (a torn write) would glue two rows together.
    let mut buf = String::with_capacity(line.len() + 2);
    if len > 0 && !read_text(data)?.ends_with('\n') {
        buf.push('\n');
    }
    buf.push_str(line);
    buf.push('\n');
    let mut opts = fs::OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path).map_err(|_| "store_error".to_string())?;
    f.write_all(buf.as_bytes())
        .map_err(|_| "store_error".to_string())
}

// ───────────────────────────── add / void / list ─────────────────────────────

fn opt_trim(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn validate(data: &Path, d: &EntryDraft, today: NaiveDate) -> Result<MoneyEntry, String> {
    let bad = || "invalid_input".to_string();
    if !KINDS.contains(&d.kind.as_str()) || !CURRENCIES.contains(&d.currency.as_str()) {
        return Err(bad());
    }
    let is_balance = d.kind == "balance";
    if d.amount_minor.abs() >= MAX_AMOUNT || (d.amount_minor <= 0 && !is_balance) {
        return Err(bad());
    }
    let date = parse_day(d.date.trim()).ok_or_else(bad)?;
    // What has happened can't be dated in the future; a bill or commitment is about the future by nature.
    if matches!(
        d.kind.as_str(),
        "income" | "expense" | "savings" | "balance"
    ) && date > today
    {
        return Err(bad());
    }
    let due = match opt_trim(&d.due) {
        Some(s) => Some(parse_day(s).ok_or_else(bad)?.format("%Y-%m-%d").to_string()),
        None => None,
    };
    let recurrence = match opt_trim(&d.recurrence) {
        Some(r) if RECURRENCES.contains(&r) => Some(r.to_string()),
        Some(_) => return Err(bad()),
        None => None,
    };
    let recurs = recurrence.as_deref().is_some_and(|r| r != "none");
    if d.kind == "bill" && due.is_none() {
        return Err(bad());
    }
    if d.kind == "commitment" && !recurs && due.is_none() {
        return Err(bad());
    }
    if recurs && !matches!(d.kind.as_str(), "bill" | "commitment") {
        return Err(bad());
    }
    let category = match opt_trim(&d.category) {
        Some(c) if CATEGORIES.contains(&c) => Some(c.to_string()),
        Some(_) => return Err(bad()),
        None => None,
    };
    let note = match d.note.as_deref() {
        Some(n) => Some(clean_text(n, 0, 200)?).filter(|n| !n.is_empty()),
        None => None,
    };
    let source = match d.source.as_deref() {
        Some(s) if SOURCES.contains(&s) => s.to_string(),
        Some(_) => return Err(bad()),
        None => "app".to_string(),
    };
    let account_id = match opt_trim(&d.account_id) {
        Some(id) => {
            if !safe_id(id) {
                return Err(bad());
            }
            let a = accounts_list_in(data)?
                .into_iter()
                .find(|a| a.id == id)
                .ok_or_else(|| "not_found".to_string())?;
            // Personal accounts only, in the entry's own currency.
            if a.scope != "personal" || a.currency != d.currency {
                return Err(bad());
            }
            Some(a.id)
        }
        None => None,
    };
    Ok(MoneyEntry {
        id: new_id("me"),
        kind: d.kind.clone(),
        account_id,
        amount_minor: d.amount_minor,
        currency: d.currency.clone(),
        date: date.format("%Y-%m-%d").to_string(),
        due,
        recurrence,
        category,
        note,
        source,
        created_at: iso(&now_jst()),
        voided_by: None,
    })
}

fn write_entry(data: &Path, e: &MoneyEntry) -> Result<(), String> {
    let line = serde_json::to_string(e).map_err(|_| "store_error".to_string())?;
    append_line(data, &line)?;
    // Back-compat: a balance typed for a manual (or watch-only) account also updates balances.json.
    // Best effort: the entry above is the record, so a failure here changes nothing the user sees.
    if e.kind == "balance" && e.amount_minor >= 0 {
        if let Some(id) = &e.account_id {
            let native = e.amount_minor as f64 / pow10(dec(&e.currency)) as f64;
            let _ = balance_set_manual_in(data, id, native, &e.date);
        }
    }
    Ok(())
}

pub(crate) fn entry_add_in(
    data: &Path,
    d: &EntryDraft,
    today: NaiveDate,
) -> Result<MoneyEntry, String> {
    let e = validate(data, d, today)?;
    let _g = lock();
    write_entry(data, &e)?;
    // Amounts stay in the money store; the activity row only says an entry was logged.
    super::amara_store::note(
        data,
        super::amara_store::NewActivity {
            certainty: "confirmed",
            scope: super::amara_store::Scope::Personal,
            kind: "money.logged",
            subject_ref: &format!("money:{}", e.id),
            occurred_at: &super::amara_store::day_start(&e.date),
            source: "user",
            source_ref: None,
            actor: "taisun",
            summary: None,
            data_json: Some(r#"{"precision":"day"}"#),
        },
    );
    Ok(e)
}

/// Voids one entry and, for an edit, appends the replacement in the same locked step. Returns the new row.
pub(crate) fn entry_void_in(
    data: &Path,
    id: &str,
    replacement: Option<&EntryDraft>,
    today: NaiveDate,
) -> Result<Option<MoneyEntry>, String> {
    if !safe_id(id) {
        return Err("invalid_input".to_string());
    }
    let new_row = match replacement {
        Some(d) => Some(validate(data, d, today)?),
        None => None,
    };
    let _g = lock();
    let (entries, _) = load(data)?;
    let target = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| "not_found".to_string())?;
    if target.voided_by.is_some() {
        return Err("already_voided".to_string());
    }
    let marker = VoidLine {
        void_of: id.to_string(),
        id: new_id("mv"),
        created_at: iso(&now_jst()),
    };
    let line = serde_json::to_string(&marker).map_err(|_| "store_error".to_string())?;
    append_line(data, &line)?;
    if let Some(e) = &new_row {
        write_entry(data, e)?;
    }
    Ok(new_row)
}

pub(crate) fn entries_list_in(data: &Path, range: &EntryRange) -> Result<EntriesList, String> {
    let from = match range.from.as_deref() {
        Some(s) => Some(parse_day(s).ok_or_else(|| "invalid_input".to_string())?),
        None => None,
    };
    let to = match range.to.as_deref() {
        Some(s) => Some(parse_day(s).ok_or_else(|| "invalid_input".to_string())?),
        None => None,
    };
    let _g = lock();
    let (mut entries, skipped_lines) = load(data)?;
    entries.retain(|e| {
        let d = parse_day(&e.date).unwrap_or(NaiveDate::MIN);
        from.is_none_or(|f| d >= f) && to.is_none_or(|t| d <= t)
    });
    entries.reverse(); // file order is recording order
    entries.truncate(range.limit.unwrap_or(500).min(5000));
    Ok(EntriesList {
        entries,
        skipped_lines,
    })
}

// ───────────────────────────── dates ─────────────────────────────

fn month_key(d: NaiveDate) -> String {
    format!("{:04}-{:02}", d.year(), d.month())
}

fn first_of(y: i32, m: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(NaiveDate::MIN)
}

fn shift_month_first(d: NaiveDate, back: u32) -> NaiveDate {
    let idx = d.year() * 12 + d.month() as i32 - 1 - back as i32;
    first_of(idx.div_euclid(12), idx.rem_euclid(12) as u32 + 1)
}

/// `anchor` plus `n` months, the day clamped to the month's end (the 31st becomes the 30th or 28th).
pub(crate) fn add_months(anchor: NaiveDate, n: u32) -> NaiveDate {
    let idx = anchor.year() * 12 + anchor.month() as i32 - 1 + n as i32;
    let (y, m) = (idx.div_euclid(12), idx.rem_euclid(12) as u32 + 1);
    let mut day = anchor.day();
    loop {
        if let Some(d) = NaiveDate::from_ymd_opt(y, m, day) {
            return d;
        }
        day -= 1;
    }
}

/// Every date of a bill in `[from, to]`: the due date itself, then each repeat of it.
pub(crate) fn occurrences(
    due: NaiveDate,
    recurrence: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    match recurrence {
        "weekly" => {
            let behind = (from - due).num_days();
            let mut k = if behind > 0 { (behind + 6) / 7 } else { 0 };
            while let Some(d) = due.checked_add_signed(chrono::Duration::days(7 * k)) {
                if d > to || out.len() >= 64 {
                    break;
                }
                out.push(d);
                k += 1;
            }
        }
        "monthly" | "yearly" => {
            let step = if recurrence == "yearly" { 12 } else { 1 };
            for k in 0..2400u32 {
                let d = add_months(due, k * step);
                if d > to {
                    break;
                }
                if d >= from {
                    out.push(d);
                }
            }
        }
        _ => {
            if due >= from && due <= to {
                out.push(due);
            }
        }
    }
    out
}

fn monthly_equivalent(amount: i64, recurrence: &str) -> i64 {
    let a = amount as i128;
    let m = match recurrence {
        "monthly" => a,
        "weekly" => (a * 52 + 6) / 12,
        "yearly" => (a + 6) / 12,
        _ => 0,
    };
    m as i64
}

// ───────────────────────────── roll-up ─────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct CategoryTotal {
    pub category: String,
    pub minor: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MonthTotals {
    /// `YYYY-MM`
    pub month: String,
    /// False when nothing at all is recorded for the month: the page shows "–", not 0.
    pub has_data: bool,
    pub income_minor: i64,
    pub expense_minor: i64,
    pub savings_minor: i64,
    /// income - expense (savings contributions are shown separately).
    pub net_minor: i64,
    pub by_category: Vec<CategoryTotal>,
    pub manual_income_minor: i64,
    pub manual_expense_minor: i64,
    pub statement_income_minor: i64,
    pub statement_expense_minor: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct Available {
    pub amount_minor: i64,
    pub base_minor: i64,
    pub base_date: String,
    /// "entry" (a balance you typed) | "accounts" (stored account balances).
    pub base_source: String,
    pub since_net_minor: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct Upcoming {
    pub id: String,
    pub label: String,
    pub amount_minor: i64,
    pub due: String,
    pub days_until: i64,
    pub recurrence: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct Commitment {
    pub id: String,
    pub label: String,
    pub amount_minor: i64,
    pub recurrence: String,
    pub monthly_minor: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MoneySavingsProgress {
    pub title: String,
    pub target_minor: i64,
    pub deadline: Option<String>,
    /// Sum of savings entries (all time) in this currency.
    pub saved_minor: i64,
    pub remaining_minor: i64,
    pub pct: f64,
    pub per_month_needed_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct SeriesPoint {
    pub label: String,
    pub value_minor: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct CurrencyView {
    pub currency: String,
    pub months: Vec<MonthTotals>,
    pub available: Option<Available>,
    pub upcoming: Vec<Upcoming>,
    pub commitments: Vec<Commitment>,
    pub committed_monthly_minor: i64,
    pub goal: Option<MoneySavingsProgress>,
    /// Cumulative savings at the end of each month of the window.
    pub savings_series: Vec<SeriesPoint>,
    /// Balance entries in this currency, oldest first (last 24).
    pub balance_series: Vec<SeriesPoint>,
    pub has_statement: bool,
    pub statement_through: Option<String>,
    /// Manual rows left out of the totals because a statement already covers that date.
    pub excluded_duplicates: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct CombinedUsd {
    pub rate: f64,
    pub rate_date: String,
    pub rate_source: String,
    pub income_cents: i64,
    pub expense_cents: i64,
    pub savings_cents: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MonthView {
    pub today: String,
    /// Oldest first; the last is the current month.
    pub months: Vec<String>,
    pub currencies: Vec<CurrencyView>,
    pub fx: Option<FxRate>,
    /// This month in USD, only when both currencies have data AND a stored rate exists.
    pub combined_usd: Option<CombinedUsd>,
    /// True when both currencies have data and no rate is stored (NEEDS_CONVERSION).
    pub needs_conversion: bool,
    pub skipped_lines: u32,
}

struct Stmt {
    account_id: String,
    through: NaiveDate,
    rows: Vec<(NaiveDate, i64)>,
}

#[derive(Debug, Clone)]
struct SavingsGoal {
    title: String,
    currency: String,
    target_minor: i64,
    deadline: Option<String>,
}

struct AcctBalance {
    currency: String,
    minor: i64,
    date: NaiveDate,
}

fn goals_in(data: &Path) -> Vec<SavingsGoal> {
    let path = data.join("goals.json");
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return Vec::new();
    };
    if !meta.file_type().is_file() || meta.len() > 1024 * 1024 {
        return Vec::new();
    }
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(goals) = serde_json::from_str::<Vec<super::goals::Goal>>(&text) else {
        return Vec::new();
    };
    goals
        .into_iter()
        .filter(|g| g.category.eq_ignore_ascii_case("savings"))
        .filter_map(|g| {
            let currency = g.unit.trim().to_uppercase();
            if !CURRENCIES.contains(&currency.as_str()) || !g.target.is_finite() || g.target <= 0.0
            {
                return None;
            }
            let minor = (g.target * pow10(dec(&currency)) as f64).round();
            if minor >= MAX_AMOUNT as f64 {
                return None;
            }
            Some(SavingsGoal {
                title: g.title,
                currency,
                target_minor: minor as i64,
                deadline: g.deadline.filter(|d| parse_day(d).is_some()),
            })
        })
        .collect()
}

struct Gathered {
    entries: Vec<MoneyEntry>,
    skipped_lines: u32,
    statements: Vec<Stmt>,
    account_balances: Vec<AcctBalance>,
    goals: Vec<SavingsGoal>,
    fx: Option<FxRate>,
}

fn label_of(e: &MoneyEntry) -> String {
    e.note
        .clone()
        .or_else(|| e.category.clone())
        .unwrap_or_else(|| e.kind.clone())
}

fn month_key_of(date: &str) -> String {
    date.get(..7).unwrap_or("").to_string()
}

fn month_view(inp: &Gathered, today: NaiveDate, months: usize) -> MonthView {
    let n = months.clamp(1, MAX_MONTHS);
    let keys: Vec<String> = (0..n as u32)
        .rev()
        .map(|b| month_key(shift_month_first(today, b)))
        .collect();
    let live: Vec<&MoneyEntry> = inp
        .entries
        .iter()
        .filter(|e| e.voided_by.is_none())
        .collect();
    let through: HashMap<&str, NaiveDate> = inp
        .statements
        .iter()
        .map(|s| (s.account_id.as_str(), s.through))
        .collect();
    // A manual income/expense row on a statement account, on a date the statement covers, is a duplicate.
    let is_dup = |e: &MoneyEntry| {
        matches!(e.kind.as_str(), "income" | "expense")
            && e.account_id
                .as_deref()
                .and_then(|a| through.get(a))
                .is_some_and(|t| parse_day(&e.date).is_some_and(|d| d <= *t))
    };

    let mut currencies = Vec::new();
    for cur in CURRENCIES {
        let has_stmt = cur == "JPY" && !inp.statements.is_empty();
        let mine: Vec<&MoneyEntry> = live.iter().copied().filter(|e| e.currency == cur).collect();
        let has_acct_bal = inp.account_balances.iter().any(|b| b.currency == cur);
        if mine.is_empty() && !has_stmt && !has_acct_bal {
            continue;
        }
        let mut totals: Vec<MonthTotals> = keys
            .iter()
            .map(|k| MonthTotals {
                month: k.clone(),
                has_data: false,
                income_minor: 0,
                expense_minor: 0,
                savings_minor: 0,
                net_minor: 0,
                by_category: Vec::new(),
                manual_income_minor: 0,
                manual_expense_minor: 0,
                statement_income_minor: 0,
                statement_expense_minor: 0,
            })
            .collect();
        let mut cats: Vec<BTreeMap<String, i64>> = vec![BTreeMap::new(); keys.len()];
        let mut excluded = 0u32;
        for e in &mine {
            let Some(d) = parse_day(&e.date) else {
                continue;
            };
            let Some(i) = keys.iter().position(|k| *k == month_key(d)) else {
                continue;
            };
            if !matches!(e.kind.as_str(), "income" | "expense" | "savings") {
                continue;
            }
            if is_dup(e) {
                excluded += 1;
                continue;
            }
            let t = &mut totals[i];
            t.has_data = true;
            match e.kind.as_str() {
                "income" => t.manual_income_minor += e.amount_minor,
                "expense" => {
                    t.manual_expense_minor += e.amount_minor;
                    let c = e.category.clone().unwrap_or_else(|| UNCATEGORISED.into());
                    *cats[i].entry(c).or_insert(0) += e.amount_minor;
                }
                _ => t.savings_minor += e.amount_minor,
            }
        }
        if has_stmt {
            for s in &inp.statements {
                for (d, amt) in &s.rows {
                    let Some(i) = keys.iter().position(|k| *k == month_key(*d)) else {
                        continue;
                    };
                    let t = &mut totals[i];
                    t.has_data = true;
                    if *amt >= 0 {
                        t.statement_income_minor += amt;
                    } else {
                        t.statement_expense_minor += -amt;
                        *cats[i].entry(UNCATEGORISED.into()).or_insert(0) += -amt;
                    }
                }
            }
        }
        for (i, t) in totals.iter_mut().enumerate() {
            t.income_minor = t.manual_income_minor + t.statement_income_minor;
            t.expense_minor = t.manual_expense_minor + t.statement_expense_minor;
            t.net_minor = t.income_minor - t.expense_minor;
            let mut v: Vec<CategoryTotal> = std::mem::take(&mut cats[i])
                .into_iter()
                .map(|(category, minor)| CategoryTotal { category, minor })
                .collect();
            v.sort_by(|a, b| b.minor.cmp(&a.minor).then(a.category.cmp(&b.category)));
            t.by_category = v;
        }

        // Available now: the latest balance entry (later file position wins a same-day tie).
        let latest = mine
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == "balance")
            .filter_map(|(i, e)| parse_day(&e.date).map(|d| (d, i, e.amount_minor)))
            .max_by_key(|(d, i, _)| (*d, *i));
        let base = match latest {
            Some((d, _, amount)) => Some((amount, d, "entry")),
            None => {
                let bs: Vec<&AcctBalance> = inp
                    .account_balances
                    .iter()
                    .filter(|b| b.currency == cur)
                    .collect();
                bs.iter()
                    .map(|b| b.date)
                    .min()
                    .map(|d| (bs.iter().map(|b| b.minor).sum::<i64>(), d, "accounts"))
            }
        };
        let available = base.map(|(amount, date, src)| {
            let since: i64 = mine
                .iter()
                .filter(|e| parse_day(&e.date).is_some_and(|d| d > date) && !is_dup(e))
                .map(|e| match e.kind.as_str() {
                    "income" => e.amount_minor,
                    "expense" | "savings" => -e.amount_minor,
                    _ => 0,
                })
                .sum();
            Available {
                amount_minor: amount + since,
                base_minor: amount,
                base_date: date.format("%Y-%m-%d").to_string(),
                base_source: src.to_string(),
                since_net_minor: since,
            }
        });

        // Upcoming bills (next 30 days) and commitments.
        let horizon = today + chrono::Duration::days(30);
        let mut upcoming = Vec::new();
        let mut commitments = Vec::new();
        let mut committed = 0i64;
        for e in &mine {
            let rec = e.recurrence.clone().unwrap_or_else(|| "none".into());
            if e.kind == "bill" {
                if let Some(due) = e.due.as_deref().and_then(parse_day) {
                    for d in occurrences(due, &rec, today, horizon) {
                        upcoming.push(Upcoming {
                            id: e.id.clone(),
                            label: label_of(e),
                            amount_minor: e.amount_minor,
                            due: d.format("%Y-%m-%d").to_string(),
                            days_until: (d - today).num_days(),
                            recurrence: rec.clone(),
                        });
                    }
                }
            }
            if matches!(e.kind.as_str(), "bill" | "commitment") {
                let m = monthly_equivalent(e.amount_minor, &rec);
                if e.kind == "commitment" {
                    commitments.push(Commitment {
                        id: e.id.clone(),
                        label: label_of(e),
                        amount_minor: e.amount_minor,
                        recurrence: rec.clone(),
                        monthly_minor: m,
                    });
                }
                committed += m;
            }
        }
        upcoming.sort_by(|a, b| a.due.cmp(&b.due).then(a.id.cmp(&b.id)));

        // Savings: cumulative series and goal progress.
        let saved_all: i64 = mine
            .iter()
            .filter(|e| e.kind == "savings")
            .map(|e| e.amount_minor)
            .sum();
        let first_key = keys.first().cloned().unwrap_or_default();
        let before: i64 = mine
            .iter()
            .filter(|e| e.kind == "savings" && month_key_of(&e.date) < first_key)
            .map(|e| e.amount_minor)
            .sum();
        let mut run = before;
        let savings_series: Vec<SeriesPoint> = totals
            .iter()
            .map(|t| {
                run += t.savings_minor;
                SeriesPoint {
                    label: t.month.clone(),
                    value_minor: run,
                }
            })
            .collect();
        let mut bal_pts: Vec<(NaiveDate, usize, i64)> = mine
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == "balance")
            .filter_map(|(i, e)| parse_day(&e.date).map(|d| (d, i, e.amount_minor)))
            .collect();
        bal_pts.sort();
        let skip = bal_pts.len().saturating_sub(24);
        let balance_series = bal_pts
            .into_iter()
            .skip(skip)
            .map(|(d, _, v)| SeriesPoint {
                label: d.format("%Y-%m-%d").to_string(),
                value_minor: v,
            })
            .collect();
        let goal = inp.goals.iter().find(|g| g.currency == cur).map(|g| {
            let remaining = (g.target_minor - saved_all).max(0);
            let pct = ((saved_all.max(0) as f64 / g.target_minor as f64) * 1000.0).round() / 10.0;
            let per_month = g
                .deadline
                .as_deref()
                .and_then(parse_day)
                .filter(|d| *d > today && remaining > 0)
                .map(|d| {
                    let m = (((d - today).num_days() as f64) / 30.4375).ceil().max(1.0) as i64;
                    (remaining + m - 1) / m
                });
            MoneySavingsProgress {
                title: g.title.clone(),
                target_minor: g.target_minor,
                deadline: g.deadline.clone(),
                saved_minor: saved_all,
                remaining_minor: remaining,
                pct: pct.min(100.0),
                per_month_needed_minor: per_month,
            }
        });
        let statement_through = if has_stmt {
            inp.statements
                .iter()
                .map(|s| s.through)
                .max()
                .map(|d| d.format("%Y-%m-%d").to_string())
        } else {
            None
        };
        currencies.push(CurrencyView {
            currency: cur.to_string(),
            months: totals,
            available,
            upcoming,
            commitments,
            committed_monthly_minor: committed,
            goal,
            savings_series,
            balance_series,
            has_statement: has_stmt,
            statement_through,
            excluded_duplicates: excluded,
        });
    }

    // USD view of this month, only with a stored rate. Never guessed.
    let both = currencies.len() == 2;
    let combined_usd = match (&inp.fx, both) {
        (Some(fx), true) if fx.rate > 0.0 => {
            let cur = |c: &str| {
                currencies
                    .iter()
                    .find(|v| v.currency == c)
                    .and_then(|v| v.months.last())
            };
            match (cur("JPY"), cur("USD")) {
                (Some(j), Some(u)) => {
                    let conv = |yen: i64| ((yen as f64 / fx.rate) * 100.0).round() as i64;
                    Some(CombinedUsd {
                        rate: fx.rate,
                        rate_date: fx.date.clone(),
                        rate_source: fx.source.clone(),
                        income_cents: u.income_minor + conv(j.income_minor),
                        expense_cents: u.expense_minor + conv(j.expense_minor),
                        savings_cents: u.savings_minor + conv(j.savings_minor),
                    })
                }
                _ => None,
            }
        }
        _ => None,
    };
    MonthView {
        today: today.format("%Y-%m-%d").to_string(),
        months: keys,
        needs_conversion: both && combined_usd.is_none(),
        combined_usd,
        currencies,
        fx: inp.fx.clone(),
        skipped_lines: inp.skipped_lines,
    }
}

fn gather(data: &Path, today: NaiveDate) -> Result<Gathered, String> {
    let (entries, skipped_lines) = {
        let _g = lock();
        load(data)?
    };
    let statements: Vec<Stmt> = personal_statements_in(data)?
        .into_iter()
        .filter(|a| a.currency == "JPY")
        .filter_map(|a| {
            let rows: Vec<(NaiveDate, i64)> = a
                .rows
                .iter()
                .filter_map(|r| parse_day(&r.date).map(|d| (d, r.amount)))
                .filter(|(d, _)| *d <= today)
                .collect();
            let through = rows.iter().map(|r| r.0).max()?;
            Some(Stmt {
                account_id: a.id,
                through,
                rows,
            })
        })
        .collect();
    let account_balances: Vec<AcctBalance> =
        balances_list_scoped_in(data, today, Some("personal"))?
            .balances
            .into_iter()
            .filter(|b| CURRENCIES.contains(&b.currency.as_str()) && b.amount.is_finite())
            .filter_map(|b| {
                let date = parse_day(&b.as_of)?;
                let minor = (b.amount * pow10(dec(&b.currency)) as f64).round();
                (minor.abs() < MAX_AMOUNT as f64).then_some(AcctBalance {
                    currency: b.currency,
                    minor: minor as i64,
                    date,
                })
            })
            .collect();
    Ok(Gathered {
        entries,
        skipped_lines,
        statements,
        account_balances,
        goals: goals_in(data),
        fx: fx_get_in(data).ok().flatten(),
    })
}

pub(crate) fn month_view_in(
    data: &Path,
    today: NaiveDate,
    months: usize,
) -> Result<MonthView, String> {
    let g = gather(data, today)?;
    Ok(month_view(&g, today, months))
}

// ───────────────────────────── savings goal ─────────────────────────────

/// Sets (or replaces) the one savings goal for the Money page, stored as a Goals row with category "savings".
pub(crate) fn savings_goal_set_in(
    data: &Path,
    currency: &str,
    target_minor: i64,
    deadline: Option<&str>,
    today: NaiveDate,
) -> Result<(), String> {
    let bad = || "invalid_input".to_string();
    if !CURRENCIES.contains(&currency) || target_minor <= 0 || target_minor >= MAX_AMOUNT {
        return Err(bad());
    }
    let deadline = match deadline.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => {
            let d = parse_day(s).ok_or_else(bad)?;
            if d <= today {
                return Err(bad());
            }
            Some(d.format("%Y-%m-%d").to_string())
        }
        None => None,
    };
    let path = data.join("goals.json");
    let _g = lock();
    let mut goals: Vec<super::goals::Goal> = match fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).map_err(|_| "store_error".to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(_) => return Err("store_error".to_string()),
    };
    let target = target_minor as f64 / pow10(dec(currency)) as f64;
    match goals.iter_mut().find(|g| g.id == SAVINGS_GOAL_ID) {
        Some(g) => {
            g.target = target;
            g.unit = currency.to_string();
            g.deadline = deadline;
            g.updated_at = String::new();
        }
        None => goals.push(super::goals::Goal {
            id: SAVINGS_GOAL_ID.to_string(),
            title: "Savings goal".to_string(),
            category: "savings".to_string(),
            target,
            current: 0.0,
            unit: currency.to_string(),
            deadline,
            created_at: String::new(),
            updated_at: String::new(),
            venture_id: None,
            why: None,
            remind_at: None,
            paused: false,
            life: Default::default(),
        }),
    }
    super::goals::save_goals_at(&path, goals)
}

// ───────────────────────────── Amara context + check-in ─────────────────────────────

/// "612,000 JPY" / "1,234.50 USD". Never a currency sign.
pub fn fmt_minor(minor: i64, currency: &str) -> String {
    let neg = minor < 0;
    let a = minor.unsigned_abs();
    let d = dec(currency);
    let (whole, frac) = (a / pow10(d) as u64, a % pow10(d) as u64);
    let digits = whole.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if neg { "-" } else { "" };
    if d == 0 {
        format!("{sign}{grouped} {currency}")
    } else {
        format!("{sign}{grouped}.{frac:02} {currency}")
    }
}

/// A short, sourced summary for Amara's context, or `None` when nothing is recorded. Native currency text, the
/// assumptions spelled out, no advice. The orchestrator adds it to the chat sections.
pub fn money_context_section(data: &Path) -> Option<String> {
    money_context_section_on(data, now_jst().date_naive())
}

pub(crate) fn money_context_section_on(data: &Path, today: NaiveDate) -> Option<String> {
    let v = month_view_in(data, today, 3).ok()?;
    if v.currencies.is_empty() {
        return None;
    }
    let mut out = String::from("<nexus_money>\nRecorded personal money (typed by Taisun or imported from his statements). Quote only these numbers and say when something is missing.\n");
    for c in &v.currencies {
        let cur = &c.currency;
        let m = c.months.last()?;
        out.push_str(&format!("[{cur}]\n"));
        match &c.available {
            Some(a) => out.push_str(&format!(
                "- Available now: {} (balance {} on {} plus entries since)\n",
                fmt_minor(a.amount_minor, cur),
                fmt_minor(a.base_minor, cur),
                a.base_date
            )),
            None => out.push_str("- Available now: not recorded (no balance entered)\n"),
        }
        if m.has_data {
            out.push_str(&format!(
                "- {} so far: income {}, spent {}, saved {}\n",
                m.month,
                fmt_minor(m.income_minor, cur),
                fmt_minor(m.expense_minor, cur),
                fmt_minor(m.savings_minor, cur)
            ));
        } else {
            out.push_str(&format!("- {}: nothing recorded yet\n", m.month));
        }
        if let Some(g) = &c.goal {
            out.push_str(&format!(
                "- Savings goal: {} of {} ({}%){}\n",
                fmt_minor(g.saved_minor, cur),
                fmt_minor(g.target_minor, cur),
                g.pct,
                g.deadline
                    .as_deref()
                    .map(|d| format!(", by {d}"))
                    .unwrap_or_default()
            ));
        }
        for u in c.upcoming.iter().take(3) {
            out.push_str(&format!(
                "- Bill due {}: {} {}\n",
                u.due,
                u.label,
                fmt_minor(u.amount_minor, cur)
            ));
        }
        if c.excluded_duplicates > 0 {
            out.push_str(&format!(
                "- {} manual rows left out because the statement already covers them\n",
                c.excluded_duplicates
            ));
        }
    }
    if v.needs_conversion {
        out.push_str("- JPY and USD are not combined: no exchange rate with a date is stored.\n");
    }
    if v.skipped_lines > 0 {
        out.push_str(&format!(
            "- {} stored lines could not be read and are not counted.\n",
            v.skipped_lines
        ));
    }
    out.push_str("</nexus_money>");
    Some(out)
}

/// True when no balance entry is dated within the last 7 days (today included): Amara then asks for the balance.
/// An unreadable store returns false (never nag blind).
/// Used by the Telegram money check-in (proactive.rs, decision 0045).
pub fn money_checkin_due(data: &Path, today: NaiveDate) -> bool {
    let Ok((entries, _)) = load(data) else {
        return false;
    };
    let cutoff = today - chrono::Duration::days(7);
    !entries.iter().any(|e| {
        e.kind == "balance"
            && e.voided_by.is_none()
            && parse_day(&e.date).is_some_and(|d| d > cutoff && d <= today)
    })
}

// ───────────────────────────── commands ─────────────────────────────

fn data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn money_entry_add(draft: EntryDraft) -> Result<MoneyEntry, String> {
    blocking(JoinErr::Code("store_error"), move || {
        entry_add_in(&data_dir(), &draft, now_jst().date_naive())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn money_entry_void(
    id: String,
    replacement: Option<EntryDraft>,
) -> Result<Option<MoneyEntry>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        entry_void_in(
            &data_dir(),
            &id,
            replacement.as_ref(),
            now_jst().date_naive(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn money_entries_list(range: Option<EntryRange>) -> Result<EntriesList, String> {
    blocking(JoinErr::Code("store_error"), move || {
        entries_list_in(&data_dir(), &range.unwrap_or_default())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn money_month_view(months: Option<usize>) -> Result<MonthView, String> {
    blocking(JoinErr::Code("store_error"), move || {
        month_view_in(&data_dir(), now_jst().date_naive(), months.unwrap_or(6))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn money_savings_goal_set(
    currency: String,
    target_minor: i64,
    deadline: Option<String>,
) -> Result<(), String> {
    blocking(JoinErr::Code("store_error"), move || {
        savings_goal_set_in(
            &data_dir(),
            &currency,
            target_minor,
            deadline.as_deref(),
            now_jst().date_naive(),
        )
    })
    .await
}

#[cfg(test)]
mod tests;
