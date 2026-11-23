//! Treasury manual entries (decision 0045 item 8): type one income or one cost for a venture by hand.
//!
//! Same store and rules as the CSV import (`treasury.rs`): integer minor units (yen, or US cents), income
//! positive, a cost stored as a negative row (the same way a refund or fee already is), `source = 'manual'`.
//! The venture must exist in the ledger. If the venture has no active stream yet, one called "General"
//! is created so the entry has a home. Undo (8 s) deletes only a manual row this module just wrote; the
//! row's `raw_ref` carries its own timestamp. Nothing here reads the personal store, moves money or
//! touches the network, and nothing logs an amount.

use super::local_store::{clean_text, new_id};
use super::shared::task::{blocking, JoinErr};
use super::treasury::{is_date, is_month, open_db, store_err, stream_add_in};
use chrono::{DateTime, FixedOffset};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::path::Path;

const MAX_AMOUNT_MINOR: i64 = 1_000_000_000_000;
const UNDO_MS: i64 = 8_000;

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryEntry", rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub venture_id: String,
    pub date: String,
    /// Signed: income positive, a cost negative.
    pub amount_minor: i64,
    pub currency: String,
}

/// What one venture took in and paid out in one month, per currency. Never merged across currencies.
#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryVentureMonth", rename_all = "camelCase")]
pub struct VentureMonth {
    pub venture_id: String,
    pub currency: String,
    pub month: String,
    pub income_minor: i64,
    /// Costs, refunds and fees as a positive number.
    pub cost_minor: i64,
    pub rows: i64,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn entry_add_in(
    data: &Path,
    venture_id: &str,
    date: &str,
    amount_minor: i64,
    currency: &str,
    kind: &str,
    note: Option<&str>,
    at: DateTime<FixedOffset>,
) -> Result<Entry, String> {
    if !(1..=MAX_AMOUNT_MINOR).contains(&amount_minor) {
        return Err("invalid_input".into());
    }
    if currency != "JPY" && currency != "USD" {
        return Err("invalid_input".into());
    }
    if kind != "income" && kind != "cost" {
        return Err("invalid_input".into());
    }
    if !is_date(date) {
        return Err("invalid_input".into());
    }
    let note = match note {
        Some(n) => clean_text(n, 0, 80).map_err(|_| "invalid_input".to_string())?,
        None => String::new(),
    }
    .replace(';', ",");
    let conn = open_db(data)?;
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM streams WHERE venture_id = ?1 AND status = 'active' ORDER BY created_at, id LIMIT 1",
            params![venture_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(store_err)?;
    let stream_id = match existing {
        Some(id) => id,
        // Validates the venture against the ledger; an unknown venture is refused here.
        None => stream_add_in(data, venture_id, "General", "other", None, None)?.id,
    };
    let signed = if kind == "cost" {
        -amount_minor
    } else {
        amount_minor
    };
    let id = new_id("inc");
    let stamp = at.to_rfc3339_opts(chrono::SecondsFormat::Millis, false);
    let raw_ref = format!("manual:{stamp};note:{note}");
    conn.execute(
        "INSERT INTO income_events(id, stream_id, date, amount_minor, currency, source, raw_ref, dedupe_key, import_id)
         VALUES(?1, ?2, ?3, ?4, ?5, 'manual', ?6, ?7, NULL)",
        params![id, stream_id, date, signed, currency, raw_ref, new_id("man")],
    )
    .map_err(store_err)?;
    Ok(Entry {
        id,
        venture_id: venture_id.into(),
        date: date.into(),
        amount_minor: signed,
        currency: currency.into(),
    })
}

pub(crate) fn entry_undo_in(
    data: &Path,
    id: &str,
    now: DateTime<FixedOffset>,
) -> Result<bool, String> {
    let conn = open_db(data)?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT raw_ref FROM income_events WHERE id = ?1 AND source = 'manual'",
            params![id],
            |r| r.get(0),
        )
        .optional()
        .map_err(store_err)?;
    let Some(raw) = raw else { return Ok(false) };
    let stamp = raw
        .strip_prefix("manual:")
        .and_then(|r| r.split(';').next())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
    let Some(stamp) = stamp else { return Ok(false) };
    if (now - stamp).num_milliseconds() > UNDO_MS {
        return Ok(false);
    }
    let n = conn
        .execute(
            "DELETE FROM income_events WHERE id = ?1 AND source = 'manual'",
            params![id],
        )
        .map_err(store_err)?;
    Ok(n == 1)
}

pub(crate) fn venture_month_in(data: &Path, month: &str) -> Result<Vec<VentureMonth>, String> {
    if !is_month(month) {
        return Err("invalid_input".into());
    }
    let conn = open_db(data)?;
    let mut stmt = conn
        .prepare(
            "SELECT s.venture_id, e.currency,
                    COALESCE(SUM(CASE WHEN e.amount_minor > 0 THEN e.amount_minor END), 0),
                    COALESCE(-SUM(CASE WHEN e.amount_minor < 0 THEN e.amount_minor END), 0),
                    COUNT(*)
             FROM income_events e JOIN streams s ON s.id = e.stream_id
             WHERE substr(e.date, 1, 7) = ?1
             GROUP BY s.venture_id, e.currency ORDER BY s.venture_id, e.currency",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map(params![month], |r| {
            Ok(VentureMonth {
                venture_id: r.get(0)?,
                currency: r.get(1)?,
                month: month.to_string(),
                income_minor: r.get(2)?,
                cost_minor: r.get(3)?,
                rows: r.get(4)?,
            })
        })
        .map_err(store_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(store_err)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub async fn treasury_entry_add(
    venture_id: String,
    date: String,
    amount_minor: i64,
    currency: String,
    kind: String,
    note: Option<String>,
) -> Result<Entry, String> {
    blocking(JoinErr::Code("store_error"), move || {
        entry_add_in(
            &crate::app_paths::data_dir(),
            &venture_id,
            &date,
            amount_minor,
            &currency,
            &kind,
            note.as_deref(),
            super::shared::time::now_jst(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_entry_undo(id: String) -> Result<bool, String> {
    blocking(JoinErr::Code("store_error"), move || {
        entry_undo_in(
            &crate::app_paths::data_dir(),
            &id,
            super::shared::time::now_jst(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_venture_month(month: String) -> Result<Vec<VentureMonth>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        venture_month_in(&crate::app_paths::data_dir(), &month)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("amara-tentries-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(d.join("business")).unwrap();
        std::fs::write(
            d.join("business").join("ledger.json"),
            r#"{"version":1,"ventures":[{"id":"v1","name":"V1","stage":"Live","health":"on_track","note":""}]}"#,
        )
        .unwrap();
        d
    }
    fn t(s: i64) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-19T10:00:00+09:00").unwrap()
            + chrono::Duration::seconds(s)
    }

    #[test]
    fn income_positive_cost_negative_and_month_splits_in_and_out() {
        let d = dir();
        entry_add_in(&d, "v1", "2026-11-03", 12_000, "JPY", "income", None, t(0)).unwrap();
        let c = entry_add_in(
            &d,
            "v1",
            "2026-11-04",
            2_500,
            "JPY",
            "cost",
            Some("Packaging"),
            t(1),
        )
        .unwrap();
        assert_eq!(c.amount_minor, -2_500);
        entry_add_in(&d, "v1", "2026-10-30", 999, "USD", "income", None, t(2)).unwrap();
        let m = venture_month_in(&d, "2026-11").unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(
            (m[0].income_minor, m[0].cost_minor, m[0].rows),
            (12_000, 2_500, 2)
        );
        let o = venture_month_in(&d, "2026-10").unwrap();
        assert_eq!(
            (o[0].currency.as_str(), o[0].income_minor, o[0].cost_minor),
            ("USD", 999, 0)
        );
        assert!(venture_month_in(&d, "2026-11-01").is_err());
    }

    #[test]
    fn reuses_one_stream_and_refuses_unknown_ventures_and_bad_input() {
        let d = dir();
        entry_add_in(&d, "v1", "2026-11-03", 1, "JPY", "income", None, t(0)).unwrap();
        entry_add_in(&d, "v1", "2026-11-03", 1, "JPY", "income", None, t(1)).unwrap();
        let n: i64 = open_db(&d)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM streams", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        for (v, date, amt, cur, kind) in [
            ("ghost", "2026-11-03", 5, "JPY", "income"),
            ("v1", "2026-13-03", 5, "JPY", "income"),
            ("v1", "2026-11-03", 0, "JPY", "income"),
            ("v1", "2026-11-03", -5, "JPY", "income"),
            ("v1", "2026-11-03", 5, "EUR", "income"),
            ("v1", "2026-11-03", 5, "JPY", "gift"),
        ] {
            assert!(
                entry_add_in(&d, v, date, amt, cur, kind, None, t(2)).is_err(),
                "{v} {date} {amt} {cur} {kind}"
            );
        }
    }

    #[test]
    fn undo_deletes_only_a_fresh_manual_row() {
        let d = dir();
        let a = entry_add_in(&d, "v1", "2026-11-03", 100, "JPY", "income", None, t(0)).unwrap();
        assert!(!entry_undo_in(&d, &a.id, t(9)).unwrap());
        assert!(entry_undo_in(&d, &a.id, t(3)).unwrap());
        assert!(!entry_undo_in(&d, &a.id, t(3)).unwrap());
        // An imported row is never touched.
        let conn = open_db(&d).unwrap();
        let sid: String = conn
            .query_row("SELECT id FROM streams LIMIT 1", [], |r| r.get(0))
            .unwrap();
        conn.execute("INSERT INTO income_events(id,stream_id,date,amount_minor,currency,source,raw_ref,dedupe_key) VALUES('imp1',?1,'2026-11-01',5,'JPY','generic','row:1','k')", params![sid]).unwrap();
        assert!(!entry_undo_in(&d, "imp1", t(1)).unwrap());
    }

    #[test]
    fn this_file_never_touches_the_personal_store_or_network() {
        let src = include_str!("treasury_entries.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        for banned in ["personal", "reqwest", "TcpStream", "life_money"] {
            assert!(
                !prod.replace("the personal store", "").contains(banned),
                "{banned}"
            );
        }
    }
}
