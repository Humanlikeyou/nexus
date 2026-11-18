//! Treasury, phase 2 (plan `tasks/plans/treasury.md`, answers `docs/decisions/0032-treasury-answers-2026-11-15.md`):
//! business income streams, a generic CSV import, a manual attention log and the reads over them.
//!
//! Storage is one private WAL sqlite file, `<data_dir>/business/treasury.db`, opened with
//! `shared::sqlite::open_private_wal`. Amounts are integer minor units (JPY yen, USD cents); refunds
//! and fees are negative rows. A stream belongs to a venture id that must exist in the hand-authored
//! `business/ledger.json` (read through `business_ledger::read_ledger_in`, never written).
//!
//! Rules this file keeps (each has a test):
//! - Missing is `None`/`null`, never 0. No events: no total. No or zero minutes: no per-hour figure
//!   (`no_hours`). Both currencies in one total without a ledger rate: no total (`needs_rate`).
//! - The only exchange rate is the ledger's hand-entered, dated `fx`. No other rate source is read and
//!   nothing is fetched.
//! - `raw_ref` is the row number plus the file's SHA-256. The CSV line text is never stored, logged or
//!   returned, and neither is any amount (nothing here logs at all).
//! - Imports run in one transaction; the dedupe key is a hash of the row's content plus its occurrence
//!   number inside the file, so the same file twice adds nothing, an overlapping export adds only the
//!   new rows, and two real sales with the same date and amount stay two rows.
//! - Treasury never reads the personal store, and the personal code never reads this file
//!   (source-pin tests below).
//!
//! Outside calls: none. No network client, no URL, no new host. Local files and the local sqlite
//! file only. Only the generic column mapper ships; no payment-platform column names (decision 0032 #6).

