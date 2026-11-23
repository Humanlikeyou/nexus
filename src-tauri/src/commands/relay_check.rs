//! Stage G / G2: the relay's number check. Every number in the text that goes out (the model's prose
//! AND the plain fallback line) must trace to the `daily_range` row for that day.
//!
//! - Values are rendered from the row by ONE function (`render`). The check re-derives each value from
//!   the row carried with the event and compares it with what the event holds, then removes those
//!   rendered values from the text: any digit left over is a number the row does not contain.
//! - A row that is NULL for the needed value, older than 36 h (`STALE_MS`), or for a day before
//!   yesterday (JST) is not trusted: no text with numbers is sent for it, prose or plain.
//! - Order: prose -> plain line (also checked) -> nothing. No fallback to the NOOP snapshot.
//! - Pure functions; no IO, no clock (callers pass `now_ms`).

use super::daily_table::{DailyRange, DailyRow};
use chrono::{Duration, NaiveDate};

/// Older than this (since the daily table was built) is stale.
pub(crate) const STALE_MS: i64 = 36 * 60 * 60 * 1000;

/// A row plus when its table was built: what an event is allowed to say.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Trace {
    pub row: DailyRow,
    pub built_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Block {
    Stale,
    NullValue,
    UntracedValue,
    UntracedNumber,
}

impl Block {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Block::Stale => "stale row",
            Block::NullValue => "null value",
            Block::UntracedValue => "value not from the row",
            Block::UntracedNumber => "number not in the row",
        }
    }
}

fn hm(min: f64) -> String {
    let t = min.round() as i64;
    format!("{}h {}m", t / 60, t % 60)
}

/// The one rendering of a row value. `None` for NULL, non-finite or out-of-range values.
pub(crate) fn render(name: &str, row: &DailyRow) -> Option<String> {
    let f = |v: Option<f64>| v.filter(|x| x.is_finite());
    match name {
        "SLEEP_TOTAL" => f(row.sleep_total_min).filter(|m| *m > 0.0).map(hm),
        "RECOVERY" => f(row.recovery)
            .filter(|r| *r >= 0.0)
            .map(|r| format!("{}%", r.round() as i64)),
        "HRV" => f(row.hrv_ms)
            .filter(|h| *h > 0.0)
            .map(|h| format!("{} ms", h.round() as i64)),
        _ => None,
    }
}

/// Fresh enough to speak from: built within 36 h and the row's day is today or yesterday (JST).
pub(crate) fn check_fresh(t: &Trace, now_ms: i64, today: NaiveDate) -> Result<(), Block> {
    let day = NaiveDate::parse_from_str(&t.row.day, "%Y-%m-%d").map_err(|_| Block::Stale)?;
    if t.built_at_ms <= 0
        || now_ms.saturating_sub(t.built_at_ms) > STALE_MS
        || day < today - Duration::days(1)
        || day > today
    {
        return Err(Block::Stale);
    }
    Ok(())
}

/// The newest row at or after yesterday with a usable value for `placeholder` (None when there is
/// none: NULL means no event, never a guess).
pub(crate) fn pick_row(range: &DailyRange, placeholder: &str) -> Option<Trace> {
    range
        .rows
        .iter()
        .rev()
        .find(|r| render(placeholder, r).is_some())
        .map(|r| Trace {
            row: r.clone(),
            built_at_ms: range.built_at_ms,
        })
}

