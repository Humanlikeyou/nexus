//! Treasury, phase 5 (part 2): the monthly review (plan `tasks/plans/treasury.md`, Phase 5).
//!
//! The LOCAL DEEP tier (`llm_router::local_deep_complete`: Claude forced off, a Claude-tier reply
//! refused) writes one page about one complete past month. Rust builds the figures from the ledger
//! rows (`income_events`), hands the model a facts block, and then checks the page before anyone sees it.
//!
//! Rules this file keeps (each has a test):
//! - Every number in the page is parsed out and must match a figure Rust computed from the ledger rows
//!   for that month (a total, the previous month, the trailing average, a change, an event count) or a
//!   month label. Any other number, a numeral of another script, or a spelled-out number hides the
//!   review (`state: "hidden"`, no text). The same check runs again on every read, against the
//!   ledger as it is now, so a later import that changes a figure also hides a stale page.
//! - A month with no events stays "no data" (`None`), never 0; a month with no data at all produces no
//!   review and no model call (`state: "no_data"`). Zero is not an allowed number unless a real figure is 0.
//! - Streams are shown to the model as "Stream A", "Stream B", never by name; the names are put back
//!   only after the page passed. Contacts never exist here (decision 0032 #2).
//! - The validator checks numbers, not wording: it cannot tell "up" from "down". The facts block says
//!   which, and the figures table (`rows`) is what the UI shows as the numbers of record.
//!
//! Outside calls from this file: none beyond the local router client used by `local_deep_complete`.

use super::shared::task::{blocking, JoinErr};
use super::shared::time::{day_key, now_jst};
use super::treasury::{is_month, monthly_totals, now_iso, open_db, store_err};
use chrono::{Months, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

const MAX_STREAMS: usize = 26;
const MAX_TEXT_CHARS: usize = 3_000;

const INSTRUCTION: &str = "Write a one-page monthly income review for the owner from the facts below. Use plain prose in at most six short paragraphs. Use ONLY numbers that appear in the facts, written exactly as shown. Never calculate, round, add or compare numbers yourself. Never write a number word, the word percent, or a numbered list. Where the facts say no data, say there is no data for that month and never write zero. Refer to streams only as Stream A, Stream B and so on. Say whether income went up or down only where the facts say up or down. No links, markup or tables.";

// ───────────────────────────── types ─────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryReviewRow", rename_all = "camelCase")]
pub struct ReviewRow {
    pub stream_id: String,
    pub stream_name: String,
    pub currency: String,
    /// `None` = no events that month (never 0). Same for the two below.
    pub month_total_minor: Option<i64>,
    pub prev_total_minor: Option<i64>,
    /// Mean of the earlier three months that have events.
    pub avg_minor: Option<i64>,
    pub events: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryReview", rename_all = "camelCase")]
pub struct Review {
    pub month: String,
    /// `ok` (text present) | `hidden` (the page failed the number check; no text) | `no_data`.
    pub state: String,
    pub text: Option<String>,
    /// Why a review is hidden: `mismatch | unavailable | too_many_streams | ledger_changed`.
    pub reason: Option<String>,
    pub built_at: Option<String>,
    /// The figures of record, from the ledger rows. Present for every state.
    pub rows: Vec<ReviewRow>,
}

// ───────────────────────────── figures ─────────────────────────────

fn month_back(month: &str, back: u32) -> Result<String, String> {
    let first = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| "invalid_input".to_string())?;
    Ok(first
        .checked_sub_months(Months::new(back))
        .ok_or_else(|| "invalid_input".to_string())?
        .format("%Y-%m")
        .to_string())
}

/// A complete past month: well-formed and strictly before the current JST month.
fn check_month(month: &str, today: NaiveDate) -> Result<(), String> {
    if !is_month(month) || month >= today.format("%Y-%m").to_string().as_str() {
        return Err("invalid_input".into());
    }
    Ok(())
}

fn rows_in(conn: &Connection, month: &str) -> Result<Vec<ReviewRow>, String> {
    let (m3, m2, m1) = (
        month_back(month, 3)?,
        month_back(month, 2)?,
        month_back(month, 1)?,
    );
    let totals = monthly_totals(conn, &m3, month, None)?;
    let mut stmt = conn
        .prepare("SELECT id, name FROM streams ORDER BY created_at, id")
        .map_err(store_err)?;
    let streams: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(store_err)?
        .collect::<Result<_, _>>()
        .map_err(store_err)?;
    let mut out = Vec::new();
    for (sid, name) in streams {
        for cur in ["JPY", "USD"] {
            let get = |m: &str| {
                totals
                    .get(&(sid.clone(), cur.to_string(), m.to_string()))
                    .copied()
            };
            let earlier = [get(&m3), get(&m2), get(&m1)];
            let (this, prev) = (get(month), get(&m1));
            if this.is_none() && prev.is_none() && earlier.iter().all(Option::is_none) {
                continue;
            }
            let events: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM income_events WHERE stream_id = ?1 AND currency = ?2 AND substr(date, 1, 7) = ?3",
                    params![sid, cur, month],
                    |r| r.get(0),
                )
                .map_err(store_err)?;
            out.push(ReviewRow {
                stream_id: sid.clone(),
                stream_name: name.clone(),
                currency: cur.to_string(),
                month_total_minor: this,
                prev_total_minor: prev,
                avg_minor: super::treasury::mean_minor(&earlier),
                events,
            });
        }
    }
    Ok(out)
}

