//! Stage G / G1: the 14-day parity report. READ-ONLY.
//!
//! Pre-takeover meaning (Q4 is open): `daily_range` (Amara's daily table) against the legacy NOOP
//! store, read through the SAME sandboxed path every other NOOP read uses (`patterns::fetch_noop_series`
//! -> `noop_query::run_query`: no shell, sandbox-exec deny-network, 10 s timeout, path allowlists).
//! No new process path, no write to the NOOP store, no network.
//!
//! - Per day and per measure (`sleep_total_min`, `hrv_ms`, `recovery`, `strain`) the two values are
//!   compared with an explicit tolerance (`TOL_*`, inclusive, plus `EPS` for float noise).
//! - A day that exists on one side only is MARKED (`missing_daily` / `missing_legacy`), never filled.
//!   A NULL is never read as 0: one side NULL and the other a value is `only_daily` / `only_legacy`;
//!   both NULL is `both_null` and counts as neither a match nor a mismatch.
//! - The full report (it holds values) goes to `<data>/parity/` only: the directory is 0700, the file
//!   0600, written atomically. The repo gets a synthetic fixture sample, never a live report.
//! - The command returns counts only. Nothing here logs a value.

use super::daily_table::{read_range_in, DailyRow};
use super::shared::fsx::atomic_write_with_mode;
use super::shared::task::{blocking, JoinErr};
use super::shared::time::{day_key, now_jst, now_ms};
use chrono::{Duration, NaiveDate};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

pub(crate) const DEFAULT_DAYS: u32 = 14;
pub(crate) const MAX_DAYS: u32 = 31;

/// Tolerances, inclusive (|daily - legacy| <= tol). Why these numbers:
/// - sleep_total_min: 1.0 minute. Both sides carry minutes; one rounding step of slack.
/// - hrv_ms: 0.5 ms. HRV is stored with a fraction; half a millisecond is below display precision.
/// - recovery: 0.5 point on the 0-100 scale (one display rounding step).
/// - strain: 0.1 on its 0-100 scale in this store (one decimal).
pub(crate) const TOL_SLEEP_TOTAL_MIN: f64 = 1.0;
pub(crate) const TOL_HRV_MS: f64 = 0.5;
pub(crate) const TOL_RECOVERY: f64 = 0.5;
pub(crate) const TOL_STRAIN: f64 = 0.1;
/// Float slack so that a difference of exactly the tolerance (5.1 - 5.0 is 0.0999..96 or 0.1000..1
/// depending on the operands) is not decided by representation noise. Far below any real step.
const EPS: f64 = 1e-9;

pub(crate) const MEASURES: [(&str, f64); 4] = [
    ("sleep_total_min", TOL_SLEEP_TOTAL_MIN),
    ("hrv_ms", TOL_HRV_MS),
    ("recovery", TOL_RECOVERY),
    ("strain", TOL_STRAIN),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Cell {
    Match,
    Mismatch,
    OnlyDaily,
    OnlyLegacy,
    BothNull,
}

/// Compare one measure. NULL (None) is never 0; a non-finite value counts as NULL.
pub(crate) fn compare(daily: Option<f64>, legacy: Option<f64>, tol: f64) -> Cell {
    let d = daily.filter(|v| v.is_finite());
    let l = legacy.filter(|v| v.is_finite());
    match (d, l) {
        (None, None) => Cell::BothNull,
        (Some(_), None) => Cell::OnlyDaily,
        (None, Some(_)) => Cell::OnlyLegacy,
        (Some(a), Some(b)) => {
            if (a - b).abs() <= tol + EPS {
                Cell::Match
            } else {
                Cell::Mismatch
            }
        }
    }
}

/// The legacy store's values for one day (NULL where the store had no point).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LegacyDay {
    pub sleep_total_min: Option<f64>,
    pub hrv_ms: Option<f64>,
    pub recovery: Option<f64>,
    pub strain: Option<f64>,
}

fn legacy_value(l: &LegacyDay, m: &str) -> Option<f64> {
    match m {
        "sleep_total_min" => l.sleep_total_min,
        "hrv_ms" => l.hrv_ms,
        "recovery" => l.recovery,
        _ => l.strain,
    }
}

fn daily_value(r: &DailyRow, m: &str) -> Option<f64> {
    match m {
        "sleep_total_min" => r.sleep_total_min,
        "hrv_ms" => r.hrv_ms,
        "recovery" => r.recovery,
        _ => r.strain,
    }
}