/// Check `text` (final prose or plain line) for an event whose rendered values are `vars`
/// (placeholder, rendered value).
pub(crate) fn check_text(
    text: &str,
    vars: &[(&str, &str)],
    t: &Trace,
    now_ms: i64,
    today: NaiveDate,
) -> Result<(), Block> {
    check_fresh(t, now_ms, today)?;
    for (name, shown) in vars {
        match render(name, &t.row) {
            None => return Err(Block::NullValue),
            Some(v) if v != *shown => return Err(Block::UntracedValue),
            Some(_) => {}
        }
    }
    // Remove the traced values (longest first), then no numeral of any script may remain.
    let mut rest = text.to_string();
    let mut shown: Vec<&str> = vars.iter().map(|(_, s)| *s).collect();
    shown.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for s in shown {
        rest = rest.replace(s, " ");
    }
    if rest.chars().any(|c| c.is_numeric()) {
        return Err(Block::UntracedNumber);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 13).unwrap()
    }
    fn row(day: &str) -> DailyRow {
        DailyRow {
            day: day.into(),
            sleep_total_min: Some(452.0),
            hrv_ms: Some(66.4),
            recovery: Some(74.0),
            ..Default::default()
        }
    }
    fn tr(day: &str, built: i64) -> Trace {
        Trace {
            row: row(day),
            built_at_ms: built,
        }
    }
    const REC: [(&str, &str); 1] = [("RECOVERY", "74%")];

    #[test]
    fn renders_from_the_row_only() {
        let r = row("2026-11-13");
        assert_eq!(render("SLEEP_TOTAL", &r).as_deref(), Some("7h 32m"));
        assert_eq!(render("RECOVERY", &r).as_deref(), Some("74%"));
        assert_eq!(render("HRV", &r).as_deref(), Some("66 ms"));
        for gone in ["DEEP", "REM", "RHR", "STRAIN"] {
            assert_eq!(render(gone, &r), None, "{gone} has no placeholder any more");
        }
        let n = DailyRow::default();
        assert_eq!(render("RECOVERY", &n), None);
        assert_eq!(
            render(
                "SLEEP_TOTAL",
                &DailyRow {
                    sleep_total_min: Some(0.0),
                    ..n
                }
            ),
            None
        );
    }

    #[test]
    fn traced_text_passes_and_a_foreign_number_blocks() {
        let t = tr("2026-11-13", NOW);
        assert_eq!(
            check_text("Recovery is 74% today.", &REC, &t, NOW, today()),
            Ok(())
        );
        assert_eq!(check_text("Recovery: 74%.", &REC, &t, NOW, today()), Ok(()));
        assert_eq!(
            check_text("Recovery is 99% today.", &REC, &t, NOW, today()),
            Err(Block::UntracedNumber)
        );
        // a second number next to a traced one
        assert_eq!(
            check_text("Recovery is 74% after 8 hours.", &REC, &t, NOW, today()),
            Err(Block::UntracedNumber)
        );
        // glued digits cannot hide behind a traced value
        assert_eq!(
            check_text("Recovery is 174%.", &REC, &t, NOW, today()),
            Err(Block::UntracedNumber)
        );
        // other scripts count as numbers
        assert_eq!(
            check_text("Recovery is \u{FF17}\u{FF14}% 74%.", &REC, &t, NOW, today()),
            Err(Block::UntracedNumber)
        );
    }

    #[test]
    fn an_event_value_that_differs_from_the_row_blocks() {
        let t = tr("2026-11-13", NOW);
        assert_eq!(
            check_text("Recovery: 80%.", &[("RECOVERY", "80%")], &t, NOW, today()),
            Err(Block::UntracedValue)
        );
        // value NULL in the row
        let mut n = t.clone();
        n.row.recovery = None;
        assert_eq!(
            check_text("Recovery: 74%.", &REC, &n, NOW, today()),
            Err(Block::NullValue)
        );
    }

    #[test]
    fn stale_rows_block_at_the_edge() {
        let ok = tr("2026-11-13", NOW - STALE_MS);
        assert_eq!(
            check_text("Recovery: 74%.", &REC, &ok, NOW, today()),
            Ok(())
        );
        let old = tr("2026-11-13", NOW - STALE_MS - 1);
        assert_eq!(
            check_text("Recovery: 74%.", &REC, &old, NOW, today()),
            Err(Block::Stale)
        );
        // never built
        assert_eq!(
            check_text("Recovery: 74%.", &REC, &tr("2026-11-13", 0), NOW, today()),
            Err(Block::Stale)
        );
        // a fresh table but a row from two days ago, and a future day
        assert_eq!(
            check_text("Recovery: 74%.", &REC, &tr("2026-11-11", NOW), NOW, today()),
            Err(Block::Stale)
        );
        assert_eq!(
            check_text("Recovery: 74%.", &REC, &tr("2026-11-12", NOW), NOW, today()),
            Ok(())
        );
        assert_eq!(
            check_text("Recovery: 74%.", &REC, &tr("2026-11-14", NOW), NOW, today()),
            Err(Block::Stale)
        );
    }

    #[test]
    fn pick_row_takes_the_newest_usable_row_and_never_a_null() {
        let mut a = row("2026-11-12");
        a.recovery = Some(50.0);
        let mut b = row("2026-11-13");
        b.recovery = None;
        let range = DailyRange {
            rows: vec![a, b],
            goals: vec![],
            built_at_ms: NOW,
        };
        assert_eq!(pick_row(&range, "RECOVERY").unwrap().row.day, "2026-11-12");
        assert_eq!(
            pick_row(&range, "SLEEP_TOTAL").unwrap().row.day,
            "2026-11-13"
        );
        let empty = DailyRange {
            rows: vec![DailyRow::default()],
            goals: vec![],
            built_at_ms: NOW,
        };
        assert!(pick_row(&empty, "RECOVERY").is_none());
    }
}