/// Display form of an amount: yen as a whole number, dollars with cents, thousands grouped.
fn fmt_amount(minor: i64, currency: &str) -> String {
    let abs = minor.unsigned_abs();
    let group = |n: u64| {
        let s = n.to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    };
    let body = if currency == "USD" {
        format!("{}.{:02}", group(abs / 100), abs % 100)
    } else {
        group(abs)
    };
    if minor < 0 {
        format!("-{body}")
    } else {
        body
    }
}

fn label_of(i: usize) -> String {
    format!("Stream {}", (b'A' + i as u8) as char)
}

fn labels(rows: &[ReviewRow]) -> Vec<(String, String)> {
    // One label per distinct stream, in row order.
    let mut seen: Vec<String> = Vec::new();
    for r in rows {
        if !seen.contains(&r.stream_id) {
            seen.push(r.stream_id.clone());
        }
    }
    seen.iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), label_of(i)))
        .collect()
}

fn label_for<'a>(labels: &'a [(String, String)], id: &str) -> &'a str {
    labels
        .iter()
        .find(|(i, _)| i == id)
        .map(|(_, l)| l.as_str())
        .unwrap_or("Stream")
}

/// Change from the previous month in words and an absolute figure, when both months have data.
fn change(r: &ReviewRow) -> Option<(&'static str, i64)> {
    let (a, b) = (r.month_total_minor?, r.prev_total_minor?);
    let d = a.checked_sub(b)?;
    Some((
        match d.cmp(&0) {
            std::cmp::Ordering::Greater => "up",
            std::cmp::Ordering::Less => "down",
            std::cmp::Ordering::Equal => "unchanged",
        },
        d.checked_abs()?,
    ))
}

fn facts_block(month: &str, rows: &[ReviewRow]) -> Result<String, String> {
    let prev = month_back(month, 1)?;
    let lab = labels(rows);
    let or_none =
        |v: Option<i64>, cur: &str| v.map_or("no data".to_string(), |m| fmt_amount(m, cur));
    let mut s = format!("Review month: {month}. Previous month: {prev}.\n");
    for r in rows {
        let c = &r.currency;
        s.push_str(&format!(
            "{} ({c}): {month} total {}; {prev} total {}; average of the three earlier months {}; events in {month}: {}.",
            label_for(&lab, &r.stream_id),
            or_none(r.month_total_minor, c),
            or_none(r.prev_total_minor, c),
            or_none(r.avg_minor, c),
            r.events,
        ));
        match change(r) {
            Some((word, by)) => s.push_str(&format!(
                " Change from {prev}: {word} by {}.\n",
                fmt_amount(by, c)
            )),
            None => s.push_str(" Change from the previous month: no data.\n"),
        }
    }
    Ok(s)
}

// ───────────────────────────── the number check ─────────────────────────────

/// A numeral in comparable form: thousands separators and leading zeros dropped, trailing decimal zeros
/// dropped. `raw` is already a well-formed token from `numerals`.
fn canon(raw: &str) -> String {
    let cleaned: String = raw.chars().filter(|c| *c != ',').collect();
    let (int, frac) = match cleaned.split_once('.') {
        Some((i, f)) => (i.to_string(), f.trim_end_matches('0').to_string()),
        None => (cleaned, String::new()),
    };
    let int = int.trim_start_matches('0');
    let int = if int.is_empty() { "0" } else { int };
    if frac.is_empty() {
        int.to_string()
    } else {
        format!("{int}.{frac}")
    }
}

/// Every numeral in `text` as (canonical value, index just past it). The shape is
/// `\d{1,3}(,\d{3})*(\.\d+)?` or a plain digit run: a comma counts as a thousands separator only before
/// exactly three digits after a group of at most three, so "1,2,3,4" is four numbers, not 1234.
fn numerals(text: &str) -> Vec<(String, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let digit = |k: usize| chars.get(k).is_some_and(char::is_ascii_digit);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut j = i;
        while digit(j) {
            j += 1;
        }
        if j - i <= 3 {
            while chars.get(j) == Some(&',')
                && digit(j + 1)
                && digit(j + 2)
                && digit(j + 3)
                && !digit(j + 4)
            {
                j += 4;
            }
        }
        if chars.get(j) == Some(&'.') && digit(j + 1) {
            j += 1;
            while digit(j) {
                j += 1;
            }
        }
        out.push((canon(&chars[i..j].iter().collect::<String>()), j));
        i = j;
    }
    out
}

