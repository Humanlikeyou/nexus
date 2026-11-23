//! Treasury, phase 3: customer counts per stage and per segment (decision 0032 #2: contacts are NOT in
//! Amara; names, emails and every other contact field stay with the provider).
//!
//! A CSV is mapped with three user-chosen columns: a stage column (required), a count column (optional;
//! without it every row counts as one contact) and a segment column (optional). Only those cells are
//! ever read; every other cell of a row is dropped unread, and the file's text is dropped when parsing
//! ends. What is stored is `segment_counts(stream_id, as_of, stage, segment, count)` in the same private
//! `business/treasury.db`, one snapshot per import date (JST). Importing again the same day replaces that
//! day's snapshot, so it is idempotent.
//!
//! Rules this file keeps (each has a test):
//! - No contact field is ever stored, logged or returned. A segment must be a short ASCII slug (no spaces,
//!   no non-ASCII, no `.`/`:`/`@`), and a file may have at most max(20, rows/20) (never over 30) distinct
//!   segments, so a wrongly mapped name/email column is refused whole.
//! - Errors are fixed codes; a bad file's text never reaches an error or a log (nothing here logs).
//! - Missing is `None`, never 0: a stage with no rows in an import is `null`, not `0`.
//!
//! Outside calls: none. No network client, no URL.

use super::local_store::new_id;
use super::shared::task::{blocking, JoinErr};
use super::treasury::{
    find_col, now_iso, open_db, parse_csv, pick_csv_titled, read_import_file, record_failed,
    store_err, stream_exists, today, ImportResult, Slot, MAX_CELLS,
};
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

