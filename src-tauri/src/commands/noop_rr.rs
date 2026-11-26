//! Session HRV from the WHOOP strap's beat-to-beat (RR) intervals, as NOOP stores them (decision 0061).
//!
//! NOOP's `whoop.sqlite` has an `rrInterval` table. Read strictly read-only (`noop::open_store`); never
//! written, checkpointed or copied. RR intervals arrive when the strap syncs, not live, so HRV is a
//! per-session figure computed after the sync, never in-session feedback.
//!
//! Method (`HRV_METHOD`):
//! - Only the session's own time window. One source channel: NOOP stores some beats on more than one
//!   channel, so the channel with the most beats in the window is used and the others are left out.
//! - Rows NOOP marks `tsSuspect` are left out.
//! - Plausible intervals only (300–2000 ms).
//! - A successive pair counts only when the beats are at most 3 s apart and the interval changes by at
//!   most 20% (a common artifact and ectopic-beat filter).
//! - RMSSD is the root mean square of the accepted successive differences.
//!
//! The result is reported only with enough data: at least 2 minutes, 100 accepted pairs, and beats in
//! 60% of the window's minutes. Otherwise it is "not available" with the reason. HRV is never computed
//! from EEG or from a heart-rate estimate. Health values are never logged.

use super::noop;
use super::shared::task::{blocking, JoinErr};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const HRV_METHOD: &str = "rmssd-v1-0061: WHOOP strap RR intervals via NOOP (read-only); one source channel per window (the one with most beats); tsSuspect rows excluded; RR 300-2000 ms; successive pairs at most 3 s apart and changing at most 20%; RMSSD over accepted pairs";
pub(crate) const SOURCE: &str = "WHOOP strap RR intervals, read from NOOP";
const MIN_RR_MS: i64 = 300;
const MAX_RR_MS: i64 = 2000;
const MAX_PAIR_GAP_S: i64 = 3;
const MAX_CHANGE: f64 = 0.2;
const MIN_WINDOW_S: i64 = 120;
const MAX_WINDOW_S: i64 = 4 * 3600;
const MIN_PAIRS: u32 = 100;
const MIN_COVERAGE: f64 = 0.6;
const MAX_ROWS: i64 = 60_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionHrv {
    /// `available` or `not_available`.
    pub state: String,
    pub rmssd_ms: Option<f64>,
    pub mean_rr_ms: Option<f64>,
    pub beats: u32,
    pub accepted_pairs: u32,
    pub rejected_pairs: u32,
    /// Share of the window's minutes that have beats, 0–100.
    pub coverage_pct: f64,
    /// Newest RR interval NOOP holds for this strap (RFC 3339), so a missing session can say "not synced yet".
    pub newest_rr_at: Option<String>,
    pub reason: Option<String>,
    pub source: String,
    pub method: String,
}

fn unavailable(reason: String, newest: Option<i64>) -> SessionHrv {
    SessionHrv {
        state: "not_available".into(),
        rmssd_ms: None,
        mean_rr_ms: None,
        beats: 0,
        accepted_pairs: 0,
        rejected_pairs: 0,
        coverage_pct: 0.0,
        newest_rr_at: newest.and_then(iso),
        reason: Some(reason),
        source: SOURCE.into(),
        method: HRV_METHOD.into(),
    }
}

fn iso(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.to_rfc3339())
}