/// The figures the page may use as amounts, and the event counts it may use only before "event(s)".
struct Allowed {
    amounts: BTreeSet<String>,
    events: BTreeSet<String>,
    labels: Vec<String>,
}

/// Built from the rows (never from the model's text). Month and year numbers are not in the sets: a
/// month may appear only as a whole `YYYY-MM` label.
fn allowed(month: &str, rows: &[ReviewRow]) -> Result<Allowed, String> {
    let mut amounts = BTreeSet::new();
    let mut events = BTreeSet::new();
    let add = |set: &mut BTreeSet<String>, s: &str| {
        for (n, _) in numerals(s) {
            set.insert(n);
        }
    };
    for r in rows {
        let c = &r.currency;
        for v in [r.month_total_minor, r.prev_total_minor, r.avg_minor]
            .into_iter()
            .flatten()
        {
            add(
                &mut amounts,
                &fmt_amount(v.checked_abs().ok_or("invalid_input")?, c),
            );
        }
        if let Some((_, by)) = change(r) {
            add(&mut amounts, &fmt_amount(by, c));
        }
        add(&mut events, &r.events.to_string());
    }
    let mut labels = Vec::new();
    for back in 0..=3 {
        labels.push(month_back(month, back)?);
    }
    Ok(Allowed {
        amounts,
        events,
        labels,
    })
}