use super::business_ledger::{read_ledger_in, FxRate};
use super::local_store::{clean_text, new_id};
use super::shared::sqlite::open_private_wal;
use super::shared::task::{blocking, JoinErr};
use super::shared::time::now_jst;
use chrono::{Datelike, Months, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_IMPORT_BYTES: u64 = 5 * 1024 * 1024;
const MAX_ROWS: usize = 100_000;
pub(super) const MAX_CELLS: usize = 64;
/// One event may not exceed this many minor units, so sums over `MAX_ROWS` rows stay inside i64.
const MAX_AMOUNT_MINOR: i64 = 1_000_000_000_000;
/// A ledger rate older than this many days is flagged stale (decision 0032 #7).
const STALE_DAYS: i64 = 7;
const MAX_MONTH_SPAN: usize = 36;
const MAX_ERRORS_SHOWN: usize = 50;
const SOURCE_MANUAL: &str = "manual";
const ADAPTER_GENERIC: &str = "generic";

const STREAM_TYPES: [&str; 5] = [
    "product_sales",
    "digital_product",
    "investment",
    "affiliate",
    "other",
];
const STREAM_STATUSES: [&str; 3] = ["active", "paused", "closed"];
const CURRENCIES: [&str; 2] = ["JPY", "USD"];

// ───────────────────────────── types ─────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryStream", rename_all = "camelCase")]
pub struct Stream {
    pub id: String,
    pub venture_id: String,
    pub name: String,
    pub stream_type: String,
    pub status: String,
    pub setup_date: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    /// True when the venture id is not in `ledger.json` (or there is no readable ledger).
    pub venture_missing: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryImportResult", rename_all = "camelCase")]
pub struct ImportResult {
    pub added: i64,
    pub skipped: i64,
    pub failed: i64,
    pub import_id: String,
}

/// A row-level problem. Never carries the row's text.
#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryRowError", rename_all = "camelCase")]
pub struct RowError {
    pub row: i64,
    /// `bad_date | bad_amount | bad_currency | short_row | wide_row`.
    pub code: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryImportPreview", rename_all = "camelCase")]
pub struct ImportPreview {
    pub rows_seen: i64,
    pub would_add: i64,
    pub would_skip: i64,
    pub failed: i64,
    /// At most 50, in file order.
    pub errors: Vec<RowError>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryAttentionRow", rename_all = "camelCase")]
pub struct AttentionRow {
    pub stream_id: String,
    pub day: String,
    pub minutes: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryStreamMonth", rename_all = "camelCase")]
pub struct StreamMonth {
    pub stream_id: String,
    /// `None` only for a stream with no events at all in the range.
    pub currency: Option<String>,
    /// `YYYY-MM`.
    pub month: String,
    /// `None` when the stream has no events that month (never 0). Negative is possible (refunds).
    pub total_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryTrendRow", rename_all = "camelCase")]
pub struct TrendRow {
    pub stream_id: String,
    pub currency: Option<String>,
    /// The last three complete calendar months (JST), oldest first. `None` = no events that month.
    pub months: [Option<i64>; 3],
    /// Mean of the months that have events (rounded half away from zero); `None` when none do.
    pub avg_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryPerHour", rename_all = "camelCase")]
pub struct PerHour {
    pub stream_id: String,
    /// Sum of logged minutes in the range; `None` when nothing was logged.
    pub minutes: Option<i64>,
    /// In `currency`. `None` when there are no events, or when the two currencies need a rate that is missing.
    pub income_minor: Option<i64>,
    /// The currency of `income_minor`: the stream's own, or `JPY` when a ledger rate combined both.
    pub currency: Option<String>,
    /// Set only when the ledger rate was used.
    pub fx_as_of: Option<String>,
    pub fx_stale: bool,
    pub per_hour_minor: Option<i64>,
    /// `no_hours | no_income | needs_rate | too_large` when `per_hour_minor` is `None`.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryFxStatus", rename_all = "camelCase")]
pub struct FxStatus {
    pub rate: Option<FxRate>,
    pub stale: bool,
}

/// The user's column mapping for the generic adapter. Column names are matched against the header
/// (trimmed, case-insensitive).
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename = "TreasuryCsvMapping", rename_all = "camelCase")]
pub struct CsvMapping {
    pub date_column: String,
    pub amount_column: String,
    /// A column holding `JPY` or `USD`. Wins over `default_currency`.
    pub currency_column: Option<String>,
    pub default_currency: Option<String>,
    /// An order or transaction id column. Only hashed into the dedupe key, never stored.
    pub ref_column: Option<String>,
    /// `ymd` (default), `mdy` or `dmy`. Separators `-`, `/` or `.`.
    pub date_format: Option<String>,
}

// ───────────────────────────── storage ─────────────────────────────

fn db_path(data: &Path) -> PathBuf {
    data.join("business").join("treasury.db")
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS streams(
  id TEXT PRIMARY KEY,
  venture_id TEXT NOT NULL,
  name TEXT NOT NULL,
  type TEXT NOT NULL CHECK(type IN ('product_sales','digital_product','investment','affiliate','other')),
  status TEXT NOT NULL CHECK(status IN ('active','paused','closed')),
  setup_date TEXT,
  notes TEXT,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS imports(
  id TEXT PRIMARY KEY,
  stream_id TEXT NOT NULL REFERENCES streams(id),
  adapter TEXT NOT NULL,
  file_sha256 TEXT,
  row_count INTEGER NOT NULL,
  added INTEGER NOT NULL,
  skipped INTEGER NOT NULL,
  status TEXT NOT NULL,
  at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS income_events(
  id TEXT PRIMARY KEY,
  stream_id TEXT NOT NULL REFERENCES streams(id),
  date TEXT NOT NULL,
  amount_minor INTEGER NOT NULL,
  currency TEXT NOT NULL CHECK(currency IN ('JPY','USD')),
  source TEXT NOT NULL,
  raw_ref TEXT NOT NULL,
  dedupe_key TEXT NOT NULL,
  import_id TEXT REFERENCES imports(id),
  UNIQUE(stream_id, dedupe_key)
);
CREATE INDEX IF NOT EXISTS income_events_stream_date ON income_events(stream_id, date);
CREATE TABLE IF NOT EXISTS attention_log(
  stream_id TEXT NOT NULL REFERENCES streams(id),
  day TEXT NOT NULL,
  minutes INTEGER NOT NULL CHECK(minutes >= 0 AND minutes <= 1440),
  source TEXT NOT NULL DEFAULT 'manual',
  PRIMARY KEY(stream_id, day, source)
);
CREATE TABLE IF NOT EXISTS segment_counts(
  stream_id TEXT NOT NULL REFERENCES streams(id),
  as_of TEXT NOT NULL,
  stage TEXT NOT NULL CHECK(stage IN ('lead','waitlist','first_purchase','repeat','lapsed')),
  segment TEXT NOT NULL,
  count INTEGER NOT NULL CHECK(count >= 0),
  import_id TEXT REFERENCES imports(id),
  PRIMARY KEY(stream_id, as_of, stage, segment)
);
CREATE TABLE IF NOT EXISTS sequence_drafts(
  id TEXT PRIMARY KEY,
  venture_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN ('welcome','post_purchase','reorder','win_back')),
  emails_json TEXT NOT NULL,
  checklist_json TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('draft','approved')),
  content_hash TEXT NOT NULL,
  approved_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS results_imports(
  id TEXT PRIMARY KEY,
  sequence_id TEXT NOT NULL REFERENCES sequence_drafts(id),
  file_sha256 TEXT,
  row_count INTEGER NOT NULL,
  status TEXT NOT NULL,
  at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sequence_results(
  sequence_id TEXT NOT NULL REFERENCES sequence_drafts(id),
  day TEXT NOT NULL,
  sends INTEGER CHECK(sends IS NULL OR sends >= 0),
  bounces INTEGER CHECK(bounces IS NULL OR bounces >= 0),
  unsubscribes INTEGER CHECK(unsubscribes IS NULL OR unsubscribes >= 0),
  purchases_after INTEGER CHECK(purchases_after IS NULL OR purchases_after >= 0),
  import_id TEXT REFERENCES results_imports(id),
  PRIMARY KEY(sequence_id, day)
);
";

pub(super) fn store_err<E>(_: E) -> String {
    "store_error".to_string()
}

pub(super) fn open_db(data: &Path) -> Result<Connection, String> {
    let biz = data.join("business");
    match std::fs::symlink_metadata(&biz) {
        Ok(m) if !m.file_type().is_dir() => return Err("unsafe_storage".into()),
        _ => {}
    }
    if let Ok(m) = std::fs::symlink_metadata(db_path(data)) {
        if !m.file_type().is_file() {
            return Err("unsafe_storage".into());
        }
    }
    let conn = open_private_wal(&db_path(data)).map_err(store_err)?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(store_err)?;
    conn.execute_batch(SCHEMA).map_err(store_err)?;
    Ok(conn)
}

pub(super) fn is_date(s: &str) -> bool {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() && s.len() == 10
}

fn is_month(s: &str) -> bool {
    s.len() == 7 && is_date(&format!("{s}-01"))
}

pub(super) fn now_iso() -> String {
    super::local_store::iso(&now_jst())
}

pub(super) fn today() -> NaiveDate {
    now_jst().date_naive()
}

// ───────────────────────────── streams ─────────────────────────────

fn row_to_stream(r: &rusqlite::Row<'_>) -> rusqlite::Result<Stream> {
    Ok(Stream {
        id: r.get(0)?,
        venture_id: r.get(1)?,
        name: r.get(2)?,
        stream_type: r.get(3)?,
        status: r.get(4)?,
        setup_date: r.get(5)?,
        notes: r.get(6)?,
        created_at: r.get(7)?,
        venture_missing: false,
    })
}

const STREAM_COLS: &str = "id, venture_id, name, type, status, setup_date, notes, created_at";

fn get_stream(conn: &Connection, id: &str) -> Result<Option<Stream>, String> {
    conn.query_row(
        &format!("SELECT {STREAM_COLS} FROM streams WHERE id = ?1"),
        [id],
        row_to_stream,
    )
    .optional()
    .map_err(store_err)
}

pub(super) fn stream_exists(conn: &Connection, id: &str) -> Result<bool, String> {
    Ok(get_stream(conn, id)?.is_some())
}

/// The set of ventures in the ledger; `None` when there is no readable ledger.
fn ledger_venture_ids(data: &Path) -> Option<HashSet<String>> {
    match read_ledger_in(data) {
        Ok(Some(l)) => Some(l.ventures.into_iter().map(|v| v.id).collect()),
        _ => None,
    }
}

fn stream_add_in(
    data: &Path,
    venture_id: &str,
    name: &str,
    stream_type: &str,
    setup_date: Option<&str>,
    notes: Option<&str>,
) -> Result<Stream, String> {
    let venture_id = venture_id.trim();
    if venture_id.is_empty() || venture_id.chars().count() > 64 {
        return Err("invalid_input".into());
    }
    let name = clean_text(name, 1, 80).map_err(|_| "invalid_input".to_string())?;
    if !STREAM_TYPES.contains(&stream_type) {
        return Err("invalid_input".into());
    }
    if let Some(d) = setup_date {
        if !is_date(d) {
            return Err("invalid_input".into());
        }
    }
    let notes = match notes {
        Some(n) => Some(clean_text(n, 0, 500).map_err(|_| "invalid_input".to_string())?),
        None => None,
    }
    .filter(|n| !n.is_empty());
    // The ledger decides which ventures exist. Never auto-created.
    match read_ledger_in(data) {
        Err(_) => return Err("ledger_invalid".into()),
        Ok(None) => return Err("ledger_missing".into()),
        Ok(Some(l)) => {
            if !l.ventures.iter().any(|v| v.id == venture_id) {
                return Err("unknown_venture".into());
            }
        }
    }
    let conn = open_db(data)?;
    let id = new_id("str");
    conn.execute(
        "INSERT INTO streams(id, venture_id, name, type, status, setup_date, notes, created_at)
         VALUES(?1, ?2, ?3, ?4, 'active', ?5, ?6, ?7)",
        params![
            id,
            venture_id,
            name,
            stream_type,
            setup_date,
            notes,
            now_iso()
        ],
    )
    .map_err(store_err)?;
    let mut s = get_stream(&conn, &id)?.ok_or_else(|| "store_error".to_string())?;
    s.venture_missing = false;
    Ok(s)
}

fn stream_update_in(
    data: &Path,
    id: &str,
    status: Option<&str>,
    name: Option<&str>,
    notes: Option<&str>,
) -> Result<Stream, String> {
    if let Some(s) = status {
        if !STREAM_STATUSES.contains(&s) {
            return Err("invalid_input".into());
        }
    }
    let name = match name {
        Some(n) => Some(clean_text(n, 1, 80).map_err(|_| "invalid_input".to_string())?),
        None => None,
    };
    // An empty string clears the notes.
    let notes = match notes {
        Some(n) => Some(clean_text(n, 0, 500).map_err(|_| "invalid_input".to_string())?),
        None => None,
    };
    let mut conn = open_db(data)?;
    if !stream_exists(&conn, id)? {
        return Err("unknown_stream".into());
    }
    // One transaction: a failure part-way leaves the stream unchanged (review LOW-1).
    let tx = conn.transaction().map_err(store_err)?;
    if let Some(s) = status {
        tx.execute(
            "UPDATE streams SET status = ?1 WHERE id = ?2",
            params![s, id],
        )
        .map_err(store_err)?;
    }
    if let Some(n) = name {
        tx.execute("UPDATE streams SET name = ?1 WHERE id = ?2", params![n, id])
            .map_err(store_err)?;
    }
    if let Some(n) = notes {
        let v: Option<String> = if n.is_empty() { None } else { Some(n) };
        tx.execute(
            "UPDATE streams SET notes = ?1 WHERE id = ?2",
            params![v, id],
        )
        .map_err(store_err)?;
    }
    tx.commit().map_err(store_err)?;
    let mut s = get_stream(&conn, id)?.ok_or_else(|| "store_error".to_string())?;
    s.venture_missing = ledger_venture_ids(data).is_none_or(|ids| !ids.contains(&s.venture_id));
    Ok(s)
}

fn streams_list_in(data: &Path) -> Result<Vec<Stream>, String> {
    let conn = open_db(data)?;
    let ids = ledger_venture_ids(data);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {STREAM_COLS} FROM streams ORDER BY created_at, id"
        ))
        .map_err(store_err)?;
    let rows = stmt.query_map([], row_to_stream).map_err(store_err)?;
    let mut out = Vec::new();
    for r in rows {
        let mut s = r.map_err(store_err)?;
        s.venture_missing = ids.as_ref().is_none_or(|ids| !ids.contains(&s.venture_id));
        out.push(s);
    }
    Ok(out)
}

// ───────────────────────────── CSV ─────────────────────────────

/// What an adapter turns one row into.
#[derive(Debug, Clone, PartialEq)]
struct EventDraft {
    date: String,
    amount_minor: i64,
    currency: &'static str,
    /// Hash of the order/transaction id column, if the user mapped one.
    ref_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RowCode {
    BadDate,
    BadAmount,
    BadCurrency,
    ShortRow,
}

impl RowCode {
    fn as_str(self) -> &'static str {
        match self {
            RowCode::BadDate => "bad_date",
            RowCode::BadAmount => "bad_amount",
            RowCode::BadCurrency => "bad_currency",
            RowCode::ShortRow => "short_row",
        }
    }
}

/// One source format. Phase 2 has the generic column mapper only: the user says which column is which.
/// Platform-specific adapters are written only from real header rows the user supplies.
trait CsvAdapter {
    fn id(&self) -> &'static str;
    /// True when this adapter can read a file with this header.
    fn detect(&self, header: &[String]) -> bool;
    fn parse(&self, row: &[String]) -> Result<EventDraft, RowCode>;
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum DateOrder {
    Ymd,
    Mdy,
    Dmy,
}

struct GenericMapper {
    mapping: CsvMapping,
    order: DateOrder,
    default_currency: Option<&'static str>,
    // Resolved column positions; `None` until `bind` has seen a header.
    date: usize,
    amount: usize,
    currency: Option<usize>,
    reference: Option<usize>,
}

pub(super) fn find_col(header: &[String], name: &str) -> Option<usize> {
    let want = name.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    header
        .iter()
        .position(|h| h.trim().trim_start_matches('\u{feff}').to_lowercase() == want)
}

fn currency_of(s: &str) -> Option<&'static str> {
    let up = s.trim().to_uppercase();
    CURRENCIES.iter().copied().find(|c| *c == up)
}

impl GenericMapper {
    /// Validates the mapping itself (before any file is read).
    fn new(mapping: CsvMapping) -> Result<Self, String> {
        let order = match mapping.date_format.as_deref().unwrap_or("ymd") {
            "ymd" => DateOrder::Ymd,
            "mdy" => DateOrder::Mdy,
            "dmy" => DateOrder::Dmy,
            _ => return Err("invalid_input".into()),
        };
        let default_currency = match mapping.default_currency.as_deref() {
            Some(c) => Some(currency_of(c).ok_or_else(|| "invalid_input".to_string())?),
            None => None,
        };
        let has_col = mapping
            .currency_column
            .as_deref()
            .is_some_and(|c| !c.trim().is_empty());
        if !has_col && default_currency.is_none() {
            return Err("invalid_input".into());
        }
        if mapping.date_column.trim().is_empty() || mapping.amount_column.trim().is_empty() {
            return Err("invalid_input".into());
        }
        Ok(Self {
            mapping,
            order,
            default_currency,
            date: 0,
            amount: 0,
            currency: None,
            reference: None,
        })
    }

    /// Resolves the columns against the header; `Err("bad_header")` when one is missing.
    fn bind(mut self, header: &[String]) -> Result<Self, String> {
        self.date = find_col(header, &self.mapping.date_column).ok_or("bad_header")?;
        self.amount = find_col(header, &self.mapping.amount_column).ok_or("bad_header")?;
        self.currency = match self.mapping.currency_column.as_deref() {
            Some(c) if !c.trim().is_empty() => Some(find_col(header, c).ok_or("bad_header")?),
            _ => None,
        };
        self.reference = match self.mapping.ref_column.as_deref() {
            Some(c) if !c.trim().is_empty() => Some(find_col(header, c).ok_or("bad_header")?),
            _ => None,
        };
        Ok(self)
    }
}

impl CsvAdapter for GenericMapper {
    fn id(&self) -> &'static str {
        ADAPTER_GENERIC
    }

    fn detect(&self, header: &[String]) -> bool {
        let need = |n: &str| find_col(header, n).is_some();
        need(&self.mapping.date_column)
            && need(&self.mapping.amount_column)
            && self
                .mapping
                .currency_column
                .as_deref()
                .is_none_or(|c| c.trim().is_empty() || need(c))
            && self
                .mapping
                .ref_column
                .as_deref()
                .is_none_or(|c| c.trim().is_empty() || need(c))
    }

    fn parse(&self, row: &[String]) -> Result<EventDraft, RowCode> {
        let cell = |i: usize| row.get(i).map(String::as_str).ok_or(RowCode::ShortRow);
        let date = parse_date(cell(self.date)?, self.order).ok_or(RowCode::BadDate)?;
        let currency = match self.currency {
            Some(i) => currency_of(cell(i)?)
                .or(self
                    .default_currency
                    .filter(|_| cell(i).is_ok_and(|c| c.trim().is_empty())))
                .ok_or(RowCode::BadCurrency)?,
            None => self.default_currency.ok_or(RowCode::BadCurrency)?,
        };
        let amount_minor = parse_amount(cell(self.amount)?, currency).ok_or(RowCode::BadAmount)?;
        let ref_hash = match self.reference {
            Some(i) => {
                let r = cell(i)?.trim();
                if r.is_empty() {
                    None
                } else {
                    Some(hex::encode(Sha256::digest(r.as_bytes())))
                }
            }
            None => None,
        };
        Ok(EventDraft {
            date,
            amount_minor,
            currency,
            ref_hash,
        })
    }
}

/// The registry. One adapter today.
fn adapter_for(adapter_id: &str, mapping: Option<CsvMapping>) -> Result<GenericMapper, String> {
    match adapter_id {
        ADAPTER_GENERIC => GenericMapper::new(mapping.ok_or_else(|| "invalid_input".to_string())?),
        _ => Err("unknown_adapter".into()),
    }
}

pub(super) fn parse_date(raw: &str, order: DateOrder) -> Option<String> {
    let token = raw
        .trim()
        .trim_start_matches('\u{feff}')
        .split([' ', 'T'])
        .next()?;
    let parts: Vec<&str> = token.split(['-', '/', '.']).collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let n = |i: usize| parts[i].parse::<u32>().ok();
    let (y, m, d, ylen) = match order {
        DateOrder::Ymd => (n(0)?, n(1)?, n(2)?, parts[0].len()),
        DateOrder::Mdy => (n(2)?, n(0)?, n(1)?, parts[2].len()),
        DateOrder::Dmy => (n(2)?, n(1)?, n(0)?, parts[2].len()),
    };
    if ylen != 4 {
        return None;
    }
    let date = NaiveDate::from_ymd_opt(i32::try_from(y).ok()?, m, d)?;
    Some(date.format("%Y-%m-%d").to_string())
}

/// Decimal text to integer minor units for the currency (JPY 0 decimals, USD 2). Commas only as thousands
/// groups, `(12.50)` is negative, more decimals than the currency has are accepted only when they are zeros.
fn parse_amount(raw: &str, currency: &str) -> Option<i64> {
    let mut s = raw.trim().trim_start_matches('\u{feff}').to_string();
    let mut neg = false;
    if s.starts_with('(') && s.ends_with(')') && s.len() > 2 {
        neg = true;
        s = s[1..s.len() - 1].trim().to_string();
    }
    if let Some(rest) = s.strip_prefix('-') {
        if neg {
            // "(-5)" is ambiguous: refuse.
            return None;
        }
        neg = true;
        s = rest.to_string();
    } else if let Some(rest) = s.strip_prefix('+') {
        s = rest.to_string();
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s.as_str(), ""),
    };
    if int_part.is_empty() || !frac_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Thousands groups: either no commas, or 1-3 digits then groups of exactly three.
    let int_digits: String = if int_part.contains(',') {
        let mut groups = int_part.split(',');
        let first = groups.next()?;
        if first.is_empty() || first.len() > 3 {
            return None;
        }
        let rest: Vec<&str> = groups.collect();
        if rest.iter().any(|g| g.len() != 3) {
            return None;
        }
        format!("{first}{}", rest.concat())
    } else {
        int_part.to_string()
    };
    if !int_digits.bytes().all(|b| b.is_ascii_digit()) || int_digits.len() > 15 {
        return None;
    }
    let decimals: usize = if currency == "USD" { 2 } else { 0 };
    if frac_part.len() > decimals && !frac_part[decimals..].bytes().all(|b| b == b'0') {
        return None;
    }
    let mut frac: String = frac_part.chars().take(decimals).collect();
    while frac.len() < decimals {
        frac.push('0');
    }
    let whole: i128 = format!("{int_digits}{frac}").parse().ok()?;
    let v = i64::try_from(whole).ok()?;
    if v > MAX_AMOUNT_MINOR {
        return None;
    }
    Some(if neg { -v } else { v })
}

/// RFC 4180-ish. Quotes, doubled quotes, newlines inside quotes, CRLF. Returns `(record number, cells)`
/// with the header as record 1; fully blank records are skipped but still counted. A record keeps at
/// most `MAX_CELLS + 1` cells (the extra one marks a wide row). More than `MAX_ROWS` records is an error.
pub(super) fn parse_csv(text: &str) -> Result<Vec<(usize, Vec<String>)>, String> {
    let mut rows: Vec<(usize, Vec<String>)> = Vec::new();
    let mut record_no = 0usize;
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let finish = |row: &mut Vec<String>,
                  cell: &mut String,
                  rows: &mut Vec<(usize, Vec<String>)>,
                  record_no: &mut usize|
     -> Result<(), String> {
        *record_no += 1;
        if row.len() <= MAX_CELLS {
            row.push(std::mem::take(cell));
        }
        cell.clear();
        let done = std::mem::take(row);
        if done.len() == 1 && done[0].trim().is_empty() {
            return Ok(());
        }
        if rows.len() > MAX_ROWS {
            return Err("too_many_rows".into());
        }
        rows.push((*record_no, done));
        Ok(())
    };
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            ',' => {
                if row.len() <= MAX_CELLS {
                    row.push(std::mem::take(&mut cell));
                }
                cell.clear();
            }
            '\r' => {}
            '\n' => finish(&mut row, &mut cell, &mut rows, &mut record_no)?,
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        finish(&mut row, &mut cell, &mut rows, &mut record_no)?;
    }
    Ok(rows)
}

// ───────────────────────────── import ─────────────────────────────

pub(super) fn read_import_file(path: &Path) -> Result<Vec<u8>, String> {
    use std::path::Component;
    if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        return Err("invalid_input".into());
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| "bad_file".to_string())?;
    if !meta.file_type().is_file() {
        return Err("bad_file".into());
    }
    if meta.len() > MAX_IMPORT_BYTES {
        return Err("too_large".into());
    }
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let f = o.open(path).map_err(|_| "bad_file".to_string())?;
    let m = f.metadata().map_err(|_| "bad_file".to_string())?;
    if !m.file_type().is_file() {
        return Err("bad_file".into());
    }
    let mut buf = Vec::new();
    f.take(MAX_IMPORT_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|_| "bad_file".to_string())?;
    if buf.len() as u64 > MAX_IMPORT_BYTES {
        return Err("too_large".into());
    }
    Ok(buf)
}

#[derive(Debug)]
struct Draft {
    row: usize,
    date: String,
    amount_minor: i64,
    currency: &'static str,
    dedupe_key: String,
}

#[derive(Debug)]
struct Parsed {
    sha: String,
    adapter: &'static str,
    drafts: Vec<Draft>,
    rows_seen: usize,
    errors: Vec<RowError>,
    failed: usize,
}

/// The dedupe recipe (version 1, generic adapter): a hash of the row's own content (optional id hash,
/// date, amount, currency) and its occurrence number among identical rows in the same file. No file hash
/// and no row number, so an overlapping export lines up with the rows already stored.
fn dedupe_key(
    adapter: &str,
    ref_hash: Option<&str>,
    date: &str,
    amount: i64,
    currency: &str,
    occurrence: u32,
) -> String {
    let s = format!(
        "v1|{adapter}|{}|{date}|{amount}|{currency}|{occurrence}",
        ref_hash.unwrap_or("-")
    );
    hex::encode(Sha256::digest(s.as_bytes()))
}

type ParsedRows = (Vec<Draft>, usize, Vec<RowError>, usize);

fn parse_file(adapter: &GenericMapper, bytes: &[u8]) -> Result<ParsedRows, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "bad_encoding".to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rows = parse_csv(text)?;
    let mut iter = rows.into_iter();
    let (_, header) = iter.next().ok_or_else(|| "bad_header".to_string())?;
    if !adapter.detect(&header) {
        return Err("bad_header".into());
    }
    let adapter = GenericMapper::new(adapter.mapping.clone())?.bind(&header)?;
    let mut drafts = Vec::new();
    let mut errors = Vec::new();
    let mut seen: HashMap<(Option<String>, String, i64, &'static str), u32> = HashMap::new();
    let mut rows_seen = 0usize;
    let mut failed = 0usize;
    for (row_no, cells) in iter {
        rows_seen += 1;
        let code = if cells.len() > MAX_CELLS {
            Err("wide_row")
        } else {
            adapter.parse(&cells).map_err(RowCode::as_str)
        };
        match code {
            Ok(d) => {
                let key = (
                    d.ref_hash.clone(),
                    d.date.clone(),
                    d.amount_minor,
                    d.currency,
                );
                let n = seen.entry(key).or_insert(0);
                *n += 1;
                drafts.push(Draft {
                    row: row_no,
                    dedupe_key: dedupe_key(
                        ADAPTER_GENERIC,
                        d.ref_hash.as_deref(),
                        &d.date,
                        d.amount_minor,
                        d.currency,
                        *n,
                    ),
                    date: d.date,
                    amount_minor: d.amount_minor,
                    currency: d.currency,
                });
            }
            Err(code) => {
                failed += 1;
                if errors.len() < MAX_ERRORS_SHOWN {
                    errors.push(RowError {
                        row: row_no as i64,
                        code: code.to_string(),
                    });
                }
            }
        }
    }
    Ok((drafts, rows_seen, errors, failed))
}

fn read_and_parse(
    adapter: &GenericMapper,
    path: &Path,
) -> Result<Parsed, (Option<String>, String)> {
    let bytes = read_import_file(path).map_err(|e| (None, e))?;
    let sha = hex::encode(Sha256::digest(&bytes));
    let (drafts, rows_seen, errors, failed) =
        parse_file(adapter, &bytes).map_err(|e| (Some(sha.clone()), e))?;
    Ok(Parsed {
        sha,
        adapter: adapter.id(),
        drafts,
        rows_seen,
        errors,
        failed,
    })
}

/// Best effort: the failure being reported is the import's, not this one's.
pub(super) fn record_failed(
    conn: &Connection,
    stream_id: &str,
    adapter: &str,
    sha: Option<&str>,
    code: &str,
) {
    let _ = conn.execute(
        "INSERT INTO imports(id, stream_id, adapter, file_sha256, row_count, added, skipped, status, at)
         VALUES(?1, ?2, ?3, ?4, 0, 0, 0, ?5, ?6)",
        params![new_id("imp"), stream_id, adapter, sha, format!("failed:{code}"), now_iso()],
    );
}

fn import_in(
    data: &Path,
    stream_id: &str,
    adapter_id: &str,
    mapping: Option<CsvMapping>,
    path: Option<&Path>,
) -> Result<ImportResult, String> {
    let path = path.ok_or_else(|| "cancelled".to_string())?;
    let adapter = adapter_for(adapter_id, mapping)?;
    let mut conn = open_db(data)?;
    if !stream_exists(&conn, stream_id)? {
        return Err("unknown_stream".into());
    }
    let parsed = match read_and_parse(&adapter, path) {
        Ok(p) => p,
        Err((sha, code)) => {
            record_failed(&conn, stream_id, adapter.id(), sha.as_deref(), &code);
            return Err(code);
        }
    };
    match commit_import(&mut conn, stream_id, &parsed) {
        Ok(r) => Ok(r),
        Err(code) => {
            record_failed(&conn, stream_id, parsed.adapter, Some(&parsed.sha), &code);
            Err(code)
        }
    }
}

/// One transaction: the import row and every event, or nothing.
fn commit_import(
    conn: &mut Connection,
    stream_id: &str,
    p: &Parsed,
) -> Result<ImportResult, String> {
    let tx = conn.transaction().map_err(store_err)?;
    let import_id = new_id("imp");
    tx.execute(
        "INSERT INTO imports(id, stream_id, adapter, file_sha256, row_count, added, skipped, status, at)
         VALUES(?1, ?2, ?3, ?4, ?5, 0, 0, 'ok', ?6)",
        params![import_id, stream_id, p.adapter, p.sha, p.rows_seen as i64, now_iso()],
    )
    .map_err(store_err)?;
    let mut added = 0i64;
    {
        let mut ins = tx
            .prepare(
                "INSERT OR IGNORE INTO income_events(id, stream_id, date, amount_minor, currency, source, raw_ref, dedupe_key, import_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(store_err)?;
        for d in &p.drafts {
            let raw_ref = format!("row:{};sha256:{}", d.row, p.sha);
            let n = ins
                .execute(params![
                    new_id("inc"),
                    stream_id,
                    d.date,
                    d.amount_minor,
                    d.currency,
                    p.adapter,
                    raw_ref,
                    d.dedupe_key,
                    import_id
                ])
                .map_err(store_err)?;
            added += n as i64;
        }
    }
    let skipped = p.drafts.len() as i64 - added;
    let status = if p.failed > 0 { "partial" } else { "ok" };
    tx.execute(
        "UPDATE imports SET added = ?1, skipped = ?2, status = ?3 WHERE id = ?4",
        params![added, skipped, status, import_id],
    )
    .map_err(store_err)?;
    tx.commit().map_err(store_err)?;
    Ok(ImportResult {
        added,
        skipped,
        failed: p.failed as i64,
        import_id,
    })
}

fn preview_in(
    data: &Path,
    stream_id: &str,
    adapter_id: &str,
    mapping: Option<CsvMapping>,
    path: Option<&Path>,
) -> Result<ImportPreview, String> {
    let path = path.ok_or_else(|| "cancelled".to_string())?;
    let adapter = adapter_for(adapter_id, mapping)?;
    let conn = open_db(data)?;
    if !stream_exists(&conn, stream_id)? {
        return Err("unknown_stream".into());
    }
    let p = read_and_parse(&adapter, path).map_err(|(_, code)| code)?;
    let mut would_skip = 0i64;
    let mut q = conn
        .prepare("SELECT 1 FROM income_events WHERE stream_id = ?1 AND dedupe_key = ?2")
        .map_err(store_err)?;
    for d in &p.drafts {
        if q.exists(params![stream_id, d.dedupe_key])
            .map_err(store_err)?
        {
            would_skip += 1;
        }
    }
    Ok(ImportPreview {
        rows_seen: p.rows_seen as i64,
        would_add: p.drafts.len() as i64 - would_skip,
        would_skip,
        failed: p.failed as i64,
        errors: p.errors,
    })
}

// ───────────────────────────── attention ─────────────────────────────

fn attention_set_in(data: &Path, stream_id: &str, day: &str, minutes: i64) -> Result<(), String> {
    if !is_date(day) || !(0..=1440).contains(&minutes) {
        return Err("invalid_input".into());
    }
    let conn = open_db(data)?;
    if !stream_exists(&conn, stream_id)? {
        return Err("unknown_stream".into());
    }
    conn.execute(
        "INSERT INTO attention_log(stream_id, day, minutes, source) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(stream_id, day, source) DO UPDATE SET minutes = excluded.minutes",
        params![stream_id, day, minutes, SOURCE_MANUAL],
    )
    .map_err(store_err)?;
    Ok(())
}

fn date_range(from: &str, to: &str) -> Result<(), String> {
    if !is_date(from) || !is_date(to) || from > to {
        return Err("invalid_input".into());
    }
    Ok(())
}

fn attention_list_in(
    data: &Path,
    stream_id: Option<&str>,
    from: &str,
    to: &str,
) -> Result<Vec<AttentionRow>, String> {
    date_range(from, to)?;
    let conn = open_db(data)?;
    if let Some(id) = stream_id {
        if !stream_exists(&conn, id)? {
            return Err("unknown_stream".into());
        }
    }
    let mut stmt = conn
        .prepare(
            "SELECT stream_id, day, SUM(minutes) FROM attention_log
             WHERE day BETWEEN ?1 AND ?2 AND (?3 IS NULL OR stream_id = ?3)
             GROUP BY stream_id, day ORDER BY day, stream_id",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map(params![from, to, stream_id], |r| {
            Ok(AttentionRow {
                stream_id: r.get(0)?,
                day: r.get(1)?,
                minutes: r.get(2)?,
            })
        })
        .map_err(store_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(store_err)
}

// ───────────────────────────── reads ─────────────────────────────

/// `(stream, currency, month) -> total` for months in `[from, to]` (`YYYY-MM`).
fn monthly_totals(
    conn: &Connection,
    from: &str,
    to: &str,
    stream_id: Option<&str>,
) -> Result<BTreeMap<(String, String, String), i64>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT stream_id, currency, substr(date, 1, 7), SUM(amount_minor) FROM income_events
             WHERE substr(date, 1, 7) BETWEEN ?1 AND ?2 AND (?3 IS NULL OR stream_id = ?3)
             GROUP BY stream_id, currency, substr(date, 1, 7)",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map(params![from, to, stream_id], |r| {
            Ok(((r.get(0)?, r.get(1)?, r.get(2)?), r.get::<_, i64>(3)?))
        })
        .map_err(store_err)?;
    rows.collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(store_err)
}

fn month_list(from: &str, to: &str) -> Result<Vec<String>, String> {
    if !is_month(from) || !is_month(to) || from > to {
        return Err("invalid_input".into());
    }
    let mut cur =
        NaiveDate::parse_from_str(&format!("{from}-01"), "%Y-%m-%d").map_err(store_err)?;
    let mut out = Vec::new();
    while cur.format("%Y-%m").to_string().as_str() <= to {
        out.push(cur.format("%Y-%m").to_string());
        if out.len() > MAX_MONTH_SPAN {
            return Err("invalid_input".into());
        }
        cur = cur
            .checked_add_months(Months::new(1))
            .ok_or_else(|| "invalid_input".to_string())?;
    }
    Ok(out)
}

fn stream_ids(conn: &Connection, only: Option<&str>) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT id FROM streams WHERE (?1 IS NULL OR id = ?1) ORDER BY created_at, id")
        .map_err(store_err)?;
    let rows = stmt.query_map([only], |r| r.get(0)).map_err(store_err)?;
    rows.collect::<Result<Vec<String>, _>>().map_err(store_err)
}