fn newest_rr(conn: &Connection, device: &str) -> Option<i64> {
    conn.query_row(
        "SELECT MAX(ts) FROM rrInterval WHERE deviceId = ?1 AND IFNULL(tsSuspect, 0) = 0",
        params![device],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
}

/// RMSSD and its counts from beats in time order: `(ts seconds, rr ms)`.
pub(crate) fn rmssd(beats: &[(i64, i64)]) -> (Option<f64>, Option<f64>, u32, u32) {
    let ok: Vec<&(i64, i64)> = beats
        .iter()
        .filter(|(_, rr)| (MIN_RR_MS..=MAX_RR_MS).contains(rr))
        .collect();
    let mean = (!ok.is_empty())
        .then(|| ok.iter().map(|(_, rr)| *rr as f64).sum::<f64>() / ok.len() as f64);
    let (mut sum, mut accepted, mut rejected) = (0.0, 0u32, 0u32);
    for w in ok.windows(2) {
        let ((t0, a), (t1, b)) = (*w[0], *w[1]);
        let (a, b) = (a as f64, b as f64);
        if t1 - t0 > MAX_PAIR_GAP_S || t1 < t0 || (b - a).abs() > MAX_CHANGE * a {
            rejected += 1;
            continue;
        }
        sum += (b - a) * (b - a);
        accepted += 1;
    }
    let r = (accepted > 0).then(|| (sum / f64::from(accepted)).sqrt());
    (r, mean, accepted, rejected)
}

pub(crate) fn session_hrv_in(conn: &Connection, device: &str, start: i64, end: i64) -> SessionHrv {
    let newest = newest_rr(conn, device);
    if end - start < MIN_WINDOW_S {
        return unavailable("The session is under 2 minutes.".into(), newest);
    }
    let end = end.min(start + MAX_WINDOW_S);
    let rows: Vec<(i64, i64, i64)> = conn
        .prepare(
            "SELECT ts, rrMs, IFNULL(srcChannel, -1) FROM rrInterval
             WHERE deviceId = ?1 AND ts >= ?2 AND ts <= ?3 AND IFNULL(tsSuspect, 0) = 0
             ORDER BY ts, IFNULL(ord, 0), seq LIMIT ?4",
        )
        .and_then(|mut s| {
            s.query_map(params![device, start, end, MAX_ROWS], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect()
        })
        .unwrap_or_default();
    if rows.is_empty() {
        let reason = match newest {
            Some(n) if n < end => {
                "The strap hasn't synced this session's beats to NOOP yet.".into()
            }
            _ => "NOOP has no beat-to-beat data for this session.".into(),
        };
        return unavailable(reason, newest);
    }
    let mut per_channel: BTreeMap<i64, u32> = BTreeMap::new();
    for (_, _, c) in &rows {
        *per_channel.entry(*c).or_default() += 1;
    }
    let channel = per_channel
        .iter()
        .max_by_key(|(c, n)| (**n, -**c))
        .map(|(c, _)| *c)
        .unwrap_or(-1);
    let beats: Vec<(i64, i64)> = rows
        .iter()
        .filter(|(_, _, c)| *c == channel)
        .map(|(t, rr, _)| (*t, *rr))
        .collect();
    let minutes = ((end - start) as f64 / 60.0).ceil().max(1.0);
    let covered = beats
        .iter()
        .map(|(t, _)| (t - start) / 60)
        .collect::<std::collections::BTreeSet<_>>()
        .len() as f64;
    let coverage = (covered / minutes).min(1.0);
    let (r, mean, accepted, rejected) = rmssd(&beats);
    let round1 = |v: f64| (v * 10.0).round() / 10.0;
    let mut out = SessionHrv {
        state: "available".into(),
        rmssd_ms: r.map(round1),
        mean_rr_ms: mean.map(round1),
        beats: beats.len() as u32,
        accepted_pairs: accepted,
        rejected_pairs: rejected,
        coverage_pct: round1(coverage * 100.0),
        newest_rr_at: newest.and_then(iso),
        reason: None,
        source: SOURCE.into(),
        method: HRV_METHOD.into(),
    };
    let short = if coverage < MIN_COVERAGE {
        Some("Beats cover less than 60% of the session (the strap may have lost contact or not synced fully).")
    } else if accepted < MIN_PAIRS {
        Some("Fewer than 100 clean beat-to-beat pairs.")
    } else {
        None
    };
    if let Some(why) = short {
        out.state = "not_available".into();
        out.rmssd_ms = None;
        out.reason = Some(why.into());
    }
    out
}

/// For the biofeedback card: is NOOP holding beat-to-beat data for this strap, and up to when.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RrStatus {
    pub available: bool,
    pub newest_rr_at: Option<String>,
    pub beats_last_24h: u32,
    pub source: String,
}

pub(crate) fn rr_status_in(conn: &Connection, device: &str) -> RrStatus {
    let newest = newest_rr(conn, device);
    let count = newest
        .and_then(|n| {
            conn.query_row(
                "SELECT COUNT(*) FROM rrInterval WHERE deviceId = ?1 AND ts > ?2 AND IFNULL(tsSuspect, 0) = 0",
                params![device, n - 86_400],
                |r| r.get::<_, i64>(0),
            )
            .ok()
        })
        .unwrap_or(0);
    RrStatus {
        available: newest.is_some(),
        newest_rr_at: newest.and_then(iso),
        beats_last_24h: u32::try_from(count).unwrap_or(u32::MAX),
        source: SOURCE.into(),
    }
}

fn with_store<T>(f: impl FnOnce(&Connection, &str) -> T, missing: T) -> T {
    // Tests never open the real NOOP store (personal health data); they use in-memory stores.
    if cfg!(test) {
        return missing;
    }
    let Some((device, path)) = noop::current_store() else {
        return missing;
    };
    match noop::open_store(&path) {
        Ok(conn) => f(&conn, &device),
        Err(_) => missing,
    }
}

fn window_hrv(c: &Connection, d: &str, start: &str, end: &str) -> SessionHrv {
    match (
        chrono::DateTime::parse_from_rfc3339(start),
        chrono::DateTime::parse_from_rfc3339(end),
    ) {
        (Ok(s), Ok(e)) => session_hrv_in(c, d, s.timestamp(), e.timestamp()),
        _ => unavailable("The session times couldn't be read.".into(), None),
    }
}

fn no_store() -> SessionHrv {
    unavailable(
        "NOOP isn't installed or its store can't be read.".into(),
        None,
    )
}

/// Session HRV for one window (RFC 3339 start and end), or a reason it isn't available.
pub(crate) fn session_hrv(start: &str, end: &str) -> SessionHrv {
    with_store(|c, d| window_hrv(c, d, start, end), no_store())
}

/// Several windows with one read-only open of the store (security review, 0061).
pub(crate) fn session_hrv_many(windows: &[(String, String)]) -> Vec<SessionHrv> {
    with_store(
        |c, d| {
            windows
                .iter()
                .map(|(s, e)| window_hrv(c, d, s, e))
                .collect()
        },
        windows.iter().map(|_| no_store()).collect(),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn noop_session_hrv(start: String, end: String) -> Result<SessionHrv, String> {
    blocking(JoinErr::Plain, move || Ok(session_hrv(&start, &end))).await
}

#[tauri::command]
#[specta::specta]
pub async fn noop_rr_status() -> Result<RrStatus, String> {
    blocking(JoinErr::Plain, || {
        Ok(with_store(
            rr_status_in,
            RrStatus {
                available: false,
                newest_rr_at: None,
                beats_last_24h: 0,
                source: SOURCE.into(),
            },
        ))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(r#"CREATE TABLE rrInterval ("deviceId" TEXT NOT NULL, "ts" INTEGER NOT NULL, "rrMs" INTEGER NOT NULL, "seq" INTEGER NOT NULL DEFAULT 0, "synced" INTEGER NOT NULL DEFAULT 0, "ord" INTEGER, "srcChannel" INTEGER, "tsSuspect" INTEGER, PRIMARY KEY ("deviceId", "ts", "rrMs", "seq"));"#).unwrap();
        c
    }

    /// One beat roughly every second for `secs`, alternating 980/1020 ms (RMSSD 40 ms).
    fn fill(c: &Connection, start: i64, secs: i64, channel: Option<i64>) {
        for i in 0..secs {
            let rr = if i % 2 == 0 { 980 } else { 1020 };
            c.execute("INSERT INTO rrInterval (deviceId, ts, rrMs, seq, ord, srcChannel) VALUES ('d', ?1, ?2, ?3, ?3, ?4)", params![start + i, rr, i, channel]).unwrap();
        }
    }

    #[test]
    fn rmssd_of_a_clean_alternating_series() {
        let beats: Vec<(i64, i64)> = (0..10)
            .map(|i| (i, if i % 2 == 0 { 980 } else { 1020 }))
            .collect();
        let (r, mean, ok, bad) = rmssd(&beats);
        assert_eq!((r, mean, ok, bad), (Some(40.0), Some(1000.0), 9, 0));
    }

    #[test]
    fn artifacts_gaps_and_implausible_beats_are_left_out() {
        let beats = vec![
            (0, 1000),
            (1, 1010),
            (2, 2500),
            (3, 1000),
            (4, 1500),
            (10, 1000),
            (11, 990),
        ];
        let (_, _, ok, bad) = rmssd(&beats);
        // 2500 ms is dropped as implausible, leaving 5 pairs: 1000->1010, 1010->1000 (2 s apart) and
        // 1000->990 are clean; 1000->1500 is a 50% jump and 1500->1000 spans a 6 s gap.
        assert_eq!((ok, bad), (3, 2));
    }

    #[test]
    fn a_full_session_gives_rmssd_from_one_channel() {
        let c = store();
        fill(&c, 1_000_000, 600, Some(8));
        // The same beats on a second channel, offset: never counted twice.
        for i in 0..100 {
            c.execute("INSERT INTO rrInterval (deviceId, ts, rrMs, seq, srcChannel) VALUES ('d', ?1, 700, ?2, 10)", params![1_000_000 + i, 1000 + i]).unwrap();
        }
        let h = session_hrv_in(&c, "d", 1_000_000, 1_000_600);
        assert_eq!(h.state, "available", "{h:?}");
        assert_eq!(h.rmssd_ms, Some(40.0));
        assert_eq!(h.beats, 600);
        assert!(h.coverage_pct >= 99.0);
        assert!(h.method.starts_with("rmssd-v1-0061"));
    }

    #[test]
    fn not_synced_yet_says_so() {
        let c = store();
        fill(&c, 1_000_000, 60, None);
        let h = session_hrv_in(&c, "d", 2_000_000, 2_000_600);
        assert_eq!(h.state, "not_available");
        assert_eq!(h.rmssd_ms, None);
        assert!(h.reason.unwrap().contains("hasn't synced"));
        assert!(h.newest_rr_at.is_some());
    }

    #[test]
    fn thin_coverage_or_suspect_rows_are_not_a_reading() {
        let c = store();
        fill(&c, 1_000_000, 120, Some(8));
        let h = session_hrv_in(&c, "d", 1_000_000, 1_000_900);
        assert_eq!(h.state, "not_available");
        assert!(h.reason.unwrap().contains("60%"));
        let c2 = store();
        fill(&c2, 1_000_000, 600, Some(8));
        c2.execute("UPDATE rrInterval SET tsSuspect = 1", [])
            .unwrap();
        assert_eq!(session_hrv_in(&c2, "d", 1_000_000, 1_000_600).beats, 0);
    }

    #[test]
    fn short_sessions_and_other_straps_give_nothing() {
        let c = store();
        fill(&c, 1_000_000, 600, Some(8));
        assert!(session_hrv_in(&c, "d", 1_000_000, 1_000_060)
            .reason
            .unwrap()
            .contains("2 minutes"));
        assert_eq!(
            session_hrv_in(&c, "other", 1_000_000, 1_000_600).state,
            "not_available"
        );
    }

    #[test]
    fn rr_status_reports_the_newest_beat() {
        let c = store();
        assert!(!rr_status_in(&c, "d").available);
        fill(&c, 1_000_000, 50, Some(8));
        let s = rr_status_in(&c, "d");
        assert!(s.available);
        assert_eq!(s.beats_last_24h, 50);
    }
}