/// Replace each whole `YYYY-MM` token that is one of `labels` with a space (a label glued to other
/// digits is left alone and then fails as a number).
fn strip_labels(text: &str, labels: &[String]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if i + 7 <= chars.len() {
            let tok: String = chars[i..i + 7].iter().collect();
            let before = i > 0 && (chars[i - 1].is_ascii_digit() || chars[i - 1] == '-');
            let after = chars.get(i + 7).is_some_and(|c| c.is_ascii_digit());
            if !before && !after && labels.contains(&tok) {
                out.push(' ');
                i += 7;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Common spelled-out numbers in other languages (the English ones are `number_like`).
const FOREIGN_NUMBER_WORDS: &[&str] = &[
    "cero",
    "uno",
    "dos",
    "tres",
    "cuatro",
    "cinco",
    "seis",
    "siete",
    "ocho",
    "nueve",
    "diez",
    "once",
    "doce",
    "trece",
    "catorce",
    "quince",
    "veinte",
    "treinta",
    "cuarenta",
    "cincuenta",
    "sesenta",
    "setenta",
    "ochenta",
    "noventa",
    "cien",
    "ciento",
    "mil",
    "millon",
    "deux",
    "trois",
    "quatre",
    "cinq",
    "sept",
    "huit",
    "neuf",
    "dix",
    "onze",
    "douze",
    "treize",
    "quinze",
    "seize",
    "vingt",
    "trente",
    "quarante",
    "cinquante",
    "soixante",
    "mille",
    "eins",
    "zwei",
    "drei",
    "vier",
    "funf",
    "sechs",
    "sieben",
    "acht",
    "neun",
    "zehn",
    "zwanzig",
    "dreissig",
    "hundert",
    "tausend",
    "due",
    "quattro",
    "cinque",
    "sette",
    "otto",
    "nove",
    "dieci",
    "venti",
    "trenta",
    "cento",
    "dois",
    "quatro",
    "sete",
    "oito",
    "dez",
    "vinte",
    "trinta",
    "cem",
];

/// Accept the page or say why not. `Ok` only when every number matches the rows. Only printable ASCII,
/// newline and a short list of typographic marks pass, so zero-width and bidi characters, other-script
/// numerals and look-alikes never reach the number check.
pub(crate) fn validate_review(
    text: &str,
    month: &str,
    rows: &[ReviewRow],
) -> Result<(), &'static str> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_TEXT_CHARS {
        return Err("length");
    }
    if t.chars()
        .any(|c| !(matches!(c, ' '..='~' | '\n' | '\u{2019}' | '\u{2013}' | '\u{2014}')))
    {
        return Err("characters");
    }
    let lower = t.to_lowercase();
    if ["http", "www.", "://", "@", "<", ">"]
        .iter()
        .any(|b| lower.contains(b))
    {
        return Err("link");
    }
    let words = t.split(|c: char| !c.is_ascii_alphabetic());
    if words.filter(|w| !w.is_empty()).any(|w| {
        let w = w.to_ascii_lowercase();
        super::notify_relay::number_like(&w) || FOREIGN_NUMBER_WORDS.contains(&w.as_str())
    }) {
        return Err("number word");
    }
    let ok = allowed(month, rows).map_err(|_| "unavailable")?;
    let rest = strip_labels(t, &ok.labels);
    let chars: Vec<char> = rest.chars().collect();
    for (n, end) in numerals(&rest) {
        if ok.amounts.contains(&n) {
            continue;
        }
        // An event count counts only when "event" or "events" follows it.
        let next: String = chars[end.min(chars.len())..]
            .iter()
            .skip_while(|c| **c == ' ')
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_lowercase();
        if ok.events.contains(&n) && (next == "event" || next == "events") {
            continue;
        }
        return Err("mismatch");
    }
    Ok(())
}

/// Put the stream names back for display (after the page passed). "Stream A" only, never inside a longer
/// word or a second letter.
fn show_names(text: &str, rows: &[ReviewRow]) -> String {
    let lab = labels(rows);
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    'outer: while i < chars.len() {
        for (id, label) in &lab {
            let l: Vec<char> = label.chars().collect();
            if chars[i..].starts_with(&l)
                && chars.get(i + l.len()).is_none_or(|c| !c.is_alphanumeric())
                && (i == 0 || !chars[i - 1].is_alphanumeric())
            {
                if let Some(r) = rows.iter().find(|r| &r.stream_id == id) {
                    out.push_str(&r.stream_name);
                    i += l.len();
                    continue 'outer;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

// ───────────────────────────── build and read ─────────────────────────────

pub(crate) trait ReviewWriter {
    async fn write(&self, instruction: &str, facts: &str) -> Result<String, String>;
}

struct DeepWriter;
impl ReviewWriter for DeepWriter {
    async fn write(&self, instruction: &str, facts: &str) -> Result<String, String> {
        super::llm_router::local_deep_complete(instruction, facts).await
    }
}

fn review(
    month: &str,
    state: &str,
    text: Option<String>,
    reason: Option<&str>,
    built_at: Option<String>,
    rows: Vec<ReviewRow>,
) -> Review {
    Review {
        month: month.into(),
        state: state.into(),
        text,
        reason: reason.map(str::to_string),
        built_at,
        rows,
    }
}

/// A rebuild that did not produce a valid page also takes the previous stored page off the screen.
async fn hide_stored(data: &Path, month: &str) -> Result<(), String> {
    let (d, m) = (data.to_path_buf(), month.to_string());
    blocking(JoinErr::Code("store_error"), move || {
        open_db(&d)?
            .execute("UPDATE reviews SET hidden = 1 WHERE month = ?1", [m])
            .map_err(store_err)?;
        Ok(())
    })
    .await
}

pub(crate) async fn build_core<W: ReviewWriter>(
    data: &Path,
    month: &str,
    today: NaiveDate,
    w: &W,
) -> Result<Review, String> {
    check_month(month, today)?;
    let (d, m) = (data.to_path_buf(), month.to_string());
    let rows = blocking(JoinErr::Code("store_error"), move || {
        rows_in(&open_db(&d)?, &m)
    })
    .await?;
    if rows.iter().all(|r| r.month_total_minor.is_none()) {
        hide_stored(data, month).await?;
        return Ok(review(month, "no_data", None, None, None, rows));
    }
    if labels(&rows).len() > MAX_STREAMS {
        hide_stored(data, month).await?;
        return Ok(review(
            month,
            "hidden",
            None,
            Some("too_many_streams"),
            None,
            rows,
        ));
    }
    let facts = facts_block(month, &rows)?;
    let raw = match w.write(INSTRUCTION, &facts).await {
        Ok(t) => t,
        Err(_) => {
            hide_stored(data, month).await?;
            return Ok(review(
                month,
                "hidden",
                None,
                Some("unavailable"),
                None,
                rows,
            ));
        }
    };
    if let Err(why) = validate_review(&raw, month, &rows) {
        let reason = if why == "unavailable" {
            "unavailable"
        } else {
            "mismatch"
        };
        hide_stored(data, month).await?;
        return Ok(review(month, "hidden", None, Some(reason), None, rows));
    }
    let built_at = now_iso();
    let (d, m, t, b) = (
        data.to_path_buf(),
        month.to_string(),
        raw.trim().to_string(),
        built_at.clone(),
    );
    blocking(JoinErr::Code("store_error"), move || {
        open_db(&d)?
            .execute(
                "INSERT INTO reviews(month, built_at, text, hidden) VALUES(?1, ?2, ?3, 0)
                 ON CONFLICT(month) DO UPDATE SET built_at = excluded.built_at, text = excluded.text, hidden = 0",
                params![m, b, t],
            )
            .map_err(store_err)?;
        Ok(())
    })
    .await?;
    Ok(review(
        month,
        "ok",
        Some(show_names(raw.trim(), &rows)),
        None,
        Some(built_at),
        rows,
    ))
}

/// The stored page, checked again against the ledger as it is now. `None` when never built.
pub(crate) fn read_in(
    data: &Path,
    month: &str,
    today: NaiveDate,
) -> Result<Option<Review>, String> {
    check_month(month, today)?;
    let conn = open_db(data)?;
    let stored: Option<(String, String, i64)> = conn
        .query_row(
            "SELECT built_at, text, hidden FROM reviews WHERE month = ?1",
            [month],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(store_err)?;
    let Some((built_at, text, hidden)) = stored else {
        return Ok(None);
    };
    let rows = rows_in(&conn, month)?;
    if hidden != 0 {
        return Ok(Some(review(
            month,
            "hidden",
            None,
            Some("rebuild_failed"),
            Some(built_at),
            rows,
        )));
    }
    Ok(Some(match validate_review(&text, month, &rows) {
        Ok(()) => review(
            month,
            "ok",
            Some(show_names(&text, &rows)),
            None,
            Some(built_at),
            rows,
        ),
        Err(_) => review(
            month,
            "hidden",
            None,
            Some("ledger_changed"),
            Some(built_at),
            rows,
        ),
    }))
}

// ───────────────────────────── commands ─────────────────────────────

static BUILDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One review build at a time (a build is a long local-model call). Dropping it frees the slot.
struct BuildSlot;
impl BuildSlot {
    fn take() -> Option<BuildSlot> {
        BUILDING
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .ok()
            .map(|_| BuildSlot)
    }
}
impl Drop for BuildSlot {
    fn drop(&mut self) {
        BUILDING.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_review_read(month: String) -> Result<Option<Review>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        read_in(&crate::app_paths::data_dir(), &month, day_key(&now_jst()))
    })
    .await
}

/// Builds (and stores) the review with the local deep tier. A page that fails the number check is
/// returned hidden and not stored.
#[tauri::command]
#[specta::specta]
pub async fn treasury_review_build(month: String) -> Result<Review, String> {
    let _slot = BuildSlot::take().ok_or_else(|| "busy".to_string())?;
    build_core(
        &crate::app_paths::data_dir(),
        &month,
        day_key(&now_jst()),
        &DeepWriter,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::treasury::stream_add_in;
    use crate::commands::treasury::test_support::{temp_dir, write_ledger, BANNED};
    use std::fs;
    use std::path::PathBuf;

    const TODAY: (i32, u32, u32) = (2026, 11, 18);
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(TODAY.0, TODAY.1, TODAY.2).unwrap()
    }

    fn setup(tag: &str) -> (PathBuf, String) {
        let d = temp_dir(tag);
        write_ledger(&d, &["v1"]);
        let s = stream_add_in(&d, "v1", "Shop 2", "product_sales", None, None).unwrap();
        (d, s.id)
    }

    fn income(d: &Path, sid: &str, day: &str, minor: i64, cur: &str) {
        open_db(d)
            .unwrap()
            .execute(
                "INSERT INTO income_events(id, stream_id, date, amount_minor, currency, source, raw_ref, dedupe_key, import_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, 'generic', 'row:1', ?6, NULL)",
                params![uuid::Uuid::new_v4().to_string(), sid, day, minor, cur, uuid::Uuid::new_v4().to_string()],
            )
            .unwrap();
    }

    /// October 123,456 yen in two events; September 100,000; August 90,000.
    fn fill(d: &Path, sid: &str) {
        income(d, sid, "2026-10-03", 100_000, "JPY");
        income(d, sid, "2026-10-20", 23_456, "JPY");
        income(d, sid, "2026-09-10", 100_000, "JPY");
        income(d, sid, "2026-08-10", 90_000, "JPY");
    }

    fn rows(d: &Path) -> Vec<ReviewRow> {
        rows_in(&open_db(d).unwrap(), "2026-10").unwrap()
    }

    struct W(Result<String, String>);
    impl ReviewWriter for W {
        async fn write(&self, _: &str, facts: &str) -> Result<String, String> {
            assert!(!facts.contains("Shop"), "names never reach the model");
            self.0.clone()
        }
    }

    const GOOD: &str = "Stream A earned 123,456 JPY in 2026-10, up from 100,000 JPY in 2026-09. The average of the earlier months was 95,000 JPY, and there were 2 events.";

    #[test]
    fn figures_come_from_the_ledger_rows_and_a_missing_month_is_none_not_zero() {
        let (d, sid) = setup("rows");
        fill(&d, &sid);
        let r = rows(&d);
        assert_eq!(r.len(), 1);
        assert_eq!(
            (
                r[0].month_total_minor,
                r[0].prev_total_minor,
                r[0].avg_minor,
                r[0].events
            ),
            (Some(123_456), Some(100_000), Some(95_000), 2)
        );
        // A month with nothing: None everywhere, and the earlier-months mean ignores it.
        let r = rows_in(&open_db(&d).unwrap(), "2026-11").unwrap_or_default();
        assert!(r.is_empty() || r[0].month_total_minor.is_none());
        let r = rows_in(&open_db(&d).unwrap(), "2026-09").unwrap();
        assert_eq!(
            (r[0].month_total_minor, r[0].avg_minor),
            (Some(100_000), Some(90_000))
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn the_facts_block_says_no_data_and_uses_labels() {
        let (d, sid) = setup("facts");
        income(&d, &sid, "2026-10-03", 5_000, "JPY");
        let f = facts_block("2026-10", &rows(&d)).unwrap();
        assert!(f.contains("Stream A (JPY): 2026-10 total 5,000; 2026-09 total no data; average of the three earlier months no data"));
        assert!(f.contains("Change from the previous month: no data."));
        assert!(!f.contains("Shop") && !f.contains(" 0;"));
        assert_eq!(fmt_amount(1250, "USD"), "12.50");
        assert_eq!(fmt_amount(-1_234_567, "JPY"), "-1,234,567");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn the_validator_accepts_a_page_whose_numbers_all_match_the_rows() {
        let (d, sid) = setup("valid");
        fill(&d, &sid);
        assert_eq!(validate_review(GOOD, "2026-10", &rows(&d)), Ok(()));
        // Formatting differences are fine (separators, a trailing .00).
        assert_eq!(
            validate_review(
                "Stream A: 123456 JPY, previous 100,000.00.",
                "2026-10",
                &rows(&d)
            ),
            Ok(())
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn the_validator_rejects_a_wrong_or_invented_number() {
        let (d, sid) = setup("tamper");
        fill(&d, &sid);
        let r = rows(&d);
        // A tampered total.
        assert_eq!(
            validate_review(&GOOD.replace("123,456", "123,465"), "2026-10", &r),
            Err("mismatch")
        );
        // An invented number (a made-up growth rate, a count, a zero).
        for bad in [
            format!("{GOOD} Growth was 23%."),
            format!("{GOOD} That is 3 sales."),
            format!("{GOOD} September had 0 income."),
        ] {
            assert_eq!(
                validate_review(&bad, "2026-10", &r),
                Err("mismatch"),
                "{bad}"
            );
        }
        // Other scripts and spelled-out numbers.
        assert_eq!(
            validate_review("Stream A made \u{0661}\u{0662}.", "2026-10", &r),
            Err("characters")
        );
        assert_eq!(
            validate_review("Stream A made twelve sales.", "2026-10", &r),
            Err("number word")
        );
        assert_eq!(
            validate_review("Stream A up 5 percent.", "2026-10", &r),
            Err("number word")
        );
        assert_eq!(validate_review("see http://x", "2026-10", &r), Err("link"));
        assert_eq!(validate_review("   ", "2026-10", &r), Err("length"));
        let _ = fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn a_good_page_is_stored_and_shown_with_the_stream_names_back() {
        let (d, sid) = setup("build-ok");
        fill(&d, &sid);
        let r = build_core(&d, "2026-10", today(), &W(Ok(GOOD.into())))
            .await
            .unwrap();
        assert_eq!(r.state, "ok");
        assert!(r
            .text
            .as_deref()
            .unwrap()
            .starts_with("Shop 2 earned 123,456 JPY"));
        assert_eq!(r.rows.len(), 1);
        let read = read_in(&d, "2026-10", today()).unwrap().unwrap();
        assert_eq!((read.state.as_str(), read.text), ("ok", r.text));
        assert!(
            read_in(&d, "2026-09", today()).unwrap().is_none(),
            "never built"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn a_tampered_or_invented_number_hides_the_review_and_stores_nothing() {
        let (d, sid) = setup("build-bad");
        fill(&d, &sid);
        for page in [
            GOOD.replace("123,456", "123,465"),
            format!("{GOOD} Revenue grew 40%."),
        ] {
            let r = build_core(&d, "2026-10", today(), &W(Ok(page)))
                .await
                .unwrap();
            assert_eq!(
                (r.state.as_str(), r.reason.as_deref()),
                ("hidden", Some("mismatch"))
            );
            assert!(r.text.is_none(), "no text of a failed page is returned");
            assert_eq!(r.rows.len(), 1, "the figures of record still show");
        }
        assert!(read_in(&d, "2026-10", today()).unwrap().is_none());
        // A model that is down hides it too, with its own reason.
        let r = build_core(&d, "2026-10", today(), &W(Err("down".into())))
            .await
            .unwrap();
        assert_eq!(r.reason.as_deref(), Some("unavailable"));
        let _ = fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn a_month_with_no_events_is_no_data_and_the_model_is_not_called() {
        let (d, sid) = setup("nodata");
        income(&d, &sid, "2026-08-10", 90_000, "JPY");
        struct Never;
        impl ReviewWriter for Never {
            async fn write(&self, _: &str, _: &str) -> Result<String, String> {
                panic!("the model must not be called");
            }
        }
        let r = build_core(&d, "2026-10", today(), &Never).await.unwrap();
        assert_eq!((r.state.as_str(), r.text), ("no_data", None));
        assert_eq!(r.rows[0].month_total_minor, None);
        let none = temp_dir("nodata-empty");
        let r = build_core(&none, "2026-10", today(), &Never).await.unwrap();
        assert_eq!((r.state.as_str(), r.rows.len()), ("no_data", 0));
        let _ = fs::remove_dir_all(d);
        let _ = fs::remove_dir_all(none);
    }

    #[tokio::test]
    async fn a_stored_page_is_hidden_when_a_later_import_changes_a_figure() {
        let (d, sid) = setup("changed");
        fill(&d, &sid);
        build_core(&d, "2026-10", today(), &W(Ok(GOOD.into())))
            .await
            .unwrap();
        income(&d, &sid, "2026-10-25", 777, "JPY");
        let r = read_in(&d, "2026-10", today()).unwrap().unwrap();
        assert_eq!(
            (r.state.as_str(), r.reason.as_deref()),
            ("hidden", Some("ledger_changed"))
        );
        assert!(r.text.is_none());
        let _ = fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn only_a_complete_past_month_can_be_reviewed() {
        let (d, _) = setup("months");
        struct Never;
        impl ReviewWriter for Never {
            async fn write(&self, _: &str, _: &str) -> Result<String, String> {
                panic!()
            }
        }
        for bad in ["2026-11", "2027-01", "2026-13", "26-10", "", "2026-10-01"] {
            assert_eq!(
                build_core(&d, bad, today(), &Never).await.unwrap_err(),
                "invalid_input",
                "{bad}"
            );
            assert_eq!(read_in(&d, bad, today()).unwrap_err(), "invalid_input");
        }
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn names_are_put_back_only_for_whole_labels() {
        let rows = vec![ReviewRow {
            stream_id: "s1".into(),
            stream_name: "Shop".into(),
            currency: "JPY".into(),
            month_total_minor: Some(1),
            prev_total_minor: None,
            avg_minor: None,
            events: 1,
        }];
        assert_eq!(
            show_names("Stream A and Stream AB and Stream A.", &rows),
            "Shop and Stream AB and Shop."
        );
    }

    fn production() -> String {
        include_str!("treasury_review.rs")
            .split(concat!("#[cfg(test)]", "\nmod tests {"))
            .next()
            .unwrap()
            .to_string()
    }

    #[test]
    fn the_writer_is_the_local_deep_tier_only_and_claude_is_refused() {
        let src = production();
        assert!(src.contains("llm_router::local_deep_complete"));
        for banned in [
            "chat_with_router",
            "router_headers",
            "claude_gate",
            "reqwest",
            "anthropic",
        ] {
            assert!(!src.contains(banned), "{banned}");
        }
        let router = include_str!("llm_router.rs");
        let h = &router[router.find("fn local_deep_headers").unwrap()..];
        let h = &h[..h.find("\n}\n").unwrap()];
        assert!(h.contains("(\"X-Amara-Allow-Claude\", \"0\".to_string())"));
        assert!(h.contains("(\"X-Amara-Route\", \"deep\".to_string())"));
        assert!(!h.contains("\"1\""));
        assert!(crate::commands::llm_router::local_helper_is_local_only(
            "local_deep_complete",
            "Deep",
            "local_deep_headers"
        ));
        // The prompt carries labels and ledger figures only: no stream name, no contact data.
        assert!(!src.contains("stream_name)") || src.contains("show_names"));
        let facts_fn = &src[src.find("fn facts_block").unwrap()..];
        let facts_fn = &facts_fn[..facts_fn.find("// ────").unwrap()];
        assert!(!facts_fn.contains("stream_name") && !facts_fn.contains("email"));
    }

    #[test]
    fn commands_avoid_the_banned_names() {
        let src = production();
        let mut n = 0;
        let mut lines = src.lines();
        while let Some(l) = lines.next() {
            if l.trim() == "#[tauri::command]" {
                let f = lines.find(|l| l.contains("fn ")).unwrap();
                let name = f
                    .split("fn ")
                    .nth(1)
                    .unwrap()
                    .split('(')
                    .next()
                    .unwrap()
                    .trim()
                    .to_string();
                assert!(name.starts_with("treasury_"));
                for b in BANNED {
                    assert!(!name.contains(b), "{name} contains {b}");
                }
                n += 1;
            }
        }
        assert_eq!(n, 2);
    }

    #[test]
    fn hidden_characters_other_scripts_and_foreign_number_words_are_rejected() {
        let (d, sid) = setup("chars");
        fill(&d, &sid);
        let r = rows(&d);
        for bad in [
            "Stream A earned 12\u{200B}34 JPY.",
            "Stream A earned 123\u{202E}456 JPY.",
            "Stream A earned \u{4E09}\u{767E}.",
            "Stream A earned \u{FF11}\u{FF12} JPY.",
            "Stream A earned caf\u{00E9} JPY.",
        ] {
            assert_eq!(
                validate_review(bad, "2026-10", &r),
                Err("characters"),
                "{bad:?}"
            );
        }
        assert_eq!(
            validate_review("Stream A earned ciento JPY.", "2026-10", &r),
            Err("number word")
        );
        assert_eq!(
            validate_review("Stream A made veinte sales.", "2026-10", &r),
            Err("number word")
        );
        // The short typographic list is fine.
        assert_eq!(
            validate_review(
                "Stream A\u{2019}s month \u{2013} a calm month \u{2014} fine.",
                "2026-10",
                &r
            ),
            Ok(())
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn month_and_year_numbers_count_only_as_whole_labels_and_events_only_before_event() {
        let (d, sid) = setup("labels");
        fill(&d, &sid);
        let r = rows(&d);
        // Whole labels pass; the bare month or year, or a glued label, does not.
        assert_eq!(
            validate_review("Stream A in 2026-10 and 2026-07.", "2026-10", &r),
            Ok(())
        );
        for bad in [
            "Stream A had 10 sales.",
            "Stream A grew 11.",
            "Stream A in 2026.",
            "Stream A in 12026-10.",
            "Stream A in 2026-10-5.",
            "Stream A in 2026-11.",
        ] {
            assert_eq!(
                validate_review(bad, "2026-10", &r),
                Err("mismatch"),
                "{bad}"
            );
        }
        // The event count is allowed only when "event(s)" follows it.
        assert_eq!(
            validate_review("Stream A had 2 events.", "2026-10", &r),
            Ok(())
        );
        assert_eq!(
            validate_review("Stream A had 2 event.", "2026-10", &r),
            Ok(())
        );
        assert_eq!(
            validate_review("Stream A had 2 sales.", "2026-10", &r),
            Err("mismatch")
        );
        assert_eq!(
            validate_review("Stream A had 2.", "2026-10", &r),
            Err("mismatch")
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn only_three_digit_comma_groups_are_thousands_separators() {
        let nums = |t: &str| numerals(t).into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        assert_eq!(nums("1,2,3,4"), ["1", "2", "3", "4"]);
        assert_eq!(nums("1,234,567"), ["1234567"]);
        assert_eq!(nums("1234,567"), ["1234", "567"]);
        assert_eq!(nums("12,34"), ["12", "34"]);
        assert_eq!(nums("1,2345"), ["1", "2345"]);
        assert_eq!(nums("100,000.00 and 12.5"), ["100000", "12.5"]);
        let (d, sid) = setup("commas");
        income(&d, &sid, "2026-10-03", 1234, "JPY");
        let r = rows(&d);
        assert_eq!(
            validate_review("Stream A earned 1,234 JPY.", "2026-10", &r),
            Ok(())
        );
        assert_eq!(
            validate_review("Stream A earned 1,2,3,4 JPY.", "2026-10", &r),
            Err("mismatch")
        );
        let _ = fs::remove_dir_all(d);
    }

    #[tokio::test]
    async fn a_failed_or_hidden_rebuild_hides_the_stored_page_and_a_good_one_restores_it() {
        let (d, sid) = setup("rebuild-hides");
        fill(&d, &sid);
        build_core(&d, "2026-10", today(), &W(Ok(GOOD.into())))
            .await
            .unwrap();
        assert_eq!(
            read_in(&d, "2026-10", today()).unwrap().unwrap().state,
            "ok"
        );
        // The model is down: the old page is no longer shown.
        build_core(&d, "2026-10", today(), &W(Err("down".into())))
            .await
            .unwrap();
        let r = read_in(&d, "2026-10", today()).unwrap().unwrap();
        assert_eq!(
            (r.state.as_str(), r.reason.as_deref()),
            ("hidden", Some("rebuild_failed"))
        );
        assert!(r.text.is_none());
        // A page that fails the check does the same.
        build_core(&d, "2026-10", today(), &W(Ok(GOOD.into())))
            .await
            .unwrap();
        assert_eq!(
            read_in(&d, "2026-10", today()).unwrap().unwrap().state,
            "ok"
        );
        build_core(
            &d,
            "2026-10",
            today(),
            &W(Ok("Stream A made 999 JPY.".into())),
        )
        .await
        .unwrap();
        assert_eq!(
            read_in(&d, "2026-10", today()).unwrap().unwrap().state,
            "hidden"
        );
        // A good rebuild brings it back.
        build_core(&d, "2026-10", today(), &W(Ok(GOOD.into())))
            .await
            .unwrap();
        assert_eq!(
            read_in(&d, "2026-10", today()).unwrap().unwrap().state,
            "ok"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn only_one_review_build_runs_at_a_time() {
        let first = BuildSlot::take().expect("free");
        assert!(BuildSlot::take().is_none(), "busy while one runs");
        drop(first);
        let again = BuildSlot::take();
        assert!(again.is_some(), "freed on drop");
        drop(again);
        let src = production();
        let cmd = &src[src.find("pub async fn treasury_review_build").unwrap()..];
        assert!(cmd.contains("BuildSlot::take()") && cmd.contains("\"busy\""));
    }
}