/// Legacy series (`patterns::NoopSeries` keys) into per-day values.
pub(crate) fn legacy_days(series: &[(&str, Vec<(String, f64)>)]) -> BTreeMap<String, LegacyDay> {
    let mut map: BTreeMap<String, LegacyDay> = BTreeMap::new();
    for (key, pts) in series {
        for (day, v) in pts {
            let d = map.entry(day.clone()).or_default();
            match *key {
                "sleep_total_min" => d.sleep_total_min = Some(*v),
                "hrv" => d.hrv_ms = Some(*v),
                "recovery" => d.recovery = Some(*v),
                "strain" => d.strain = Some(*v),
                _ => {}
            }
        }
    }
    map
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MeasureSummary {
    pub measure: String,
    pub tolerance: f64,
    pub matched: u32,
    pub mismatched: u32,
    pub only_daily: u32,
    pub only_legacy: u32,
    pub both_null: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ParitySummary {
    pub from: String,
    pub to: String,
    pub days: u32,
    /// Days with no daily_range row / no legacy point at all (marked, never filled).
    pub days_missing_daily: u32,
    pub days_missing_legacy: u32,
    /// `built_at_ms` of the daily table (0 = it has never been built).
    pub daily_built_at_ms: i64,
    pub measures: Vec<MeasureSummary>,
    /// File name inside `<data>/parity/` (never a full path).
    pub report_file: String,
}

/// Build the full report JSON (holds values; goes to the 0600 file only) and the counts.
pub(crate) fn build_report(
    from: NaiveDate,
    to: NaiveDate,
    daily: &[DailyRow],
    legacy: &BTreeMap<String, LegacyDay>,
    daily_built_at_ms: i64,
    generated_at_ms: i64,
) -> (serde_json::Value, ParitySummary) {
    let by_day: BTreeMap<&str, &DailyRow> = daily.iter().map(|r| (r.day.as_str(), r)).collect();
    let mut counts: Vec<MeasureSummary> = MEASURES
        .iter()
        .map(|(m, t)| MeasureSummary {
            measure: (*m).into(),
            tolerance: *t,
            ..Default::default()
        })
        .collect();
    let (mut miss_d, mut miss_l, mut total) = (0u32, 0u32, 0u32);
    let mut rows = vec![];
    let mut day = from;
    while day <= to {
        let key = day.format("%Y-%m-%d").to_string();
        let d = by_day.get(key.as_str()).copied();
        let l = legacy.get(&key);
        let status = match (d.is_some(), l.is_some()) {
            (true, true) => "both",
            (false, true) => "missing_daily",
            (true, false) => "missing_legacy",
            (false, false) => "missing_both",
        };
        if d.is_none() {
            miss_d += 1;
        }
        if l.is_none() {
            miss_l += 1;
        }
        let mut cells = serde_json::Map::new();
        for (i, (m, tol)) in MEASURES.iter().enumerate() {
            let dv = d.and_then(|r| daily_value(r, m)).filter(|v| v.is_finite());
            let lv = l.and_then(|r| legacy_value(r, m)).filter(|v| v.is_finite());
            let cell = compare(dv, lv, *tol);
            match cell {
                Cell::Match => counts[i].matched += 1,
                Cell::Mismatch => counts[i].mismatched += 1,
                Cell::OnlyDaily => counts[i].only_daily += 1,
                Cell::OnlyLegacy => counts[i].only_legacy += 1,
                Cell::BothNull => counts[i].both_null += 1,
            }
            let delta = match (dv, lv) {
                (Some(a), Some(b)) => Some(a - b),
                _ => None,
            };
            cells.insert(
                (*m).into(),
                json!({ "daily": dv, "legacy": lv, "delta": delta, "state": cell }),
            );
        }
        rows.push(json!({ "day": key, "status": status, "measures": cells }));
        total += 1;
        day += Duration::days(1);
    }
    let tolerances: serde_json::Map<String, serde_json::Value> = MEASURES
        .iter()
        .map(|(m, t)| ((*m).to_string(), json!(t)))
        .collect();
    let summary = ParitySummary {
        from: from.format("%Y-%m-%d").to_string(),
        to: to.format("%Y-%m-%d").to_string(),
        days: total,
        days_missing_daily: miss_d,
        days_missing_legacy: miss_l,
        daily_built_at_ms,
        measures: counts,
        report_file: String::new(),
    };
    let report = json!({
        "kind": "noop-parity",
        "meaning": "pre-takeover: daily_range vs the legacy NOOP store",
        "generatedAtMs": generated_at_ms,
        "from": summary.from,
        "to": summary.to,
        "dailyBuiltAtMs": daily_built_at_ms,
        "tolerances": tolerances,
        "rows": rows,
    });
    (report, summary)
}

const PARITY_DIR: &str = "parity";
const FAILED: &str = "parity: couldn't write the report";

/// `<data>/parity/` (0700) and the report file (0600), atomic. Returns the file name only.
pub(crate) fn write_report_in(
    data_dir: &Path,
    report: &serde_json::Value,
    from: &str,
    to: &str,
    at_ms: i64,
) -> Result<String, String> {
    let dir = data_dir.join(PARITY_DIR);
    ensure_private_dir(&dir)?;
    let name = format!("parity-{from}_{to}-{at_ms}.json");
    let body = serde_json::to_vec_pretty(report).map_err(|_| FAILED.to_string())?;
    atomic_write_with_mode(&dir.join(&name), &body, Some(0o600)).map_err(|_| FAILED.to_string())?;
    prune_old(&dir, KEEP_REPORTS);
    Ok(name)
}

/// Keep this many reports; older ones (our own name pattern only) are removed on each write.
pub(crate) const KEEP_REPORTS: usize = 10;

/// `parity-YYYY-MM-DD_YYYY-MM-DD-<ms>.json` -> the ms, else None.
fn report_ms(name: &str) -> Option<i64> {
    let rest = name.strip_prefix("parity-")?.strip_suffix(".json")?;
    let b = rest.as_bytes();
    // 10 + 1 + 10 + 1 = 22 bytes of two dates and separators, then the digits
    if b.len() < 23 || b[10] != b'_' || b[21] != b'-' {
        return None;
    }
    let day = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok();
    if !(day(&rest[..10]) && day(&rest[11..21])) {
        return None;
    }
    let ms = &rest[22..];
    if ms.is_empty() || !ms.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    ms.parse().ok()
}

/// Remove all but the newest `keep` matching regular files. Best effort; nothing else is touched.
pub(crate) fn prune_old(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut mine: Vec<(i64, std::path::PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let ms = report_ms(&e.file_name().to_string_lossy())?;
            let is_file = std::fs::symlink_metadata(e.path())
                .map(|m| m.file_type().is_file())
                .unwrap_or(false);
            is_file.then(|| (ms, e.path()))
        })
        .collect();
    mine.sort_by_key(|m| std::cmp::Reverse(m.0));
    for (_, p) in mine.into_iter().skip(keep) {
        let _ = std::fs::remove_file(p);
    }
}

/// The parity dir: created 0700, never a symlink, tightened if it exists with group/world bits.
fn ensure_private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let fail = || FAILED.to_string();
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(|_| fail())?;
    }
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(dir)
                .map_err(|_| fail())?;
        }
        Err(_) => return Err(fail()),
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => return Err(fail()),
        Ok(m) => {
            if m.permissions().mode() & 0o077 != 0 {
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| fail())?;
            }
        }
    }
    // Re-check after the fact: still a real directory, owner-only.
    let m = std::fs::symlink_metadata(dir).map_err(|_| fail())?;
    if m.file_type().is_symlink() || !m.is_dir() || m.permissions().mode() & 0o077 != 0 {
        return Err(fail());
    }
    Ok(())
}