fn currencies_in(
    totals: &BTreeMap<(String, String, String), i64>,
    stream: &str,
) -> Vec<&'static str> {
    CURRENCIES
        .iter()
        .copied()
        .filter(|c| totals.keys().any(|(s, cur, _)| s == stream && cur == c))
        .collect()
}

fn income_month_in(
    data: &Path,
    from_month: &str,
    to_month: &str,
) -> Result<Vec<StreamMonth>, String> {
    let months = month_list(from_month, to_month)?;
    let conn = open_db(data)?;
    let totals = monthly_totals(&conn, from_month, to_month, None)?;
    let mut out = Vec::new();
    for sid in stream_ids(&conn, None)? {
        let currencies = currencies_in(&totals, &sid);
        if currencies.is_empty() {
            for m in &months {
                out.push(StreamMonth {
                    stream_id: sid.clone(),
                    currency: None,
                    month: m.clone(),
                    total_minor: None,
                });
            }
            continue;
        }
        for c in currencies {
            for m in &months {
                out.push(StreamMonth {
                    stream_id: sid.clone(),
                    currency: Some(c.to_string()),
                    month: m.clone(),
                    total_minor: totals
                        .get(&(sid.clone(), c.to_string(), m.clone()))
                        .copied(),
                });
            }
        }
    }
    Ok(out)
}