pub const STAGES: [&str; 5] = ["lead", "waitlist", "first_purchase", "repeat", "lapsed"];
const ADAPTER_COUNTS: &str = "counts";
const MAX_SEGMENT_CHARS: usize = 24;
const MAX_SEGMENTS: usize = 30;
const MAX_COUNT: i64 = 10_000_000;
const TREND_DATES: i64 = 12;

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename = "TreasuryCountsMapping", rename_all = "camelCase")]
pub struct CountsMapping {
    /// The column holding `lead | waitlist | first_purchase | repeat | lapsed`.
    pub stage_column: String,
    /// A column with a number of contacts for that row. Without it each row counts as one.
    pub count_column: Option<String>,
    /// A column with a segment or tag name (never a name, email or phone).
    pub segment_column: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryStageCount", rename_all = "camelCase")]
pub struct StageCount {
    pub stage: String,
    /// `None` when that import had no row for the stage (never 0).
    pub count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryCountRow", rename_all = "camelCase")]
pub struct CountRow {
    pub stage: String,
    pub segment: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryCountsTrendRow", rename_all = "camelCase")]
pub struct CountsTrendRow {
    pub as_of: String,
    /// All five stages, in `STAGES` order.
    pub stages: Vec<StageCount>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryCountsSnapshot", rename_all = "camelCase")]
pub struct CountsSnapshot {
    /// The newest import date; `None` when nothing was imported.
    pub as_of: Option<String>,
    /// Latest per-stage totals (all five stages, `None` where not reported).
    pub stages: Vec<StageCount>,
    /// Latest per stage and segment (only rows that have a segment).
    pub segments: Vec<CountRow>,
    /// The last 12 import dates, oldest first.
    pub trend: Vec<CountsTrendRow>,
}

struct Mapped {
    stage: usize,
    count: Option<usize>,
    segment: Option<usize>,
}

fn opt_col(header: &[String], name: &Option<String>) -> Result<Option<usize>, String> {
    match name.as_deref() {
        Some(c) if !c.trim().is_empty() => Ok(Some(find_col(header, c).ok_or("bad_header")?)),
        _ => Ok(None),
    }
}

fn stage_of(raw: &str) -> Option<&'static str> {
    let k: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c == ' ' || c == '-' { '_' } else { c })
        .collect();
    STAGES.iter().copied().find(|s| *s == k)
}

/// A segment is a short ASCII slug: `^[a-z0-9][a-z0-9_-]{0,23}$` after trimming and lowercasing.
/// Internal whitespace, non-ASCII text (names, fullwidth digits), `.`, `:` and `@` are all refused, so
/// "john smith", "x at y dot com", a domain or a phone number can't become a segment. A seven-digit run is
/// refused as well. A one-word first name still fits the slug, which is why the import also caps how many
/// distinct segments a file may have (see `segment_limit`).
fn segment_of(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_lowercase();
    let b = s.as_bytes();
    if b.is_empty() || b.len() > MAX_SEGMENT_CHARS || !s.is_ascii() {
        return None;
    }
    if !(b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
        return None;
    }
    if !b
        .iter()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
    {
        return None;
    }
    if b.iter().filter(|c| c.is_ascii_digit()).count() >= 7 {
        return None;
    }
    Some(s)
}

/// At most `MAX_SEGMENTS`, and no more than max(20, rows / 20) distinct segments for the file.
fn segment_limit(rows_seen: i64) -> usize {
    MAX_SEGMENTS.min(20.max((rows_seen / 20) as usize))
}

fn count_of(raw: &str) -> Option<i64> {
    let t = raw.trim();
    if t.is_empty() || t.len() > 9 || !t.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    t.parse::<i64>().ok().filter(|n| *n <= MAX_COUNT)
}

struct Reduced {
    sha: String,
    rows_seen: i64,
    failed: i64,
    counts: BTreeMap<(&'static str, String), i64>,
}

/// Parses and reduces to counts in memory. Only the three mapped cells of each row are read.
fn reduce_file(mapping: &CountsMapping, path: &Path) -> Result<Reduced, (Option<String>, String)> {
    let bytes = read_import_file(path).map_err(|e| (None, e))?;
    let sha = hex::encode(Sha256::digest(&bytes));
    let fail = |c: &str| (Some(sha.clone()), c.to_string());
    let text = std::str::from_utf8(&bytes).map_err(|_| fail("bad_encoding"))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rows = parse_csv(text).map_err(|e| fail(&e))?;
    let mut iter = rows.into_iter();
    let (_, header) = iter.next().ok_or_else(|| fail("bad_header"))?;
    let cols = (|| -> Result<Mapped, String> {
        Ok(Mapped {
            stage: find_col(&header, &mapping.stage_column).ok_or("bad_header")?,
            count: opt_col(&header, &mapping.count_column)?,
            segment: opt_col(&header, &mapping.segment_column)?,
        })
    })()
    .map_err(|e| fail(&e))?;
    let mut counts: BTreeMap<(&'static str, String), i64> = BTreeMap::new();
    let (mut rows_seen, mut failed) = (0i64, 0i64);
    for (_, cells) in iter {
        rows_seen += 1;
        let one = (|| -> Option<(&'static str, String, i64)> {
            if cells.len() > MAX_CELLS {
                return None;
            }
            let stage = stage_of(cells.get(cols.stage)?)?;
            let n = match cols.count {
                Some(i) => count_of(cells.get(i)?)?,
                None => 1,
            };
            let seg = match cols.segment {
                Some(i) => {
                    let raw = cells.get(i)?;
                    if raw.trim().is_empty() {
                        String::new()
                    } else {
                        segment_of(raw)?
                    }
                }
                None => String::new(),
            };
            Some((stage, seg, n))
        })();
        match one {
            Some((stage, seg, n)) => {
                let e = counts.entry((stage, seg)).or_insert(0);
                *e = (*e + n).min(MAX_COUNT * 100);
            }
            None => failed += 1,
        }
    }
    let segments: std::collections::HashSet<&String> = counts.keys().map(|(_, s)| s).collect();
    let distinct = segments.iter().filter(|s| !s.is_empty()).count();
    if distinct > segment_limit(rows_seen) {
        return Err(fail("too_many_segments"));
    }
    if counts.is_empty() {
        return Err(fail("no_rows"));
    }
    Ok(Reduced {
        sha,
        rows_seen,
        failed,
        counts,
    })
}

fn validate_mapping(m: &CountsMapping) -> Result<(), String> {
    let long = |c: &Option<String>| c.as_deref().is_some_and(|c| c.chars().count() > 128);
    if m.stage_column.trim().is_empty()
        || m.stage_column.chars().count() > 128
        || long(&m.count_column)
        || long(&m.segment_column)
    {
        return Err("invalid_input".into());
    }
    Ok(())
}

pub(crate) fn counts_import_in(
    data: &Path,
    stream_id: &str,
    mapping: &CountsMapping,
    path: Option<&Path>,
    as_of: NaiveDate,
) -> Result<ImportResult, String> {
    let path = path.ok_or_else(|| "cancelled".to_string())?;
    validate_mapping(mapping)?;
    if stream_id.chars().count() > 64 {
        return Err("invalid_input".into());
    }
    let mut conn = open_db(data)?;
    if !stream_exists(&conn, stream_id)? {
        return Err("unknown_stream".into());
    }
    let red = match reduce_file(mapping, path) {
        Ok(r) => r,
        Err((sha, code)) => {
            record_failed(&conn, stream_id, ADAPTER_COUNTS, sha.as_deref(), &code);
            return Err(code);
        }
    };
    match commit_counts(
        &mut conn,
        stream_id,
        &red,
        &as_of.format("%Y-%m-%d").to_string(),
    ) {
        Ok(r) => Ok(r),
        Err(code) => {
            record_failed(&conn, stream_id, ADAPTER_COUNTS, Some(&red.sha), &code);
            Err(code)
        }
    }
}

fn commit_counts(
    conn: &mut Connection,
    stream_id: &str,
    red: &Reduced,
    as_of: &str,
) -> Result<ImportResult, String> {
    let tx = conn.transaction().map_err(store_err)?;
    let import_id = new_id("imp");
    tx.execute(
        "INSERT INTO imports(id, stream_id, adapter, file_sha256, row_count, added, skipped, status, at)
         VALUES(?1, ?2, ?3, ?4, ?5, 0, 0, 'ok', ?6)",
        params![import_id, stream_id, ADAPTER_COUNTS, red.sha, red.rows_seen, now_iso()],
    )
    .map_err(store_err)?;
    let mut existing: BTreeMap<(String, String), i64> = BTreeMap::new();
    {
        let mut q = tx
            .prepare("SELECT stage, segment, count FROM segment_counts WHERE stream_id = ?1 AND as_of = ?2")
            .map_err(store_err)?;
        let rows = q
            .query_map(params![stream_id, as_of], |r| {
                Ok(((r.get(0)?, r.get(1)?), r.get(2)?))
            })
            .map_err(store_err)?;
        for r in rows {
            let (k, v) = r.map_err(store_err)?;
            existing.insert(k, v);
        }
    }
    tx.execute(
        "DELETE FROM segment_counts WHERE stream_id = ?1 AND as_of = ?2",
        params![stream_id, as_of],
    )
    .map_err(store_err)?;
    let (mut added, mut skipped) = (0i64, 0i64);
    for ((stage, segment), n) in &red.counts {
        if existing.get(&(stage.to_string(), segment.clone())) == Some(n) {
            skipped += 1;
        } else {
            added += 1;
        }
        tx.execute(
            "INSERT INTO segment_counts(stream_id, as_of, stage, segment, count, import_id) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![stream_id, as_of, stage, segment, n, import_id],
        )
        .map_err(store_err)?;
    }
    let status = if red.failed > 0 { "partial" } else { "ok" };
    tx.execute(
        "UPDATE imports SET added = ?1, skipped = ?2, status = ?3 WHERE id = ?4",
        params![added, skipped, status, import_id],
    )
    .map_err(store_err)?;
    tx.commit().map_err(store_err)?;
    Ok(ImportResult {
        added,
        skipped,
        failed: red.failed,
        import_id,
    })
}

fn stage_totals(
    conn: &Connection,
    stream_id: &str,
    as_of: &str,
) -> Result<Vec<StageCount>, String> {
    let mut q = conn
        .prepare("SELECT stage, SUM(count) FROM segment_counts WHERE stream_id = ?1 AND as_of = ?2 GROUP BY stage")
        .map_err(store_err)?;
    let rows = q
        .query_map(params![stream_id, as_of], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(store_err)?;
    let mut m: BTreeMap<String, i64> = BTreeMap::new();
    for r in rows {
        let (s, n) = r.map_err(store_err)?;
        m.insert(s, n);
    }
    Ok(STAGES
        .iter()
        .map(|s| StageCount {
            stage: s.to_string(),
            count: m.get(*s).copied(),
        })
        .collect())
}

pub(crate) fn counts_list_in(data: &Path, stream_id: &str) -> Result<CountsSnapshot, String> {
    let conn = open_db(data)?;
    if !stream_exists(&conn, stream_id)? {
        return Err("unknown_stream".into());
    }
    let mut q = conn
        .prepare("SELECT DISTINCT as_of FROM segment_counts WHERE stream_id = ?1 ORDER BY as_of DESC LIMIT ?2")
        .map_err(store_err)?;
    let mut dates: Vec<String> = q
        .query_map(params![stream_id, TREND_DATES], |r| r.get(0))
        .map_err(store_err)?
        .collect::<Result<_, _>>()
        .map_err(store_err)?;
    dates.reverse();
    let Some(latest) = dates.last().cloned() else {
        return Ok(CountsSnapshot {
            as_of: None,
            stages: Vec::new(),
            segments: Vec::new(),
            trend: Vec::new(),
        });
    };
    let mut trend = Vec::new();
    for d in &dates {
        trend.push(CountsTrendRow {
            as_of: d.clone(),
            stages: stage_totals(&conn, stream_id, d)?,
        });
    }
    let mut sq = conn
        .prepare(
            "SELECT stage, segment, count FROM segment_counts
             WHERE stream_id = ?1 AND as_of = ?2 AND segment <> '' ORDER BY stage, segment",
        )
        .map_err(store_err)?;
    let mut segments: Vec<CountRow> = sq
        .query_map(params![stream_id, latest], |r| {
            Ok(CountRow {
                stage: r.get(0)?,
                segment: r.get(1)?,
                count: r.get(2)?,
            })
        })
        .map_err(store_err)?
        .collect::<Result<_, _>>()
        .map_err(store_err)?;
    segments.sort_by_key(|r| (STAGES.iter().position(|s| *s == r.stage), r.segment.clone()));
    Ok(CountsSnapshot {
        as_of: Some(latest.clone()),
        stages: stage_totals(&conn, stream_id, &latest)?,
        segments,
        trend,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_counts_import_pick(
    app: tauri::AppHandle,
    stream_id: String,
    mapping: CountsMapping,
) -> Result<ImportResult, String> {
    let _slot = Slot::take().ok_or_else(|| "busy".to_string())?;
    let path = pick_csv_titled(app, "Choose a customer counts CSV").await?;
    blocking(JoinErr::Code("failed"), move || {
        counts_import_in(
            &crate::app_paths::data_dir(),
            &stream_id,
            &mapping,
            path.as_deref(),
            today(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_counts_list(stream_id: String) -> Result<CountsSnapshot, String> {
    blocking(JoinErr::Code("store_error"), move || {
        counts_list_in(&crate::app_paths::data_dir(), &stream_id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::treasury::test_support::{csv_file, setup};
    use std::fs;

    fn m() -> CountsMapping {
        CountsMapping {
            stage_column: "Stage".into(),
            count_column: None,
            segment_column: Some("Tag".into()),
        }
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    const CONTACTS: &str = "Name,Email,Phone,Stage,Tag\n\
Alice Zephyrine,alice.zephyrine@example.invalid,+81 90 1234 5678,lead,newsletter\n\
Bob Quillfeather,bob.quillfeather@example.invalid,03-1111-2222,lead,newsletter\n\
Carol Marrowdale,carol.marrowdale@example.invalid,,first_purchase,vip\n\
Dan Thistlewick,dan.thistlewick@example.invalid,,First Purchase,\n\
Eve Ostrander,eve.ostrander@example.invalid,,lapsed,vip\n";

    #[test]
    fn one_row_per_contact_is_reduced_to_counts_and_no_contact_field_is_stored() {
        let (d, sid) = setup("counts-bytes");
        let f = csv_file(&d, "c.csv", CONTACTS);
        let r = counts_import_in(&d, &sid, &m(), Some(&f), day("2026-11-18")).unwrap();
        assert_eq!((r.added, r.skipped, r.failed), (4, 0, 0));
        let snap = counts_list_in(&d, &sid).unwrap();
        assert_eq!(snap.as_of.as_deref(), Some("2026-11-18"));
        let get = |s: &str| snap.stages.iter().find(|x| x.stage == s).unwrap().count;
        assert_eq!(get("lead"), Some(2));
        assert_eq!(get("first_purchase"), Some(2));
        assert_eq!(get("lapsed"), Some(1));
        assert_eq!(get("waitlist"), None, "not reported is null, never 0");
        assert_eq!(get("repeat"), None);
        assert!(snap.segments.contains(&CountRow {
            stage: "lead".into(),
            segment: "newsletter".into(),
            count: 2
        }));
        // Byte scan: db, wal and shm hold no name, email or phone from the file.
        let db = d.join("business");
        for entry in fs::read_dir(&db).unwrap() {
            let bytes = fs::read(entry.unwrap().path()).unwrap_or_default();
            let hay = String::from_utf8_lossy(&bytes).to_lowercase();
            for needle in [
                "zephyrine",
                "quillfeather",
                "marrowdale",
                "thistlewick",
                "ostrander",
                "example.invalid",
                "1234 5678",
                "1111-2222",
                "alice",
                "@example",
            ] {
                assert!(!hay.contains(needle), "db bytes contain {needle}");
            }
        }
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_count_column_is_summed_and_importing_the_same_day_twice_is_idempotent() {
        let (d, sid) = setup("counts-twice");
        let map = CountsMapping {
            stage_column: "stage".into(),
            count_column: Some("n".into()),
            segment_column: None,
        };
        let f = csv_file(&d, "c.csv", "stage,n\nlead,10\nlead,5\nrepeat,3\n");
        let r1 = counts_import_in(&d, &sid, &map, Some(&f), day("2026-11-18")).unwrap();
        assert_eq!((r1.added, r1.skipped), (2, 0));
        let r2 = counts_import_in(&d, &sid, &map, Some(&f), day("2026-11-18")).unwrap();
        assert_eq!((r2.added, r2.skipped), (0, 2));
        let snap = counts_list_in(&d, &sid).unwrap();
        assert_eq!(snap.stages[0].count, Some(15));
        assert_eq!(snap.trend.len(), 1);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn the_trend_keeps_one_snapshot_per_import_date_oldest_first() {
        let (d, sid) = setup("counts-trend");
        let a = csv_file(&d, "a.csv", "Stage\nlead\nlead\n");
        let b = csv_file(&d, "b.csv", "Stage\nlead\nlead\nlead\nwaitlist\n");
        counts_import_in(
            &d,
            &sid,
            &CountsMapping {
                stage_column: "Stage".into(),
                count_column: None,
                segment_column: None,
            },
            Some(&a),
            day("2026-11-10"),
        )
        .unwrap();
        counts_import_in(
            &d,
            &sid,
            &CountsMapping {
                stage_column: "Stage".into(),
                count_column: None,
                segment_column: None,
            },
            Some(&b),
            day("2026-11-17"),
        )
        .unwrap();
        let snap = counts_list_in(&d, &sid).unwrap();
        assert_eq!(snap.as_of.as_deref(), Some("2026-11-17"));
        assert_eq!(snap.trend.len(), 2);
        assert_eq!(snap.trend[0].as_of, "2026-11-10");
        assert_eq!(snap.trend[0].stages[0].count, Some(2));
        assert_eq!(snap.trend[0].stages[1].count, None);
        assert_eq!(snap.trend[1].stages[0].count, Some(3));
        assert_eq!(snap.trend[1].stages[1].count, Some(1));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_contact_looking_segment_rejects_the_row_and_never_reaches_an_error() {
        let (d, sid) = setup("counts-seg");
        // The segment column is mapped to the email column by mistake.
        let f = csv_file(
            &d,
            "c.csv",
            "Stage,Tag\nlead,zed.nobody@example.invalid\nlead,+81 90 1234 5678\nlead,vip\nnonsense,vip\n",
        );
        let r = counts_import_in(&d, &sid, &m(), Some(&f), day("2026-11-18")).unwrap();
        assert_eq!((r.added, r.failed), (1, 3));
        let snap = counts_list_in(&d, &sid).unwrap();
        assert_eq!(snap.segments.len(), 1);
        // Only the email column: nothing usable, a fixed code, and the code names no contact data.
        let g = csv_file(&d, "g.csv", "Stage,Tag\nlead,zed.nobody@example.invalid\n");
        let e = counts_import_in(&d, &sid, &m(), Some(&g), day("2026-11-19")).unwrap_err();
        assert_eq!(e, "no_rows");
        assert!(!e.contains('@'));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn bad_files_fail_with_fixed_codes_and_leave_counts_untouched() {
        let (d, sid) = setup("counts-bad");
        let ok = csv_file(&d, "ok.csv", "Stage\nlead\n");
        counts_import_in(
            &d,
            &sid,
            &CountsMapping {
                stage_column: "Stage".into(),
                count_column: None,
                segment_column: None,
            },
            Some(&ok),
            day("2026-11-18"),
        )
        .unwrap();
        let wrong = csv_file(&d, "w.csv", "Email,Level\nnobody@example.invalid,lead\n");
        let e = counts_import_in(&d, &sid, &m(), Some(&wrong), day("2026-11-18")).unwrap_err();
        assert_eq!(e, "bad_header");
        let bin = d.join("bin.csv");
        fs::write(&bin, [0xff, 0xfe, 0x00, 0x41]).unwrap();
        assert_eq!(
            counts_import_in(&d, &sid, &m(), Some(&bin), day("2026-11-18")).unwrap_err(),
            "bad_encoding"
        );
        #[cfg(unix)]
        {
            let link = d.join("link.csv");
            std::os::unix::fs::symlink(&ok, &link).unwrap();
            assert_eq!(
                counts_import_in(&d, &sid, &m(), Some(&link), day("2026-11-18")).unwrap_err(),
                "bad_file"
            );
        }
        assert_eq!(
            counts_import_in(&d, "nope", &m(), Some(&ok), day("2026-11-18")).unwrap_err(),
            "unknown_stream"
        );
        assert_eq!(
            counts_import_in(&d, &sid, &m(), None, day("2026-11-18")).unwrap_err(),
            "cancelled"
        );
        assert_eq!(counts_list_in(&d, &sid).unwrap().stages[0].count, Some(1));
        let conn = open_db(&d).unwrap();
        let failed: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM imports WHERE status LIKE 'failed:%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(failed >= 2);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn an_empty_store_reads_as_no_data() {
        let (d, sid) = setup("counts-empty");
        let s = counts_list_in(&d, &sid).unwrap();
        assert_eq!(s.as_of, None);
        assert!(s.stages.is_empty() && s.trend.is_empty());
        assert_eq!(counts_list_in(&d, "nope").unwrap_err(), "unknown_stream");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn only_ascii_slugs_are_segments() {
        for ok in ["vip", "newsletter", "q4-2026", "a_b", "9lives", "  VIP "] {
            assert!(segment_of(ok).is_some(), "{ok}");
        }
        for bad in [
            "\u{ff11}\u{ff12}\u{ff13}\u{ff14}\u{ff15}\u{ff16}\u{ff17}",
            "john smith",
            "\u{5c71}\u{7530}\u{592a}\u{90ce}",
            "x at y dot com",
            "example.com",
            "a:b",
            "x@y",
            "-lead",
            "1234567",
            "",
            "abcdefghijklmnopqrstuvwxyz",
        ] {
            assert!(segment_of(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_column_of_one_hundred_names_is_refused_whole_and_nothing_is_stored() {
        let (d, sid) = setup("counts-names");
        let mut body = String::from("Stage,Tag\n");
        for i in 0..100 {
            body.push_str(&format!("lead,zorblaxq{i:03}x\n"));
        }
        let f = csv_file(&d, "n.csv", &body);
        let e = counts_import_in(&d, &sid, &m(), Some(&f), day("2026-11-18")).unwrap_err();
        assert_eq!(e, "too_many_segments");
        assert_eq!(counts_list_in(&d, &sid).unwrap().as_of, None);
        for entry in fs::read_dir(d.join("business")).unwrap() {
            let bytes = fs::read(entry.unwrap().path()).unwrap_or_default();
            assert!(!String::from_utf8_lossy(&bytes).contains("zorblax"));
        }
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn overlong_ids_and_column_names_are_refused() {
        let (d, sid) = setup("counts-long");
        let f = csv_file(&d, "o.csv", "Stage\nlead\n");
        let mut long = m();
        long.stage_column = "s".repeat(129);
        assert_eq!(
            counts_import_in(&d, &sid, &long, Some(&f), day("2026-11-18")).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            counts_import_in(&d, &"i".repeat(65), &m(), Some(&f), day("2026-11-18")).unwrap_err(),
            "invalid_input"
        );
        let _ = fs::remove_dir_all(d);
    }

    fn production() -> String {
        let src = include_str!("treasury_counts.rs");
        src.split(concat!("#[cfg(test)]", "\nmod tests {"))
            .next()
            .unwrap()
            .to_string()
    }

    #[test]
    fn no_logging_no_network_no_personal_store_and_safe_command_names() {
        let src = production();
        for needle in [
            "println!",
            "eprintln!",
            "dbg!",
            "log::",
            "tracing::",
            "info!(",
            "warn!(",
            "error!(",
            "debug!(",
            "reqwest",
            "shared::http",
            "TcpStream",
            "UdpSocket",
            concat!("http", "://"),
            concat!("https", "://"),
            "Command::new",
            concat!("money", "::"),
            concat!("money", "_summary"),
            "llm_router",
            "amara_turn",
        ] {
            assert!(!src.contains(needle), "production source contains {needle}");
        }
        let mut names = Vec::new();
        let mut lines = src.lines();
        while let Some(l) = lines.next() {
            if l.trim() == "#[tauri::command]" {
                let f = lines.find(|l| l.contains("fn ")).unwrap();
                names.push(
                    f.split("fn ")
                        .nth(1)
                        .unwrap()
                        .split('(')
                        .next()
                        .unwrap()
                        .trim()
                        .to_string(),
                );
            }
        }
        assert_eq!(
            names,
            ["treasury_counts_import_pick", "treasury_counts_list"]
        );
        for n in &names {
            for b in crate::commands::treasury::test_support::BANNED {
                assert!(!n.contains(b), "{n} contains {b}");
            }
        }
    }

    #[test]
    fn no_chat_or_context_module_reads_treasury_counts() {
        for (name, src) in [
            ("amara_turn.rs", include_str!("amara_turn.rs")),
            ("terrence_context.rs", include_str!("terrence_context.rs")),
            ("turn_memory.rs", include_str!("turn_memory.rs")),
        ] {
            let p = src.to_lowercase();
            assert!(
                !p.contains("segment_counts") && !p.contains("treasury_counts"),
                "{name}"
            );
        }
    }
}