pub(crate) fn validate_days(days: Option<u32>) -> Result<u32, String> {
    let d = days.unwrap_or(DEFAULT_DAYS);
    if d == 0 || d > MAX_DAYS {
        return Err(format!("days must be 1 to {MAX_DAYS}"));
    }
    Ok(d)
}

/// The whole report for a window ending at `today`, with the legacy read injected (tests pass a
/// fixture; the command passes the sandboxed reader).
pub(crate) fn report_in<R>(
    data_dir: &Path,
    days: u32,
    today: NaiveDate,
    read_legacy: R,
) -> Result<ParitySummary, String>
where
    R: FnOnce(i64) -> Result<BTreeMap<String, LegacyDay>, String>,
{
    let from = today - Duration::days(i64::from(days) - 1);
    let (f, t) = (
        from.format("%Y-%m-%d").to_string(),
        today.format("%Y-%m-%d").to_string(),
    );
    let range = read_range_in(data_dir, &f, &t)?;
    // Two spare days so a boundary day is not cut by the store's own window; filtered below.
    let mut legacy = read_legacy(i64::from(days) + 2)?;
    legacy.retain(|d, _| d.as_str() >= f.as_str() && d.as_str() <= t.as_str());
    let at = now_ms() as i64;
    let (report, mut summary) =
        build_report(from, today, &range.rows, &legacy, range.built_at_ms, at);
    summary.report_file = write_report_in(data_dir, &report, &f, &t, at)?;
    Ok(summary)
}