/// Mean of the known values, rounded half away from zero. `None` when there are none.
fn mean_minor(vals: &[Option<i64>]) -> Option<i64> {
    let known: Vec<i128> = vals.iter().flatten().map(|v| *v as i128).collect();
    if known.is_empty() {
        return None;
    }
    let (sum, n) = (known.iter().sum::<i128>(), known.len() as i128);
    let half = if sum < 0 { -n } else { n };
    i64::try_from((2 * sum + half) / (2 * n)).ok()
}

fn income_trend_in(
    data: &Path,
    stream_id: Option<&str>,
    today: NaiveDate,
) -> Result<Vec<TrendRow>, String> {
    let first = today
        .with_day(1)
        .ok_or_else(|| "invalid_input".to_string())?;
    let key = |back: u32| -> Result<String, String> {
        Ok(first
            .checked_sub_months(Months::new(back))
            .ok_or_else(|| "invalid_input".to_string())?
            .format("%Y-%m")
            .to_string())
    };
    let months = [key(3)?, key(2)?, key(1)?];
    let conn = open_db(data)?;
    if let Some(id) = stream_id {
        if !stream_exists(&conn, id)? {
            return Err("unknown_stream".into());
        }
    }
    let totals = monthly_totals(&conn, &months[0], &months[2], stream_id)?;
    let mut out = Vec::new();
    for sid in stream_ids(&conn, stream_id)? {
        let currencies = currencies_in(&totals, &sid);
        let mut push = |currency: Option<&str>| {
            let vals: [Option<i64>; 3] = std::array::from_fn(|i| {
                currency.and_then(|c| {
                    totals
                        .get(&(sid.clone(), c.to_string(), months[i].clone()))
                        .copied()
                })
            });
            out.push(TrendRow {
                stream_id: sid.clone(),
                currency: currency.map(str::to_string),
                avg_minor: mean_minor(&vals),
                months: vals,
            });
        };
        if currencies.is_empty() {
            push(None);
        }
        for c in currencies {
            push(Some(c));
        }
    }
    Ok(out)
}