fn read_legacy_live(days: i64) -> Result<BTreeMap<String, LegacyDay>, String> {
    let series = super::patterns::fetch_noop_series(days)?;
    Ok(legacy_days(&series))
}

/// `days` 1..=31 (default 14; more is an error, not a clamp). Counts out; the report stays on disk.
#[tauri::command]
#[specta::specta]
pub async fn noop_parity_report(days: Option<u32>) -> Result<ParitySummary, String> {
    let days = validate_days(days)?;
    blocking(JoinErr::Plain, move || {
        report_in(
            &crate::app_paths::data_dir(),
            days,
            day_key(&now_jst()),
            read_legacy_live,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::daily_table::DailyRow;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("parity-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn nd(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    fn drow(
        day: &str,
        s: Option<f64>,
        h: Option<f64>,
        r: Option<f64>,
        st: Option<f64>,
    ) -> DailyRow {
        DailyRow {
            day: day.into(),
            sleep_total_min: s,
            hrv_ms: h,
            recovery: r,
            strain: st,
            ..Default::default()
        }
    }
    fn ld(s: Option<f64>, h: Option<f64>, r: Option<f64>, st: Option<f64>) -> LegacyDay {
        LegacyDay {
            sleep_total_min: s,
            hrv_ms: h,
            recovery: r,
            strain: st,
        }
    }

    #[test]
    fn tolerance_edges_are_inclusive_and_just_past_is_a_mismatch() {
        for (name, tol) in MEASURES {
            assert_eq!(compare(Some(50.0), Some(50.0), tol), Cell::Match, "{name}");
            assert_eq!(
                compare(Some(50.0), Some(50.0 + tol), tol),
                Cell::Match,
                "{name} at tol"
            );
            assert_eq!(
                compare(Some(50.0 + tol), Some(50.0), tol),
                Cell::Match,
                "{name} at -tol"
            );
            assert_eq!(
                compare(Some(50.0), Some(50.0 + tol + 0.01), tol),
                Cell::Mismatch,
                "{name} past tol"
            );
        }
        // the float-noise case the slack exists for
        assert_eq!(compare(Some(5.1), Some(5.0), TOL_STRAIN), Cell::Match);
        assert_eq!(compare(Some(5.2), Some(5.0), TOL_STRAIN), Cell::Mismatch);
        // documented values are pinned
        assert_eq!(
            MEASURES.map(|m| m.1),
            [1.0, 0.5, 0.5, 0.1],
            "tolerances changed: update the docs and the handoff"
        );
    }

    #[test]
    fn null_is_never_zero_and_non_finite_is_null() {
        assert_eq!(compare(None, None, 1.0), Cell::BothNull);
        // 0 vs NULL must not be a match in either direction
        assert_eq!(compare(Some(0.0), None, 1.0), Cell::OnlyDaily);
        assert_eq!(compare(None, Some(0.0), 1.0), Cell::OnlyLegacy);
        assert_eq!(compare(Some(f64::NAN), Some(0.0), 1.0), Cell::OnlyLegacy);
        assert_eq!(compare(Some(f64::INFINITY), None, 1.0), Cell::BothNull);
    }

    #[test]
    fn missing_days_are_marked_never_filled_and_counts_add_up() {
        let daily = vec![
            drow("2026-11-10", Some(400.0), Some(60.0), Some(70.0), Some(8.0)),
            drow("2026-11-11", Some(410.0), None, Some(70.0), None),
            // 12th has no row at all; 13th only exists on the legacy side
        ];
        let mut legacy = BTreeMap::new();
        legacy.insert(
            "2026-11-10".to_string(),
            ld(Some(400.5), Some(61.0), Some(70.2), Some(8.05)),
        );
        legacy.insert(
            "2026-11-11".to_string(),
            ld(Some(410.0), Some(55.0), None, None),
        );
        legacy.insert("2026-11-13".to_string(), ld(Some(300.0), None, None, None));
        let (rep, sum) = build_report(nd("2026-11-10"), nd("2026-11-14"), &daily, &legacy, 5, 9);
        assert_eq!(sum.days, 5);
        assert_eq!(sum.days_missing_daily, 3, "12th, 13th, 14th");
        assert_eq!(sum.days_missing_legacy, 2, "12th and 14th");
        let rows = rep["rows"].as_array().unwrap();
        let st = |i: usize| rows[i]["status"].as_str().unwrap().to_string();
        assert_eq!(
            [st(0), st(1), st(2), st(3), st(4)],
            [
                "both",
                "both",
                "missing_both",
                "missing_daily",
                "missing_both"
            ]
        );
        // a day missing on a side carries null there, never 0
        let c13 = &rows[3]["measures"]["sleep_total_min"];
        assert!(c13["daily"].is_null() && c13["state"] == "only_legacy");
        assert!(rows[2]["measures"]["recovery"]["state"] == "both_null");
        let m = |n: &str| {
            sum.measures
                .iter()
                .find(|m| m.measure == n)
                .unwrap()
                .clone()
        };
        // sleep: 10th match, 11th match, 13th only_legacy, 12th+14th both_null
        let s = m("sleep_total_min");
        assert_eq!(
            (s.matched, s.only_legacy, s.both_null, s.mismatched),
            (2, 1, 2, 0)
        );
        // hrv: 10th mismatch (1.0 > 0.5), 11th only_legacy (daily NULL, legacy 55)
        let h = m("hrv_ms");
        assert_eq!((h.mismatched, h.only_legacy, h.matched), (1, 1, 0));
        // recovery: 10th match, 11th only_daily
        let r = m("recovery");
        assert_eq!((r.matched, r.only_daily), (1, 1));
        for x in &sum.measures {
            assert_eq!(
                x.matched + x.mismatched + x.only_daily + x.only_legacy + x.both_null,
                5,
                "every day lands in exactly one bucket"
            );
        }
    }

    #[test]
    fn days_cap_default_and_zero() {
        assert_eq!(validate_days(None), Ok(14));
        assert_eq!(validate_days(Some(1)), Ok(1));
        assert_eq!(validate_days(Some(31)), Ok(31));
        assert!(validate_days(Some(32)).is_err());
        assert!(validate_days(Some(0)).is_err());
        assert!(validate_days(Some(u32::MAX)).is_err());
    }

    #[test]
    fn report_file_is_private_atomic_and_summary_holds_no_values() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("perms");
        let sum = report_in(&d, 3, nd("2026-11-14"), |days| {
            assert_eq!(days, 5, "two spare days");
            let mut m = BTreeMap::new();
            m.insert("2026-11-13".into(), ld(Some(401.0), None, Some(77.0), None));
            // outside the window: dropped, not compared
            m.insert("2026-11-01".into(), ld(Some(1.0), None, None, None));
            Ok(m)
        })
        .unwrap();
        let dir = d.join("parity");
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let file = dir.join(&sum.report_file);
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!sum.report_file.contains('/'), "file name only");
        // atomic: nothing but the report is left behind (no .tmp)
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec![sum.report_file.clone()]);
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(v["rows"].as_array().unwrap().len(), 3);
        assert_eq!(v["rows"][0]["day"], "2026-11-12");
        // no daily table was built here: all 3 days marked missing_daily, built_at 0
        assert_eq!(sum.days_missing_daily, 3);
        assert_eq!(sum.daily_built_at_ms, 0);
        assert_eq!(sum.days_missing_legacy, 2);
        // the returned summary serialises counts only
        // (the file name carries a millisecond timestamp, so it is left out of the value check)
        let s = serde_json::to_string(&sum)
            .unwrap()
            .replace(&sum.report_file, "<file>");
        assert!(!s.contains("401") && !s.contains("77"));
        // a re-run into an existing dir keeps the modes
        let sum2 = report_in(&d, 3, nd("2026-11-14"), |_| Ok(BTreeMap::new())).unwrap();
        assert_eq!(
            std::fs::metadata(dir.join(&sum2.report_file))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn parity_dir_refuses_a_symlink_tightens_a_loose_dir_and_keeps_ten_reports() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        // symlink: refused, and nothing written through it
        let d = tmp("link");
        let elsewhere = tmp("elsewhere");
        symlink(&elsewhere, d.join("parity")).unwrap();
        let e = write_report_in(&d, &json!({}), "2026-11-01", "2026-11-02", 1).unwrap_err();
        assert_eq!(e, FAILED);
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
        // a loose existing dir is tightened
        let d2 = tmp("loose");
        std::fs::create_dir(d2.join("parity")).unwrap();
        std::fs::set_permissions(d2.join("parity"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        write_report_in(&d2, &json!({}), "2026-11-01", "2026-11-02", 1).unwrap();
        assert_eq!(
            std::fs::metadata(d2.join("parity"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        // retention: 12 reports + a foreign file + a look-alike; newest 10 reports stay, others untouched
        let dir = d2.join("parity");
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join("parity-latest.json"), "x").unwrap();
        for ms in 2..=12 {
            write_report_in(&d2, &json!({}), "2026-11-01", "2026-11-02", ms).unwrap();
        }
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        let reports: Vec<_> = names.iter().filter(|n| report_ms(n).is_some()).collect();
        assert_eq!(reports.len(), KEEP_REPORTS);
        assert!(names.contains(&"parity-2026-11-01_2026-11-02-12.json".to_string()));
        assert!(!names.contains(&"parity-2026-11-01_2026-11-02-1.json".to_string()));
        assert!(!names.contains(&"parity-2026-11-01_2026-11-02-2.json".to_string()));
        assert!(
            names.contains(&"notes.txt".to_string())
                && names.contains(&"parity-latest.json".to_string())
        );
        assert_eq!(report_ms("parity-2026-11-01_2026-11-02-7.json"), Some(7));
        assert_eq!(report_ms("parity-latest.json"), None);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&elsewhere);
        let _ = std::fs::remove_dir_all(&d2);
    }

    #[test]
    fn a_legacy_read_failure_writes_nothing_and_fills_nothing() {
        let d = tmp("fail");
        let e = report_in(&d, 3, nd("2026-11-14"), |_| Err("noop unavailable".into())).unwrap_err();
        assert_eq!(e, "noop unavailable");
        assert!(!d.join("parity").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn legacy_series_maps_keys_and_the_command_uses_the_shared_sandboxed_reader() {
        let series = vec![
            ("sleep_total_min", vec![("2026-11-13".to_string(), 400.0)]),
            ("hrv", vec![("2026-11-13".to_string(), 61.0)]),
        ];
        let m = legacy_days(&series);
        assert_eq!(m["2026-11-13"], ld(Some(400.0), Some(61.0), None, None));
        // no new process path: the live reader is patterns::fetch_noop_series and nothing here spawns
        let src = include_str!("noop_parity.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        assert!(prod.contains("patterns::fetch_noop_series"));
        for banned in [
            "Command::new",
            "std::process",
            "TcpStream",
            "reqwest",
            "Connection::open",
        ] {
            assert!(!prod.contains(banned), "{banned}");
        }
    }

    /// The repo keeps a SYNTHETIC sample (invented numbers) so a reader can see the shape. A live
    /// report is never committed. `UPDATE_PARITY_FIXTURE=1` rewrites it.
    #[test]
    fn fixture_sample_matches_the_synthetic_inputs() {
        let daily = vec![
            drow(
                "2026-01-01",
                Some(420.0),
                Some(50.0),
                Some(60.0),
                Some(10.0),
            ),
            drow("2026-01-02", Some(430.0), Some(52.0), Some(65.0), None),
            drow(
                "2026-01-03",
                Some(300.0),
                Some(40.0),
                Some(30.0),
                Some(12.0),
            ),
        ];
        let mut legacy = BTreeMap::new();
        legacy.insert(
            "2026-01-01".to_string(),
            ld(Some(420.5), Some(50.4), Some(60.5), Some(10.1)),
        );
        legacy.insert(
            "2026-01-02".to_string(),
            ld(Some(433.0), Some(52.0), Some(65.0), Some(9.0)),
        );
        legacy.insert("2026-01-04".to_string(), ld(Some(380.0), None, None, None));
        let (rep, _) = build_report(nd("2026-01-01"), nd("2026-01-04"), &daily, &legacy, 1, 2);
        let text = serde_json::to_string_pretty(&rep).unwrap() + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../test/fixtures/noop-parity-sample.json");
        if std::env::var("UPDATE_PARITY_FIXTURE").is_ok() {
            std::fs::write(&path, &text).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            text,
            "fixture is stale"
        );
    }
}