/// The ledger rate's age in whole days against `today` (negative when dated in the future).
fn fx_stale(rate: &FxRate, today: NaiveDate) -> bool {
    NaiveDate::parse_from_str(&rate.as_of, "%Y-%m-%d")
        .map(|d| (today - d).num_days() > STALE_DAYS)
        .unwrap_or(true)
}

fn fx_status_in(data: &Path, today: NaiveDate) -> Result<FxStatus, String> {
    match read_ledger_in(data) {
        Err(_) => Err("ledger_invalid".into()),
        Ok(None) => Ok(FxStatus {
            rate: None,
            stale: false,
        }),
        Ok(Some(l)) => Ok(match l.fx {
            Some(r) => FxStatus {
                stale: fx_stale(&r, today),
                rate: Some(r),
            },
            None => FxStatus {
                rate: None,
                stale: false,
            },
        }),
    }
}

/// Income and reason for one stream's per-currency sums. A single currency needs no rate; JPY and USD
/// together are combined into JPY only with the ledger rate, and never otherwise.
fn combine_income(
    sums: &[(&'static str, i64)],
    fx: Option<&FxRate>,
    today: NaiveDate,
) -> (Option<i64>, Option<String>, Option<String>, bool) {
    match sums {
        [] => (None, None, None, false),
        [(c, v)] => (Some(*v), Some(c.to_string()), None, false),
        _ => {
            let Some(rate) = fx else {
                return (None, None, None, false);
            };
            let jpy: i64 = sums
                .iter()
                .filter(|(c, _)| *c == "JPY")
                .map(|(_, v)| *v)
                .sum();
            let cents: i64 = sums
                .iter()
                .filter(|(c, _)| *c == "USD")
                .map(|(_, v)| *v)
                .sum();
            let usd_in_jpy = (cents as f64) / 100.0 / rate.usd_per_jpy;
            if !usd_in_jpy.is_finite() || usd_in_jpy.abs() > 9.0e15 {
                return (None, None, None, false);
            }
            let total = jpy.checked_add(usd_in_jpy.round() as i64);
            (
                total,
                total.map(|_| "JPY".to_string()),
                total.map(|_| rate.as_of.clone()),
                fx_stale(rate, today),
            )
        }
    }
}

fn per_hour_in(
    data: &Path,
    from: &str,
    to: &str,
    fx: Option<&FxRate>,
    today: NaiveDate,
) -> Result<Vec<PerHour>, String> {
    date_range(from, to)?;
    let conn = open_db(data)?;
    let mut out = Vec::new();
    for sid in stream_ids(&conn, None)? {
        let (mins, n): (Option<i64>, i64) = conn
            .query_row(
                "SELECT SUM(minutes), COUNT(*) FROM attention_log
                 WHERE stream_id = ?1 AND day BETWEEN ?2 AND ?3",
                params![sid, from, to],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(store_err)?;
        let minutes = if n > 0 { mins } else { None };
        let mut stmt = conn
            .prepare(
                "SELECT currency, SUM(amount_minor) FROM income_events
                 WHERE stream_id = ?1 AND date BETWEEN ?2 AND ?3 GROUP BY currency ORDER BY currency",
            )
            .map_err(store_err)?;
        let found: Vec<(String, i64)> = stmt
            .query_map(params![sid, from, to], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(store_err)?
            .collect::<Result<_, _>>()
            .map_err(store_err)?;
        let sums: Vec<(&'static str, i64)> = found
            .iter()
            .filter_map(|(c, v)| CURRENCIES.iter().copied().find(|k| k == c).map(|k| (k, *v)))
            .collect();
        let (income, currency, fx_as_of, stale) = combine_income(&sums, fx, today);
        let hours = minutes.filter(|m| *m > 0);
        let reason = if hours.is_none() {
            Some("no_hours")
        } else if sums.is_empty() {
            Some("no_income")
        } else if income.is_none() {
            Some(if sums.len() > 1 && fx.is_some() {
                "too_large"
            } else {
                "needs_rate"
            })
        } else {
            None
        };
        let per_hour = match (reason, income, hours) {
            (None, Some(i), Some(m)) => {
                let num = i as i128 * 60;
                let den = m as i128;
                let half = if num < 0 { -den } else { den };
                i64::try_from((2 * num + half) / (2 * den)).ok()
            }
            _ => None,
        };
        out.push(PerHour {
            stream_id: sid,
            minutes,
            income_minor: income,
            currency,
            fx_as_of,
            fx_stale: stale,
            per_hour_minor: per_hour,
            reason: reason.map(str::to_string),
        });
    }
    Ok(out)
}

// ───────────────────────────── commands ─────────────────────────────

static DIALOG_OPEN: AtomicBool = AtomicBool::new(false);

/// Held for the whole pick call (dialog and import). Dropping it frees the slot, panics included.
pub(super) struct Slot;

impl Slot {
    pub(super) fn take() -> Option<Slot> {
        DIALOG_OPEN
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        DIALOG_OPEN.store(false, Ordering::Release);
    }
}

/// The native open-file dialog. The renderer never supplies a path; `None` when cancelled.
async fn pick_csv(app: tauri::AppHandle) -> Result<Option<PathBuf>, String> {
    pick_csv_titled(app, "Choose an income CSV").await
}

/// Same dialog with another title (the counts and results imports in the sibling modules).
pub(super) async fn pick_csv_titled(
    app: tauri::AppHandle,
    title: &'static str,
) -> Result<Option<PathBuf>, String> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_title(title)
            .add_filter("CSV", &["csv"]);
        // Parent the sheet to the main window when there is one, so it can't hide behind it.
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_pick_file()
    })
    .await
    .map_err(|_| "failed".to_string())?;
    Ok(picked.and_then(|p| p.into_path().ok()))
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_stream_add(
    venture_id: String,
    name: String,
    stream_type: String,
    setup_date: Option<String>,
    notes: Option<String>,
) -> Result<Stream, String> {
    blocking(JoinErr::Code("store_error"), move || {
        stream_add_in(
            &crate::app_paths::data_dir(),
            &venture_id,
            &name,
            &stream_type,
            setup_date.as_deref(),
            notes.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_stream_update(
    id: String,
    status: Option<String>,
    name: Option<String>,
    notes: Option<String>,
) -> Result<Stream, String> {
    blocking(JoinErr::Code("store_error"), move || {
        stream_update_in(
            &crate::app_paths::data_dir(),
            &id,
            status.as_deref(),
            name.as_deref(),
            notes.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_streams_list() -> Result<Vec<Stream>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        streams_list_in(&crate::app_paths::data_dir())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_import_pick(
    app: tauri::AppHandle,
    stream_id: String,
    adapter_id: String,
    mapping: Option<CsvMapping>,
) -> Result<ImportResult, String> {
    let _slot = Slot::take().ok_or_else(|| "busy".to_string())?;
    let path = pick_csv(app).await?;
    blocking(JoinErr::Code("failed"), move || {
        import_in(
            &crate::app_paths::data_dir(),
            &stream_id,
            &adapter_id,
            mapping,
            path.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_import_preview(
    app: tauri::AppHandle,
    stream_id: String,
    adapter_id: String,
    mapping: Option<CsvMapping>,
) -> Result<ImportPreview, String> {
    let _slot = Slot::take().ok_or_else(|| "busy".to_string())?;
    let path = pick_csv(app).await?;
    blocking(JoinErr::Code("failed"), move || {
        preview_in(
            &crate::app_paths::data_dir(),
            &stream_id,
            &adapter_id,
            mapping,
            path.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_attention_set(
    stream_id: String,
    day: String,
    minutes: i64,
) -> Result<(), String> {
    blocking(JoinErr::Code("store_error"), move || {
        attention_set_in(&crate::app_paths::data_dir(), &stream_id, &day, minutes)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_attention_list(
    stream_id: Option<String>,
    from: String,
    to: String,
) -> Result<Vec<AttentionRow>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        attention_list_in(
            &crate::app_paths::data_dir(),
            stream_id.as_deref(),
            &from,
            &to,
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_income_month(
    from_month: String,
    to_month: String,
) -> Result<Vec<StreamMonth>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        income_month_in(&crate::app_paths::data_dir(), &from_month, &to_month)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_income_trend(stream_id: Option<String>) -> Result<Vec<TrendRow>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        income_trend_in(&crate::app_paths::data_dir(), stream_id.as_deref(), today())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_per_hour(from: String, to: String) -> Result<Vec<PerHour>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        let data = crate::app_paths::data_dir();
        // A missing or unreadable ledger means no rate: mixed-currency figures stay null.
        let fx = match read_ledger_in(&data) {
            Ok(Some(l)) => l.fx,
            _ => None,
        };
        per_hour_in(&data, &from, &to, fx.as_ref(), today())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_fx_status() -> Result<FxStatus, String> {
    blocking(JoinErr::Code("store_error"), move || {
        fx_status_in(&crate::app_paths::data_dir(), today())
    })
    .await
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("amara-treasury-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_ledger(dir: &Path, fx: Option<(&str, f64)>, ventures: &[&str]) {
        let biz = dir.join("business");
        fs::create_dir_all(&biz).unwrap();
        let v: Vec<String> = ventures
            .iter()
            .map(|id| {
                format!(
                    r#"{{"id":"{id}","name":"{id}","stage":"idea","health":"on_track","note":""}}"#
                )
            })
            .collect();
        let fxj = match fx {
            Some((d, r)) => format!(r#"{{"usdPerJpy":{r},"asOf":"{d}"}}"#),
            None => "null".into(),
        };
        fs::write(
            biz.join("ledger.json"),
            format!(r#"{{"version":1,"fx":{fxj},"ventures":[{}]}}"#, v.join(",")),
        )
        .unwrap();
    }

    fn mapping() -> CsvMapping {
        CsvMapping {
            date_column: "Date".into(),
            amount_column: "Amount".into(),
            currency_column: Some("Cur".into()),
            default_currency: None,
            ref_column: None,
            date_format: None,
        }
    }

    fn setup(tag: &str) -> (PathBuf, String) {
        let d = temp_dir(tag);
        write_ledger(&d, None, &["v1"]);
        let s = stream_add_in(&d, "v1", "Shop", "product_sales", None, None).unwrap();
        (d, s.id)
    }

    fn csv_file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        p
    }

    fn count(d: &Path, sql: &str) -> i64 {
        open_db(d)
            .unwrap()
            .query_row(sql, [], |r| r.get(0))
            .unwrap()
    }

    const FILE_A: &str =
        "Date,Amount,Cur\n2026-11-01,1000,JPY\n2026-11-02,2500,JPY\n2026-11-03,12.50,USD\n";

    #[test]
    fn import_twice_adds_nothing_and_skips_every_row() {
        let (d, sid) = setup("twice");
        let f = csv_file(&d, "a.csv", FILE_A);
        let r1 = import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        assert_eq!((r1.added, r1.skipped, r1.failed), (3, 0, 0));
        let rows = count(&d, "SELECT COUNT(*) FROM income_events");
        let r2 = import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        assert_eq!((r2.added, r2.skipped), (0, 3));
        assert_eq!(count(&d, "SELECT COUNT(*) FROM income_events"), rows);
        assert_eq!(
            count(&d, "SELECT COUNT(*) FROM imports WHERE status = 'ok'"),
            2
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn an_overlapping_file_adds_only_the_new_rows() {
        let (d, sid) = setup("overlap");
        let a = csv_file(&d, "a.csv", FILE_A);
        import_in(&d, &sid, "generic", Some(mapping()), Some(&a)).unwrap();
        let b = csv_file(
            &d,
            "b.csv",
            "Date,Amount,Cur\n2026-11-02,2500,JPY\n2026-11-03,12.50,USD\n2026-11-04,700,JPY\n",
        );
        let r = import_in(&d, &sid, "generic", Some(mapping()), Some(&b)).unwrap();
        assert_eq!((r.added, r.skipped), (1, 2));
        assert_eq!(count(&d, "SELECT COUNT(*) FROM income_events"), 4);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn same_date_same_amount_real_sales_stay_distinct_and_still_dedupe_on_reimport() {
        let (d, sid) = setup("samesale");
        let f = csv_file(
            &d,
            "s.csv",
            "Date,Amount,Cur\n2026-11-01,1000,JPY\n2026-11-01,1000,JPY\n2026-11-01,1000,JPY\n",
        );
        let r = import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        assert_eq!(r.added, 3);
        let again = import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        assert_eq!((again.added, again.skipped), (0, 3));
        // The recipe is fixed: a change here would silently re-import everyone's history.
        assert_eq!(
            dedupe_key("generic", None, "2026-11-01", 1000, "JPY", 1),
            hex::encode(Sha256::digest(b"v1|generic|-|2026-11-01|1000|JPY|1"))
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_mapped_ref_column_separates_rows_and_is_never_stored() {
        let (d, sid) = setup("refcol");
        let mut m = mapping();
        m.ref_column = Some("Order".into());
        let f = csv_file(
            &d,
            "r.csv",
            "Date,Amount,Cur,Order\n2026-11-01,1000,JPY,SECRET-ORDER-1\n2026-11-01,1000,JPY,SECRET-ORDER-2\n",
        );
        assert_eq!(
            import_in(&d, &sid, "generic", Some(m.clone()), Some(&f))
                .unwrap()
                .added,
            2
        );
        let dump = fs::read(db_path(&d)).unwrap();
        assert!(!String::from_utf8_lossy(&dump).contains("SECRET-ORDER"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn raw_ref_is_row_number_and_file_hash_only() {
        let (d, sid) = setup("rawref");
        let f = csv_file(&d, "a.csv", FILE_A);
        import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        let sha = hex::encode(Sha256::digest(FILE_A.as_bytes()));
        let refs: Vec<String> = {
            let c = open_db(&d).unwrap();
            let mut s = c
                .prepare("SELECT raw_ref FROM income_events ORDER BY date")
                .unwrap();
            s.query_map([], |r| r.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        assert_eq!(refs[0], format!("row:2;sha256:{sha}"));
        assert_eq!(refs[2], format!("row:4;sha256:{sha}"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_mid_import_failure_leaves_nothing_partial() {
        let (d, sid) = setup("txn");
        // A real database failure on the third row: a trigger aborts the insert.
        open_db(&d)
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER boom BEFORE INSERT ON income_events WHEN NEW.amount_minor = 777
                 BEGIN SELECT RAISE(ABORT, 'x'); END;",
            )
            .unwrap();
        let f = csv_file(
            &d,
            "t.csv",
            "Date,Amount,Cur\n2026-11-01,100,JPY\n2026-11-02,200,JPY\n2026-11-03,777,JPY\n2026-11-04,400,JPY\n",
        );
        assert_eq!(
            import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap_err(),
            "store_error"
        );
        assert_eq!(count(&d, "SELECT COUNT(*) FROM income_events"), 0);
        assert_eq!(
            count(&d, "SELECT COUNT(*) FROM imports WHERE status = 'ok'"),
            0
        );
        assert_eq!(
            count(
                &d,
                "SELECT COUNT(*) FROM imports WHERE status = 'failed:store_error'"
            ),
            1
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn bad_files_fail_with_fixed_codes_and_record_a_failed_import() {
        let (d, sid) = setup("badfile");
        let m = || Some(mapping());
        // Wrong header.
        let f = csv_file(&d, "h.csv", "Foo,Bar\n1,2\n");
        assert_eq!(
            import_in(&d, &sid, "generic", m(), Some(&f)).unwrap_err(),
            "bad_header"
        );
        // Wrong encoding.
        let p = d.join("e.csv");
        fs::write(&p, [0x44u8, 0x61, 0xff, 0xfe, 0x0a]).unwrap();
        assert_eq!(
            import_in(&d, &sid, "generic", m(), Some(&p)).unwrap_err(),
            "bad_encoding"
        );
        // Over size.
        let big = d.join("big.csv");
        fs::write(&big, vec![b'a'; (MAX_IMPORT_BYTES + 1) as usize]).unwrap();
        assert_eq!(
            import_in(&d, &sid, "generic", m(), Some(&big)).unwrap_err(),
            "too_large"
        );
        // Symlink.
        #[cfg(unix)]
        {
            let real = csv_file(&d, "real.csv", FILE_A);
            let link = d.join("link.csv");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert_eq!(
                import_in(&d, &sid, "generic", m(), Some(&link)).unwrap_err(),
                "bad_file"
            );
        }
        // Relative path and a directory.
        assert_eq!(
            import_in(&d, &sid, "generic", m(), Some(Path::new("a.csv"))).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            import_in(&d, &sid, "generic", m(), Some(&d)).unwrap_err(),
            "bad_file"
        );
        assert!(
            count(
                &d,
                "SELECT COUNT(*) FROM imports WHERE status LIKE 'failed:%'"
            ) >= 5
        );
        assert_eq!(count(&d, "SELECT COUNT(*) FROM income_events"), 0);
        // Cancelled and unknown inputs record nothing and use fixed codes.
        assert_eq!(
            import_in(&d, &sid, "generic", m(), None).unwrap_err(),
            "cancelled"
        );
        assert_eq!(
            import_in(&d, &sid, "stripe", m(), Some(&f)).unwrap_err(),
            "unknown_adapter"
        );
        assert_eq!(
            import_in(&d, "nope", "generic", m(), Some(&f)).unwrap_err(),
            "unknown_stream"
        );
        assert_eq!(
            import_in(&d, &sid, "generic", None, Some(&f)).unwrap_err(),
            "invalid_input"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn bad_rows_are_counted_and_never_quoted() {
        let (d, sid) = setup("badrows");
        let f = csv_file(
            &d,
            "b.csv",
            "Date,Amount,Cur\n2026-11-01,1000,JPY\nnot-a-date,5,JPY\n2026-11-02,abc,JPY\n2026-11-03,5,EUR\n2026-11-04,1.5,JPY\n2026-11-05\n",
        );
        let p = preview_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        assert_eq!((p.rows_seen, p.would_add, p.failed), (6, 1, 5));
        let codes: Vec<&str> = p.errors.iter().map(|e| e.code.as_str()).collect();
        assert_eq!(
            codes,
            [
                "bad_date",
                "bad_amount",
                "bad_currency",
                "bad_amount",
                "short_row"
            ]
        );
        assert_eq!(p.errors[0].row, 3);
        assert!(!serde_json::to_string(&p).unwrap().contains("not-a-date"));
        // Preview writes nothing.
        assert_eq!(count(&d, "SELECT COUNT(*) FROM income_events"), 0);
        assert_eq!(count(&d, "SELECT COUNT(*) FROM imports"), 0);
        let r = import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        assert_eq!((r.added, r.failed), (1, 5));
        assert_eq!(
            count(&d, "SELECT COUNT(*) FROM imports WHERE status = 'partial'"),
            1
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn preview_counts_what_would_be_skipped() {
        let (d, sid) = setup("preview");
        let a = csv_file(&d, "a.csv", FILE_A);
        import_in(&d, &sid, "generic", Some(mapping()), Some(&a)).unwrap();
        let p = preview_in(&d, &sid, "generic", Some(mapping()), Some(&a)).unwrap();
        assert_eq!((p.rows_seen, p.would_add, p.would_skip), (3, 0, 3));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn amounts_dates_and_currencies_parse_strictly() {
        assert_eq!(parse_amount("1,234", "JPY"), Some(1234));
        assert_eq!(parse_amount("1,23", "JPY"), None);
        assert_eq!(parse_amount("12.50", "USD"), Some(1250));
        assert_eq!(parse_amount("12.5", "USD"), Some(1250));
        assert_eq!(parse_amount("12.500", "USD"), Some(1250));
        assert_eq!(parse_amount("12.505", "USD"), None);
        assert_eq!(parse_amount("-12.50", "USD"), Some(-1250));
        assert_eq!(parse_amount("(12.50)", "USD"), Some(-1250));
        assert_eq!(parse_amount("(-5)", "USD"), None);
        assert_eq!(parse_amount("100.00", "JPY"), Some(100));
        assert_eq!(parse_amount("100.5", "JPY"), None);
        assert_eq!(parse_amount("", "JPY"), None);
        assert_eq!(parse_amount("1e3", "JPY"), None);
        assert_eq!(parse_amount("9999999999999", "JPY"), None);
        assert_eq!(parse_amount("1000000000000", "JPY"), Some(MAX_AMOUNT_MINOR));
        assert_eq!(
            parse_date("2026-11-05", DateOrder::Ymd).as_deref(),
            Some("2026-11-05")
        );
        assert_eq!(
            parse_date("2026/1/5 10:00", DateOrder::Ymd).as_deref(),
            Some("2026-01-05")
        );
        assert_eq!(
            parse_date("11/05/2026", DateOrder::Mdy).as_deref(),
            Some("2026-11-05")
        );
        assert_eq!(
            parse_date("05.11.2026", DateOrder::Dmy).as_deref(),
            Some("2026-11-05")
        );
        assert_eq!(parse_date("2026-02-30", DateOrder::Ymd), None);
        assert_eq!(parse_date("26-11-05", DateOrder::Ymd), None);
    }

    #[test]
    fn a_default_currency_works_without_a_currency_column() {
        let (d, sid) = setup("defcur");
        let mut m = mapping();
        m.currency_column = None;
        m.default_currency = Some("usd".into());
        let f = csv_file(&d, "u.csv", "date,AMOUNT\n11/05/2026,\"1,200.00\"\n");
        m.date_format = Some("mdy".into());
        let r = import_in(&d, &sid, "generic", Some(m), Some(&f)).unwrap();
        assert_eq!(r.added, 1);
        assert_eq!(
            count(
                &d,
                "SELECT amount_minor FROM income_events WHERE currency = 'USD'"
            ),
            120000
        );
        // No currency source at all is refused before any file is read.
        let mut none = mapping();
        none.currency_column = None;
        assert_eq!(
            import_in(&d, &sid, "generic", Some(none), Some(&f)).unwrap_err(),
            "invalid_input"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn refunds_reduce_the_month_and_a_refund_only_month_is_negative() {
        let (d, sid) = setup("refund");
        let f = csv_file(
            &d,
            "r.csv",
            "Date,Amount,Cur\n2026-10-05,-300,JPY\n2026-11-01,1000,JPY\n2026-11-09,-400,JPY\n",
        );
        import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        let rows = income_month_in(&d, "2026-10", "2026-11").unwrap();
        let tot: Vec<Option<i64>> = rows.iter().map(|r| r.total_minor).collect();
        assert_eq!(tot, [Some(-300), Some(600)]);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn null_not_zero_for_income_months_and_trend() {
        let (d, sid) = setup("nulls");
        let rows = income_month_in(&d, "2026-09", "2026-11").unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows
            .iter()
            .all(|r| r.total_minor.is_none() && r.currency.is_none()));
        let t = income_trend_in(&d, None, NaiveDate::from_ymd_opt(2026, 11, 15).unwrap()).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].months, t[0].avg_minor), ([None, None, None], None));
        // One event in September only: the other months have no events and stay null, not 0.
        let f = csv_file(&d, "n.csv", "Date,Amount,Cur\n2026-09-10,900,JPY\n");
        import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        let t = income_trend_in(
            &d,
            Some(&sid),
            NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(),
        )
        .unwrap();
        assert_eq!(t[0].currency.as_deref(), Some("JPY"));
        assert_eq!(t[0].months, [None, Some(900), None]);
        assert_eq!(t[0].avg_minor, Some(900));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn trend_uses_the_last_three_complete_months_and_averages_known_ones() {
        let (d, sid) = setup("trend");
        let f = csv_file(
            &d,
            "t.csv",
            "Date,Amount,Cur\n2026-08-01,300,JPY\n2026-10-20,100,JPY\n2026-11-02,99999,JPY\n",
        );
        import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        let t = income_trend_in(
            &d,
            Some(&sid),
            NaiveDate::from_ymd_opt(2026, 11, 15).unwrap(),
        )
        .unwrap();
        assert_eq!(t[0].months, [Some(300), None, Some(100)]);
        assert_eq!(t[0].avg_minor, Some(200));
        assert_eq!(mean_minor(&[Some(1), Some(2)]), Some(2));
        assert_eq!(mean_minor(&[Some(-1), Some(-2)]), Some(-2));
        assert_eq!(mean_minor(&[None]), None);
        assert_eq!(
            income_trend_in(
                &d,
                Some("nope"),
                NaiveDate::from_ymd_opt(2026, 11, 15).unwrap()
            )
            .unwrap_err(),
            "unknown_stream"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn per_hour_is_null_without_hours_and_with_zero_hours() {
        let (d, sid) = setup("perhour");
        let f = csv_file(&d, "p.csv", "Date,Amount,Cur\n2026-11-01,6000,JPY\n");
        import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let r = per_hour_in(&d, "2026-11-01", "2026-11-30", None, today).unwrap();
        assert_eq!(
            (
                r[0].minutes,
                r[0].income_minor,
                r[0].per_hour_minor,
                r[0].reason.as_deref()
            ),
            (None, Some(6000), None, Some("no_hours"))
        );
        attention_set_in(&d, &sid, "2026-11-02", 0).unwrap();
        let r = per_hour_in(&d, "2026-11-01", "2026-11-30", None, today).unwrap();
        assert_eq!((r[0].minutes, r[0].per_hour_minor), (Some(0), None));
        assert_eq!(r[0].reason.as_deref(), Some("no_hours"));
        attention_set_in(&d, &sid, "2026-11-03", 120).unwrap();
        attention_set_in(&d, &sid, "2026-11-03", 180).unwrap(); // upsert
        let r = per_hour_in(&d, "2026-11-01", "2026-11-30", None, today).unwrap();
        assert_eq!(
            (r[0].minutes, r[0].per_hour_minor, r[0].reason.clone()),
            (Some(180), Some(2000), None)
        );
        assert_eq!(r[0].currency.as_deref(), Some("JPY"));
        // Hours but no income: null with its own reason, never 0.
        let r = per_hour_in(&d, "2026-12-01", "2026-12-31", None, today).unwrap();
        assert_eq!(
            (r[0].income_minor, r[0].reason.as_deref()),
            (None, Some("no_hours"))
        );
        attention_set_in(&d, &sid, "2026-12-03", 60).unwrap();
        let r = per_hour_in(&d, "2026-12-01", "2026-12-31", None, today).unwrap();
        assert_eq!(
            (
                r[0].income_minor,
                r[0].per_hour_minor,
                r[0].reason.as_deref()
            ),
            (None, None, Some("no_income"))
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn mixed_currency_needs_the_ledger_rate_and_never_a_made_up_one() {
        let (d, sid) = setup("fx");
        let f = csv_file(
            &d,
            "m.csv",
            "Date,Amount,Cur\n2026-11-01,3000,JPY\n2026-11-02,10.00,USD\n",
        );
        import_in(&d, &sid, "generic", Some(mapping()), Some(&f)).unwrap();
        attention_set_in(&d, &sid, "2026-11-03", 60).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        // No rate: no combined total.
        let r = per_hour_in(&d, "2026-11-01", "2026-11-30", None, today).unwrap();
        assert_eq!(
            (
                r[0].income_minor,
                r[0].per_hour_minor,
                r[0].reason.as_deref(),
                r[0].fx_as_of.clone()
            ),
            (None, None, Some("needs_rate"), None)
        );
        // Per-currency totals are still safe.
        assert_eq!(income_month_in(&d, "2026-11", "2026-11").unwrap().len(), 2);
        // With the ledger rate (0.0100 USD per JPY, so $10.00 = 1000 yen) and its date.
        let rate = FxRate {
            usd_per_jpy: 0.01,
            as_of: "2026-11-10".into(),
        };
        let r = per_hour_in(&d, "2026-11-01", "2026-11-30", Some(&rate), today).unwrap();
        assert_eq!(r[0].income_minor, Some(4000));
        assert_eq!(r[0].currency.as_deref(), Some("JPY"));
        assert_eq!(r[0].fx_as_of.as_deref(), Some("2026-11-10"));
        assert!(!r[0].fx_stale);
        assert_eq!(r[0].per_hour_minor, Some(4000));
        // An old rate is used with its date and flagged.
        let old = FxRate {
            usd_per_jpy: 0.01,
            as_of: "2026-10-01".into(),
        };
        let r = per_hour_in(&d, "2026-11-01", "2026-11-30", Some(&old), today).unwrap();
        assert!(r[0].fx_stale);
        assert_eq!(r[0].fx_as_of.as_deref(), Some("2026-10-01"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn fx_status_reads_only_the_ledger_rate_and_flags_stale() {
        let d = temp_dir("fxstatus");
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        // No ledger: no rate, not stale.
        assert_eq!(
            fx_status_in(&d, today).unwrap(),
            FxStatus {
                rate: None,
                stale: false
            }
        );
        write_ledger(&d, None, &["v1"]);
        assert_eq!(fx_status_in(&d, today).unwrap().rate, None);
        write_ledger(&d, Some(("2026-11-08", 0.0066)), &["v1"]);
        let s = fx_status_in(&d, today).unwrap();
        assert_eq!(s.rate.as_ref().unwrap().as_of, "2026-11-08");
        assert!(!s.stale); // exactly 7 days is not older than 7
        write_ledger(&d, Some(("2026-11-07", 0.0066)), &["v1"]);
        assert!(fx_status_in(&d, today).unwrap().stale);
        write_ledger(&d, Some(("2026-11-20", 0.0066)), &["v1"]);
        assert!(!fx_status_in(&d, today).unwrap().stale);
        // A malformed ledger is an error, not an invented "no rate".
        fs::write(d.join("business").join("ledger.json"), "{bad").unwrap();
        assert_eq!(fx_status_in(&d, today).unwrap_err(), "ledger_invalid");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn streams_need_a_ledger_venture_and_list_flags_orphans() {
        let d = temp_dir("venture");
        // Ledger missing: refuse, and no database is created.
        assert_eq!(
            stream_add_in(&d, "v1", "Shop", "product_sales", None, None).unwrap_err(),
            "ledger_missing"
        );
        assert!(!db_path(&d).exists());
        write_ledger(&d, None, &["v1", "v2"]);
        assert_eq!(
            stream_add_in(&d, "nope", "Shop", "product_sales", None, None).unwrap_err(),
            "unknown_venture"
        );
        assert_eq!(
            stream_add_in(&d, "v1", "Shop", "bogus", None, None).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            stream_add_in(&d, "v1", "  ", "other", None, None).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            stream_add_in(&d, "v1", "Shop", "other", Some("2026-13-01"), None).unwrap_err(),
            "invalid_input"
        );
        let a = stream_add_in(
            &d,
            "v1",
            "Shop",
            "product_sales",
            Some("2026-01-02"),
            Some("n"),
        )
        .unwrap();
        let b = stream_add_in(&d, "v2", "Tips", "affiliate", None, None).unwrap();
        assert!(!a.venture_missing);
        assert_eq!(
            (a.status.as_str(), a.notes.as_deref()),
            ("active", Some("n"))
        );
        // v2 leaves the ledger: its stream is flagged, nothing is deleted.
        write_ledger(&d, None, &["v1"]);
        let list = streams_list_in(&d).unwrap();
        assert_eq!(list.len(), 2);
        assert!(!list.iter().find(|s| s.id == a.id).unwrap().venture_missing);
        assert!(list.iter().find(|s| s.id == b.id).unwrap().venture_missing);
        // No ledger at all: every stream shows venture_missing and the list still works.
        fs::remove_file(d.join("business").join("ledger.json")).unwrap();
        assert!(streams_list_in(&d)
            .unwrap()
            .iter()
            .all(|s| s.venture_missing));
        // Update.
        let u = stream_update_in(&d, &a.id, Some("paused"), Some("Shop 2"), Some("")).unwrap();
        assert_eq!(
            (u.status.as_str(), u.name.as_str(), u.notes),
            ("paused", "Shop 2", None)
        );
        assert_eq!(
            stream_update_in(&d, &a.id, Some("bogus"), None, None).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            stream_update_in(&d, "nope", None, None, None).unwrap_err(),
            "unknown_stream"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn attention_is_validated_and_listed_by_range() {
        let (d, sid) = setup("attn");
        assert_eq!(
            attention_set_in(&d, &sid, "2026-11-0", 5).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            attention_set_in(&d, &sid, "2026-11-01", -1).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            attention_set_in(&d, &sid, "2026-11-01", 1441).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            attention_set_in(&d, "nope", "2026-11-01", 5).unwrap_err(),
            "unknown_stream"
        );
        attention_set_in(&d, &sid, "2026-11-01", 30).unwrap();
        attention_set_in(&d, &sid, "2026-11-05", 45).unwrap();
        let all = attention_list_in(&d, None, "2026-11-01", "2026-11-30").unwrap();
        assert_eq!(all.iter().map(|r| r.minutes).collect::<Vec<_>>(), [30, 45]);
        let some = attention_list_in(&d, Some(&sid), "2026-11-02", "2026-11-30").unwrap();
        assert_eq!(some.len(), 1);
        assert!(attention_list_in(&d, None, "2026-11-30", "2026-11-01").is_err());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn month_ranges_are_bounded() {
        assert!(month_list("2026-01", "2026-12").is_ok());
        assert!(month_list("2026-12", "2026-01").is_err());
        assert!(month_list("2026-1", "2026-12").is_err());
        assert!(month_list("2020-01", "2026-12").is_err());
    }

    #[test]
    fn the_database_is_private_and_a_symlinked_business_folder_is_refused() {
        let (d, _) = setup("perm");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = fs::metadata(db_path(&d)).unwrap().permissions().mode() & 0o777;
            assert_eq!(m, 0o600);
            let other = temp_dir("perm-target");
            let d2 = temp_dir("perm2");
            std::os::unix::fs::symlink(&other, d2.join("business")).unwrap();
            assert_eq!(open_db(&d2).unwrap_err(), "unsafe_storage");
            let _ = fs::remove_dir_all(d2);
            let _ = fs::remove_dir_all(other);
        }
        let _ = fs::remove_dir_all(d);
    }

    // ── source scans ──

    fn production(src: &str) -> String {
        let marker = concat!("#[cfg(test)]", "\nmod tests {");
        src.split(marker).next().unwrap().to_string()
    }

    fn own_source() -> String {
        production(include_str!("treasury.rs"))
    }

    pub(super) const BANNED: [&str; 17] = [
        "transfer",
        "pay",
        "trade",
        "convert",
        "withdraw",
        "send",
        "buy",
        "sell",
        "move",
        "wire",
        "remit",
        "swap",
        "sign",
        "broadcast",
        "order",
        "deposit",
        "exchange",
    ];

    #[test]
    fn every_command_is_named_treasury_and_none_is_a_money_mover() {
        let src = own_source();
        let mut names = Vec::new();
        let mut lines = src.lines();
        while let Some(l) = lines.next() {
            if l.trim() == "#[tauri::command]" {
                let f = lines.find(|l| l.contains("fn ")).unwrap();
                let name = f
                    .split("fn ")
                    .nth(1)
                    .and_then(|r| r.split('(').next())
                    .unwrap()
                    .trim();
                names.push(name.to_string());
            }
        }
        assert_eq!(names.len(), 11, "{names:?}");
        for n in &names {
            assert!(n.starts_with("treasury_"), "{n}");
            for b in BANNED {
                assert!(!n.contains(b), "command {n} contains {b}");
            }
        }
    }

    #[test]
    fn treasury_has_no_network_client_and_no_url() {
        let src = own_source();
        for needle in [
            "reqwest",
            "shared::http",
            "TcpStream",
            "UdpSocket",
            "hyper",
            concat!("http", "://"),
            concat!("https", "://"),
            "Command::new",
        ] {
            assert!(!src.contains(needle), "production source contains {needle}");
        }
        assert!(src.contains(".set_parent("));
    }

    #[test]
    fn treasury_never_reads_the_personal_store_or_its_rate_file() {
        let src = own_source();
        for needle in [
            concat!("money", "::"),
            concat!("money", "_summary"),
            concat!("money", "_import"),
            concat!("money", "/"),
            concat!("fx", ".json"),
            "weekly_review",
        ] {
            assert!(!src.contains(needle), "production source contains {needle}");
        }
    }

    #[test]
    fn the_personal_money_code_never_reads_treasury() {
        for (name, src) in [
            ("money.rs", include_str!("money.rs")),
            ("money_summary.rs", include_str!("money_summary.rs")),
            ("money_import_pick.rs", include_str!("money_import_pick.rs")),
            ("weekly_review.rs", include_str!("weekly_review.rs")),
        ] {
            let p = production(src).to_lowercase();
            assert!(!p.contains("treasury"), "{name} mentions treasury");
        }
    }
}

/// Fixtures shared with the sibling `treasury_counts` and `treasury_sequences` test modules.
/// After the `tests` module on purpose: the source-scan tests cut the file at that module.
#[cfg(test)]
pub(super) mod test_support {
    use super::*;
    use std::fs;

    pub(crate) const BANNED: [&str; 17] = [
        "transfer",
        "pay",
        "trade",
        "convert",
        "withdraw",
        "send",
        "buy",
        "sell",
        "move",
        "wire",
        "remit",
        "swap",
        "sign",
        "broadcast",
        "order",
        "deposit",
        "exchange",
    ];

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("amara-treasury-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    pub(crate) fn write_ledger(dir: &Path, ventures: &[&str]) {
        let biz = dir.join("business");
        fs::create_dir_all(&biz).unwrap();
        let v: Vec<String> = ventures
            .iter()
            .map(|id| {
                format!(
                    r#"{{"id":"{id}","name":"{id}","stage":"idea","health":"on_track","note":""}}"#
                )
            })
            .collect();
        fs::write(
            biz.join("ledger.json"),
            format!(r#"{{"version":1,"fx":null,"ventures":[{}]}}"#, v.join(",")),
        )
        .unwrap();
    }

    pub(crate) fn setup(tag: &str) -> (PathBuf, String) {
        let d = temp_dir(tag);
        write_ledger(&d, &["v1"]);
        let s = stream_add_in(&d, "v1", "Shop", "product_sales", None, None).unwrap();
        (d, s.id)
    }

    pub(crate) fn csv_file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        p
    }
}
