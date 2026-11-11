//! Read-only bridge to NOOP's authoritative local health store.
//!
//! Amara deliberately does not pair with the strap, calculate WHOOP metrics, or
//! maintain a second health database. NOOP owns those jobs. This module only opens
//! NOOP's SQLite database read-only and exposes a bounded snapshot for Amara.

use chrono::{DateTime, Local, NaiveDate, Timelike, Utc};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_DEVICE_ID: &str = "my-whoop";
const NOOP_BUNDLE_ID: &str = "com.noopapp.noop";
const RECENT_DAY_LIMIT: usize = 14;
/// Newest day more than this many days old → status "stale".
const STALE_AFTER_DAYS: i64 = 2;
const BASELINE_DAY_LIMIT: usize = 7;
const BATTERY_MAX_AGE_SECONDS: i64 = 48 * 60 * 60;
const BATTERY_MAX_FUTURE_SKEW_SECONDS: i64 = 5 * 60;
const BATTERY_MAX_LOCAL_CONFIRMATION_AGE_SECONDS: i64 = 48 * 60 * 60;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoopDailyMetric {
    pub day: String,
    pub source: String,
    pub total_sleep_min: Option<f64>,
    pub efficiency: Option<f64>,
    pub deep_min: Option<f64>,
    pub rem_min: Option<f64>,
    pub light_min: Option<f64>,
    pub resting_hr: Option<i64>,
    pub avg_hrv: Option<f64>,
    pub recovery: Option<f64>,
    pub strain: Option<f64>,
    pub spo2_pct: Option<f64>,
    pub skin_temp_dev_c: Option<f64>,
    pub resp_rate_bpm: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoopSleepSession {
    pub day: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_min: f64,
    pub efficiency: Option<f64>,
    pub deep_min: Option<f64>,
    pub rem_min: Option<f64>,
    pub light_min: Option<f64>,
    pub awake_min: Option<f64>,
    pub session_count: usize,
    pub resting_hr: Option<i64>,
    pub avg_hrv: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default)]
struct SleepStageMinutes {
    awake: f64,
    light: f64,
    deep: f64,
    rem: f64,
}

impl SleepStageMinutes {
    fn asleep(self) -> f64 {
        self.light + self.deep + self.rem
    }

    fn in_bed(self) -> f64 {
        self.asleep() + self.awake
    }

    fn add(&mut self, other: Self) {
        self.awake += other.awake;
        self.light += other.light;
        self.deep += other.deep;
        self.rem += other.rem;
    }
}

fn decode_sleep_stage_minutes(json: Option<&str>) -> Option<SleepStageMinutes> {
    let value = serde_json::from_str::<serde_json::Value>(json?).ok()?;
    if let Some(segments) = value.as_array() {
        let mut minutes = SleepStageMinutes::default();
        for segment in segments {
            let Some(start) = segment.get("start").and_then(serde_json::Value::as_f64) else {
                continue;
            };
            let Some(end) = segment.get("end").and_then(serde_json::Value::as_f64) else {
                continue;
            };
            let Some(stage) = segment.get("stage").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if end <= start {
                continue;
            }
            let value = (end - start) / 60.0;
            match stage {
                "wake" | "awake" => minutes.awake += value,
                "light" => minutes.light += value,
                "deep" => minutes.deep += value,
                "rem" => minutes.rem += value,
                _ => continue,
            }
        }
        return (minutes.in_bed() > 0.0).then_some(minutes);
    }
    let object = value.as_object()?;
    let number = |key: &str| {
        object
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0)
    };
    let minutes = SleepStageMinutes {
        awake: number("awake") + number("wake"),
        light: number("light"),
        deep: number("deep"),
        rem: number("rem"),
    };
    (minutes.in_bed() > 0.0).then_some(minutes)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoopBatteryState {
    pub percentage: f64,
    pub charging: Option<bool>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoopHealthInsight {
    pub metric: String,
    pub fact: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoopHealthSnapshot {
    pub source: String,
    pub status: String,
    pub generated_at: String,
    pub device_id: String,
    pub latest_day: Option<String>,
    /// Newest day that holds a reading; see [`reading_day`]. `today` may be an empty row.
    pub reading_day: Option<String>,
    pub data_age_days: Option<i64>,
    pub today: Option<NoopDailyMetric>,
    pub recent_days: Vec<NoopDailyMetric>,
    pub latest_sleep: Option<NoopSleepSession>,
    pub latest_night: Option<NoopSleepSession>,
    pub battery: Option<NoopBatteryState>,
    pub insights: Vec<NoopHealthInsight>,
    pub message: Option<String>,
}

/// Which NOOP store a snapshot was read from (logged/printed; never values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoopStoreKind {
    /// `NOOP_DB_PATH` override.
    Explicit,
    /// Sandboxed production app: ~/Library/Containers/com.noopapp.noop/...
    Container,
    /// Unsandboxed build: ~/Library/Application Support/OpenWhoop/...
    AppSupport,
}

#[cfg(test)]
impl NoopStoreKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Container => "container",
            Self::AppSupport => "app-support",
        }
    }
}

const STORE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Open a NOOP store strictly read-only. NOOP may be writing to it right now:
/// never write, checkpoint, VACUUM or copy it.
fn open_store(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(STORE_BUSY_TIMEOUT)?;
    Ok(conn)
}

/// Newest `dailyMetric.day` across the imported and NOOP-computed rows, or
/// None if the store can't be read (it still loses to any readable store).
fn store_latest_day(path: &Path, device_id: &str) -> Option<String> {
    let conn = open_store(path).ok()?;
    conn.query_row(
        "SELECT MAX(day) FROM dailyMetric WHERE deviceId IN (?1, ?2)",
        params![device_id, format!("{device_id}-noop")],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
}

/// Newer mtime of the database file or its -wal (NOOP writes the WAL first).
fn store_mtime(path: &Path) -> Option<std::time::SystemTime> {
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    [path.to_path_buf(), PathBuf::from(wal)]
        .iter()
        .filter_map(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
        .max()
}

#[derive(Debug, Clone)]
struct NoopPaths {
    explicit: Option<PathBuf>,
    home: PathBuf,
}

impl NoopPaths {
    fn from_environment() -> Self {
        Self {
            explicit: env::var_os("NOOP_DB_PATH")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            home: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")),
        }
    }

    /// Production stores only. The `com.noopapp.noop.staging` container is
    /// never a candidate.
    fn candidates(&self) -> Vec<(NoopStoreKind, PathBuf)> {
        if let Some(path) = self.explicit.as_ref() {
            return vec![(NoopStoreKind::Explicit, expand_home(path, &self.home))];
        }
        vec![
            (
                NoopStoreKind::Container,
                self.home
                    .join("Library/Containers")
                    .join(NOOP_BUNDLE_ID)
                    .join("Data/Library/Application Support/OpenWhoop/whoop.sqlite"),
            ),
            (
                NoopStoreKind::AppSupport,
                self.home
                    .join("Library/Application Support/OpenWhoop/whoop.sqlite"),
            ),
        ]
    }

    /// `NOOP_DB_PATH` wins when set. Otherwise every existing candidate is
    /// checked and the freshest one is used: newest MAX(dailyMetric.day), ties
    /// broken by the newer mtime of the db or its -wal.
    fn resolve(&self, device_id: &str) -> Result<(NoopStoreKind, PathBuf), String> {
        let existing: Vec<_> = self
            .candidates()
            .into_iter()
            .filter(|(_, path)| path.is_file())
            .collect();
        if existing.len() <= 1 {
            return existing.into_iter().next().ok_or_else(|| {
                "NOOP has not created its local health store yet. Open NOOP, pair or import your data, then Amara will read it locally.".to_string()
            });
        }
        Ok(existing
            .into_iter()
            .map(|(kind, path)| {
                let rank = (store_latest_day(&path, device_id), store_mtime(&path));
                (rank, kind, path)
            })
            // max_by_key keeps the last maximum; reverse so a full tie keeps
            // candidate order (container first).
            .rev()
            .max_by_key(|(rank, _, _)| rank.clone())
            .map(|(_, kind, path)| (kind, path))
            .expect("non-empty"))
    }
}

fn expand_home(path: &Path, home: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else {
        path.to_path_buf()
    }
}

/// Newest day with an HRV, recovery or sleep value, from `(day, has_value)` rows. NOOP adds
/// today's row before the night has synced, so its metrics stay null for hours; that row is not a
/// reading. Same rule as `readingDay` in src-console/lib/mindBody.ts, and also None when the
/// reading is over [`STALE_AFTER_DAYS`] older than `today` (the current day, when known).
pub(crate) fn reading_day<'a>(
    rows: impl Iterator<Item = (&'a str, bool)>,
    today: Option<NaiveDate>,
) -> Option<String> {
    let rows: Vec<(NaiveDate, bool)> = rows
        .filter_map(|(day, has_value)| {
            let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
            Some((date, has_value))
        })
        .collect();
    let newest_row = rows.iter().map(|row| row.0).max()?;
    let newest_value = rows.iter().filter(|row| row.1).map(|row| row.0).max()?;
    let reference = today.map_or(newest_row, |day| day.max(newest_row));
    ((reference - newest_value).num_days() <= STALE_AFTER_DAYS).then(|| newest_value.to_string())
}

fn now_iso(now: DateTime<Utc>) -> String {
    now.to_rfc3339()
}

fn unavailable_snapshot(
    device_id: String,
    now: DateTime<Utc>,
    message: String,
) -> NoopHealthSnapshot {
    NoopHealthSnapshot {
        source: "noop".to_string(),
        status: "unavailable".to_string(),
        generated_at: now_iso(now),
        device_id,
        latest_day: None,
        reading_day: None,
        data_age_days: None,
        today: None,
        recent_days: Vec::new(),
        latest_sleep: None,
        latest_night: None,
        battery: None,
        insights: Vec::new(),
        message: Some(message),
    }
}

fn table_names(conn: &Connection) -> Result<HashSet<String>, String> {
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .map_err(|error| format!("NOOP health store could not be inspected: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("NOOP health store could not be inspected: {error}"))?;
    rows.collect::<Result<HashSet<_>, _>>()
        .map_err(|error| format!("NOOP health store could not be inspected: {error}"))
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let sql = format!("PRAGMA table_info({table})");
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| format!("NOOP schema could not be inspected: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("NOOP schema could not be inspected: {error}"))?;
    for name in rows {
        if name.map_err(|error| format!("NOOP schema could not be inspected: {error}"))? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_daily_metrics(conn: &Connection, device_id: &str) -> Result<Vec<NoopDailyMetric>, String> {
    let sources = [format!("{device_id}-noop"), device_id.to_string()];
    let mut merged = BTreeMap::new();
    for source in sources {
        let mut statement = conn
            .prepare(
                "SELECT day, totalSleepMin, efficiency, deepMin, remMin, lightMin, restingHr, avgHrv,
                        recovery, strain, spo2Pct, skinTempDevC, respRateBpm
                 FROM dailyMetric
                 WHERE deviceId = ?1
                 ORDER BY day DESC
                 LIMIT 45",
            )
            .map_err(|error| format!("NOOP daily metrics could not be read: {error}"))?;
        let rows = statement
            .query_map(params![source.clone()], |row| {
                Ok(NoopDailyMetric {
                    day: row.get(0)?,
                    source: source.clone(),
                    total_sleep_min: row.get(1)?,
                    efficiency: row.get(2)?,
                    deep_min: row.get(3)?,
                    rem_min: row.get(4)?,
                    light_min: row.get(5)?,
                    resting_hr: row.get(6)?,
                    avg_hrv: row.get(7)?,
                    recovery: row.get(8)?,
                    strain: row.get(9)?,
                    spo2_pct: row.get(10)?,
                    skin_temp_dev_c: row.get(11)?,
                    resp_rate_bpm: row.get(12)?,
                })
            })
            .map_err(|error| format!("NOOP daily metrics could not be read: {error}"))?;
        for row in rows {
            let metric =
                row.map_err(|error| format!("NOOP daily metrics could not be read: {error}"))?;
            // Imported data takes precedence over NOOP-computed fallbacks, matching NOOP Local Access,
            // but only field by field: an imported NULL must not hide a computed value.
            match merged.remove(&metric.day) {
                Some(fallback) => {
                    let combined = merge_daily_metric(fallback, metric);
                    merged.insert(combined.day.clone(), combined)
                }
                None => merged.insert(metric.day.clone(), metric),
            };
        }
    }
    Ok(merged.into_values().collect())
}

/// Combine two rows for the same day. Every `Some` field of `preferred` wins;
/// `None` fields fall back to `fallback`. The source label follows `preferred`.
fn merge_daily_metric(fallback: NoopDailyMetric, preferred: NoopDailyMetric) -> NoopDailyMetric {
    NoopDailyMetric {
        day: preferred.day,
        source: preferred.source,
        total_sleep_min: preferred.total_sleep_min.or(fallback.total_sleep_min),
        efficiency: preferred.efficiency.or(fallback.efficiency),
        deep_min: preferred.deep_min.or(fallback.deep_min),
        rem_min: preferred.rem_min.or(fallback.rem_min),
        light_min: preferred.light_min.or(fallback.light_min),
        resting_hr: preferred.resting_hr.or(fallback.resting_hr),
        avg_hrv: preferred.avg_hrv.or(fallback.avg_hrv),
        recovery: preferred.recovery.or(fallback.recovery),
        strain: preferred.strain.or(fallback.strain),
        spo2_pct: preferred.spo2_pct.or(fallback.spo2_pct),
        skin_temp_dev_c: preferred.skin_temp_dev_c.or(fallback.skin_temp_dev_c),
        resp_rate_bpm: preferred.resp_rate_bpm.or(fallback.resp_rate_bpm),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SleepSelection {
    Latest,
    Overnight,
}

fn read_sleep(
    conn: &Connection,
    device_id: &str,
    stages_column_present: bool,
    selection: SleepSelection,
) -> Result<Option<NoopSleepSession>, String> {
    #[derive(Debug)]
    struct SleepRow {
        start: i64,
        end: i64,
        efficiency: Option<f64>,
        resting_hr: Option<i64>,
        avg_hrv: Option<f64>,
        stages_json: Option<String>,
    }

    let sources = [device_id.to_string(), format!("{device_id}-noop")];
    let mut selected_source = None;
    let mut latest_end = i64::MIN;
    for source in &sources {
        let end = conn
            .query_row(
                "SELECT MAX(endTs) FROM sleepSession WHERE deviceId = ?1",
                params![source],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|error| format!("NOOP sleep sessions could not be read: {error}"))?;
        if end.is_some_and(|value| value > latest_end) {
            latest_end = end.unwrap_or_default();
            selected_source = Some(source);
        }
    }
    let Some(source) = selected_source else {
        return Ok(None);
    };

    let stages = if stages_column_present {
        "stagesJSON"
    } else {
        "NULL"
    };
    let sql = format!(
        "SELECT startTs, endTs, efficiency, restingHr, avgHrv, {stages}
         FROM sleepSession
         WHERE deviceId = ?1
         ORDER BY endTs DESC
         LIMIT 16"
    );
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| format!("NOOP sleep sessions could not be read: {error}"))?;
    let rows = statement
        .query_map(params![source], |row| {
            Ok(SleepRow {
                start: row.get(0)?,
                end: row.get(1)?,
                efficiency: row.get(2)?,
                resting_hr: row.get(3)?,
                avg_hrv: row.get(4)?,
                stages_json: row.get(5)?,
            })
        })
        .map_err(|error| format!("NOOP sleep sessions could not be read: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("NOOP sleep sessions could not be read: {error}"))?;
    let mut index = 0;
    while index < rows.len() {
        let latest = &rows[index];
        let mut group = vec![latest];
        let mut earliest_start = latest.start;
        for candidate in rows.iter().skip(index + 1) {
            let gap = earliest_start - candidate.end;
            if !(0..60 * 60).contains(&gap) {
                break;
            }
            group.push(candidate);
            earliest_start = candidate.start.min(earliest_start);
        }

        let started_at = DateTime::<Utc>::from_timestamp(earliest_start, 0)
            .unwrap_or_else(Utc::now)
            .with_timezone(&Local);
        let ended_at = DateTime::<Utc>::from_timestamp(latest.end, 0)
            .unwrap_or_else(Utc::now)
            .with_timezone(&Local);
        let is_overnight =
            started_at.hour() >= 18 || (started_at.hour() < 4 && ended_at.hour() <= 12);
        if selection == SleepSelection::Latest || is_overnight {
            let stage_totals = group
                .iter()
                .filter_map(|row| decode_sleep_stage_minutes(row.stages_json.as_deref()))
                .fold(None, |total: Option<SleepStageMinutes>, stages| {
                    let mut combined = total.unwrap_or_default();
                    combined.add(stages);
                    Some(combined)
                });
            let duration_min = stage_totals
                .map(SleepStageMinutes::asleep)
                .unwrap_or_else(|| {
                    group
                        .iter()
                        .map(|row| (row.end - row.start).max(0) as f64 / 60.0)
                        .sum()
                });
            let efficiency = stage_totals
                .and_then(|stages| {
                    (stages.in_bed() > 0.0).then_some(stages.asleep() / stages.in_bed())
                })
                .or(latest.efficiency);

            return Ok(Some(NoopSleepSession {
                day: ended_at.date_naive().format("%Y-%m-%d").to_string(),
                started_at: timestamp_iso(earliest_start),
                ended_at: timestamp_iso(latest.end),
                duration_min,
                efficiency,
                deep_min: stage_totals.map(|stages| stages.deep),
                rem_min: stage_totals.map(|stages| stages.rem),
                light_min: stage_totals.map(|stages| stages.light),
                awake_min: stage_totals.map(|stages| stages.awake),
                session_count: group.len(),
                resting_hr: latest.resting_hr,
                avg_hrv: latest.avg_hrv,
            }));
        }
        index += group.len();
    }
    Ok(None)
}

fn read_latest_sleep(
    conn: &Connection,
    device_id: &str,
    stages_column_present: bool,
) -> Result<Option<NoopSleepSession>, String> {
    read_sleep(
        conn,
        device_id,
        stages_column_present,
        SleepSelection::Latest,
    )
}

fn read_latest_night(
    conn: &Connection,
    device_id: &str,
    stages_column_present: bool,
) -> Result<Option<NoopSleepSession>, String> {
    read_sleep(
        conn,
        device_id,
        stages_column_present,
        SleepSelection::Overnight,
    )
}

fn timestamp_iso(timestamp: i64) -> String {
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .unwrap_or_else(Utc::now)
        .to_rfc3339()
}

fn health_delivery_state_path(home: &Path) -> PathBuf {
    home.join(".hermes/health/noop_delivery.json")
}

fn battery_record_key(timestamp: i64, state: &NoopBatteryState) -> String {
    let charging = if state.charging == Some(true) {
        "charging"
    } else {
        "not_charging"
    };
    format!("{timestamp}:{:.1}:{charging}", state.percentage)
}

fn battery_is_locally_confirmed(
    confirmation_path: Option<&Path>,
    timestamp: i64,
    state: &NoopBatteryState,
    now: DateTime<Utc>,
) -> bool {
    let Some(path) = confirmation_path else {
        return false;
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(delivery_state) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return false;
    };
    let Some(probe) = delivery_state
        .get("battery_clock_probe")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    let record_key = battery_record_key(timestamp, state);
    if probe.get("record_key").and_then(serde_json::Value::as_str) != Some(record_key.as_str()) {
        return false;
    }
    let Some(confirmed_at) = probe
        .get("confirmed_at")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
    else {
        return false;
    };
    let confirmation_age = (now - confirmed_at.with_timezone(&Utc)).num_seconds();
    (0..=BATTERY_MAX_LOCAL_CONFIRMATION_AGE_SECONDS).contains(&confirmation_age)
}

fn battery_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, NoopBatteryState)> {
    let timestamp: i64 = row.get(0)?;
    let charging: Option<i64> = row.get(2)?;
    Ok((
        timestamp,
        NoopBatteryState {
            percentage: row.get(1)?,
            charging: charging.map(|value| value != 0),
            observed_at: timestamp_iso(timestamp),
        },
    ))
}

fn read_latest_battery(
    conn: &Connection,
    device_id: &str,
    charging_column_present: bool,
    confirmation_path: Option<&Path>,
    now: DateTime<Utc>,
) -> Result<Option<NoopBatteryState>, String> {
    let charging = if charging_column_present {
        "charging"
    } else {
        "NULL"
    };
    let quality_order = if charging_column_present {
        "CASE WHEN charging IS NOT NULL THEN 0 ELSE 1 END,"
    } else {
        ""
    };
    let oldest_allowed = now.timestamp() - BATTERY_MAX_AGE_SECONDS;
    let newest_allowed = now.timestamp() + BATTERY_MAX_FUTURE_SKEW_SECONDS;
    let current_sql = format!(
        "SELECT ts, soc, {charging} FROM battery
         WHERE deviceId = ?1 AND soc IS NOT NULL AND ts BETWEEN ?2 AND ?3
         ORDER BY {quality_order} ts DESC
         LIMIT 1"
    );
    let current = conn
        .query_row(
            &current_sql,
            params![device_id, oldest_allowed, newest_allowed],
            battery_row,
        )
        .optional()
        .map_err(|error| format!("NOOP battery state could not be read: {error}"))?;
    if let Some((_, state)) = current {
        return Ok(Some(state));
    }

    let fallback_sql = format!(
        "SELECT ts, soc, {charging} FROM battery
         WHERE deviceId = ?1 AND soc IS NOT NULL
         ORDER BY rowid DESC
         LIMIT 1"
    );
    let fallback = conn
        .query_row(&fallback_sql, params![device_id], battery_row)
        .optional()
        .map_err(|error| format!("NOOP battery fallback could not be read: {error}"))?;

    Ok(fallback.and_then(|(timestamp, state)| {
        (timestamp > newest_allowed
            && battery_is_locally_confirmed(confirmation_path, timestamp, &state, now))
        .then_some(state)
    }))
}

fn has_nightly_data(
    today: Option<&NoopDailyMetric>,
    latest_night: Option<&NoopSleepSession>,
) -> bool {
    today
        .and_then(|metric| metric.total_sleep_min)
        .is_some_and(|minutes| minutes > 0.0)
        || latest_night.is_some_and(|sleep| sleep.duration_min > 0.0)
}

fn baseline(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let values = values.flatten().collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn direction(delta: f64) -> &'static str {
    if delta > 0.5 {
        "above"
    } else if delta < -0.5 {
        "below"
    } else {
        "near"
    }
}

fn baseline_context(delta: f64, noun: &str, unit: &str) -> String {
    if direction(delta) == "near" {
        format!("That is near your recent recorded {noun}.")
    } else {
        let suffix = if unit.is_empty() {
            String::new()
        } else {
            format!(" {unit}")
        };
        format!(
            "That is {} your recent recorded {noun} by {}{suffix}.",
            direction(delta),
            delta.abs().round() as i64
        )
    }
}

fn health_insights(today: &NoopDailyMetric, prior: &[NoopDailyMetric]) -> Vec<NoopHealthInsight> {
    let window = prior
        .iter()
        .rev()
        .take(BASELINE_DAY_LIMIT)
        .collect::<Vec<_>>();
    let mut insights = Vec::new();
    if let Some(minutes) = today.total_sleep_min {
        insights.push(NoopHealthInsight {
            metric: "sleep".to_string(),
            fact: format!(
                "Sleep: {}h {:02}m.",
                (minutes / 60.0).floor() as i64,
                (minutes as i64).rem_euclid(60)
            ),
            context: baseline(window.iter().map(|day| day.total_sleep_min)).map(|average| {
                let delta = minutes - average;
                baseline_context(delta, "average", "minutes")
            }),
        });
    }
    if let Some(recovery) = today.recovery {
        insights.push(NoopHealthInsight {
            metric: "recovery".to_string(),
            fact: format!("Recovery: {}%.", recovery.round() as i64),
            context: baseline(window.iter().map(|day| day.recovery)).map(|average| {
                let delta = recovery - average;
                baseline_context(delta, "baseline", "points")
            }),
        });
    }
    if let Some(hrv) = today.avg_hrv {
        insights.push(NoopHealthInsight {
            metric: "hrv".to_string(),
            fact: format!("HRV: {}.", hrv.round() as i64),
            context: baseline(window.iter().map(|day| day.avg_hrv)).map(|average| {
                let delta = hrv - average;
                baseline_context(delta, "baseline", "ms")
            }),
        });
    }
    if let Some(rhr) = today.resting_hr {
        insights.push(NoopHealthInsight {
            metric: "resting_hr".to_string(),
            fact: format!("Resting heart rate: {rhr} bpm."),
            context: baseline(
                window
                    .iter()
                    .map(|day| day.resting_hr.map(|value| value as f64)),
            )
            .map(|average| {
                let delta = rhr as f64 - average;
                baseline_context(delta, "baseline", "bpm")
            }),
        });
    }
    insights
}

fn sleep_session_insights(
    sleep: &NoopSleepSession,
    prior: &[NoopDailyMetric],
) -> Vec<NoopHealthInsight> {
    let window = prior
        .iter()
        .rev()
        .take(BASELINE_DAY_LIMIT)
        .collect::<Vec<_>>();
    let mut insights = vec![NoopHealthInsight {
        metric: "sleep".to_string(),
        fact: format!(
            "Last night's sleep: {}.",
            duration_label(sleep.duration_min)
        ),
        context: baseline(window.iter().map(|day| day.total_sleep_min)).map(|average| {
            let delta = sleep.duration_min - average;
            baseline_context(delta, "average", "minutes")
        }),
    }];
    if let Some(hrv) = sleep.avg_hrv {
        insights.push(NoopHealthInsight {
            metric: "hrv".to_string(),
            fact: format!("Overnight HRV: {} ms.", hrv.round() as i64),
            context: baseline(window.iter().map(|day| day.avg_hrv)).map(|average| {
                let delta = hrv - average;
                baseline_context(delta, "baseline", "ms")
            }),
        });
    }
    if let Some(rhr) = sleep.resting_hr {
        insights.push(NoopHealthInsight {
            metric: "resting_hr".to_string(),
            fact: format!("Overnight resting heart rate: {rhr} bpm."),
            context: baseline(
                window
                    .iter()
                    .map(|day| day.resting_hr.map(|value| value as f64)),
            )
            .map(|average| {
                let delta = rhr as f64 - average;
                baseline_context(delta, "baseline", "bpm")
            }),
        });
    }
    insights
}

fn snapshot_from_database(
    database_path: &Path,
    device_id: String,
    battery_confirmation_path: Option<&Path>,
    now: DateTime<Utc>,
) -> Result<NoopHealthSnapshot, String> {
    let conn = open_store(database_path)
        .map_err(|error| format!("NOOP health store is unavailable: {error}"))?;
    let tables = table_names(&conn)?;
    if !tables.contains("grdb_migrations") {
        return Err("The NOOP database is missing its migration metadata. Open NOOP to repair it before Amara reads it.".to_string());
    }
    if !tables.contains("dailyMetric") {
        return Err("NOOP has not created daily health metrics yet. Let NOOP finish a sync or import your history first.".to_string());
    }

    let all_days = read_daily_metrics(&conn, &device_id)?;
    let recent_days = all_days
        .iter()
        .rev()
        .take(RECENT_DAY_LIMIT)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let today = all_days.last().cloned();
    let latest_day = today.as_ref().map(|metric| metric.day.clone());
    let reading_day = reading_day(
        all_days.iter().map(|metric| {
            let has_value = metric.avg_hrv.is_some()
                || metric.recovery.is_some()
                || metric.total_sleep_min.is_some();
            (metric.day.as_str(), has_value)
        }),
        Some(now.date_naive()),
    );
    let data_age_days = latest_day.as_deref().and_then(|day| {
        NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .ok()
            .map(|date| (now.date_naive() - date).num_days().max(0))
    });
    let stages_column_present =
        tables.contains("sleepSession") && has_column(&conn, "sleepSession", "stagesJSON")?;
    let latest_sleep = if tables.contains("sleepSession") {
        read_latest_sleep(&conn, &device_id, stages_column_present)?
    } else {
        None
    };
    let latest_night = if tables.contains("sleepSession") {
        read_latest_night(&conn, &device_id, stages_column_present)?
    } else {
        None
    };
    let status = if today.is_none() {
        "waiting_for_data"
    } else if data_age_days.unwrap_or(STALE_AFTER_DAYS + 1) > STALE_AFTER_DAYS {
        "stale"
    } else if !has_nightly_data(today.as_ref(), latest_night.as_ref()) {
        "waiting_for_nightly_data"
    } else {
        "ready"
    };
    let battery = if tables.contains("battery") {
        read_latest_battery(
            &conn,
            &device_id,
            has_column(&conn, "battery", "charging")?,
            battery_confirmation_path,
            now,
        )?
    } else {
        None
    };
    let insights = latest_night
        .as_ref()
        .map(|night| {
            if let Some(index) = all_days.iter().position(|metric| metric.day == night.day) {
                health_insights(&all_days[index], &all_days[..index])
            } else {
                sleep_session_insights(night, &all_days)
            }
        })
        .or_else(|| {
            today.as_ref().map(|metric| {
                health_insights(metric, &all_days[..all_days.len().saturating_sub(1)])
            })
        })
        .unwrap_or_default();
    let message = match status {
        "ready" => None,
        "stale" => Some(
            "NOOP health data is more than 2 days old, so Amara will not present it as current."
                .to_string(),
        ),
        "waiting_for_nightly_data" => Some(
            "NOOP is connected, but its latest daily row has not synchronized an overnight sleep record yet. Amara will wait rather than infer a recovery summary."
                .to_string(),
        ),
        _ => Some("NOOP is connected, but it has not recorded daily health data yet.".to_string()),
    };
    Ok(NoopHealthSnapshot {
        source: "noop".to_string(),
        status: status.to_string(),
        generated_at: now_iso(now),
        device_id,
        latest_day,
        reading_day,
        data_age_days,
        today,
        recent_days,
        latest_sleep,
        latest_night,
        battery,
        insights,
        message,
    })
}

fn device_id_from_environment() -> String {
    env::var("NOOP_DEVICE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_DEVICE_ID.to_string())
}

/// The single NOOP store resolver shared by Home (direct read-only SQLite) and
/// Mind-Body (`noop_query` CLI): `NOOP_DB_PATH` wins, else the freshest of the
/// official container / Application Support stores.
pub(crate) fn resolve_store_path() -> Result<PathBuf, String> {
    NoopPaths::from_environment()
        .resolve(&device_id_from_environment())
        .map(|(_, path)| path)
}

pub fn health_snapshot() -> NoopHealthSnapshot {
    let now = Utc::now();
    let device_id = device_id_from_environment();
    let paths = NoopPaths::from_environment();
    let path = match paths.resolve(&device_id) {
        Ok((_, path)) => path,
        Err(error) => return unavailable_snapshot(device_id, now, error),
    };
    let battery_confirmation_path = health_delivery_state_path(&paths.home);
    match snapshot_from_database(
        &path,
        device_id.clone(),
        Some(&battery_confirmation_path),
        now,
    ) {
        Ok(snapshot) => snapshot,
        Err(error) => unavailable_snapshot(device_id, now, error),
    }
}

fn duration_label(minutes: f64) -> String {
    let total = minutes.round().max(0.0) as i64;
    format!("{}h {:02}m", total / 60, total % 60)
}

fn runtime_health_brief_from_snapshot(snapshot: &NoopHealthSnapshot, limit: usize) -> String {
    if matches!(
        snapshot.status.as_str(),
        "unavailable" | "waiting_for_data" | "waiting_for_nightly_data"
    ) {
        return String::new();
    }
    let mut lines = vec![format!(
        "NOOP health context (read-only, source-authoritative; status {}, latest daily data {}).",
        snapshot.status,
        snapshot
            .reading_day
            .as_deref()
            .unwrap_or("none in the last 2 days")
    )];
    if let Some(sleep) = snapshot
        .latest_night
        .as_ref()
        .or(snapshot.latest_sleep.as_ref())
    {
        let efficiency = sleep
            .efficiency
            .map(|value| format!(", {}% efficiency", (value * 100.0).round() as i64))
            .unwrap_or_default();
        lines.push(format!(
            "Latest NOOP overnight sleep: {} asleep across {} session{}, from {} to {}{}.",
            duration_label(sleep.duration_min),
            sleep.session_count,
            if sleep.session_count == 1 { "" } else { "s" },
            sleep.started_at,
            sleep.ended_at,
            efficiency
        ));
        let mut signals = Vec::new();
        if let Some(hrv) = sleep.avg_hrv {
            signals.push(format!("HRV {} ms", hrv.round() as i64));
        }
        if let Some(resting_hr) = sleep.resting_hr {
            signals.push(format!("resting heart rate {resting_hr} bpm"));
        }
        if !signals.is_empty() {
            lines.push(format!(
                "Latest recorded sleep-segment signals: {}.",
                signals.join(", ")
            ));
        }
        let mut stages = Vec::new();
        for (label, minutes) in [
            ("light", sleep.light_min),
            ("deep", sleep.deep_min),
            ("REM", sleep.rem_min),
            ("awake", sleep.awake_min),
        ] {
            if let Some(minutes) = minutes.filter(|value| *value > 0.0) {
                stages.push(format!("{label} {}", duration_label(minutes)));
            }
        }
        if !stages.is_empty() {
            lines.push(format!("Merged sleep stages: {}.", stages.join(", ")));
        }
    }
    if let (Some(night), Some(latest)) = (
        snapshot.latest_night.as_ref(),
        snapshot.latest_sleep.as_ref(),
    ) {
        if latest.ended_at != night.ended_at {
            lines.push(format!(
                "Latest additional sleep session: {} asleep, from {} to {}.",
                duration_label(latest.duration_min),
                latest.started_at,
                latest.ended_at
            ));
        }
    }
    for insight in snapshot.insights.iter().take(4) {
        lines.push(match insight.context.as_deref() {
            Some(context) => format!("{} {context}", insight.fact),
            None => insight.fact.clone(),
        });
    }
    if let Some(battery) = snapshot.battery.as_ref() {
        let charging = battery
            .charging
            .map(|value| if value { "charging" } else { "not charging" })
            .unwrap_or("charging state unknown");
        lines.push(format!(
            "WHOOP battery: {}% ({charging}); observed {}.",
            battery.percentage.round() as i64,
            battery.observed_at
        ));
    }
    let text = lines.join("\n");
    text.chars().take(limit).collect()
}

pub fn runtime_health_brief(limit: usize) -> String {
    runtime_health_brief_from_snapshot(&health_snapshot(), limit)
}

#[tauri::command]
pub fn get_noop_health_snapshot() -> NoopHealthSnapshot {
    health_snapshot()
}

/// A reading older than this isn't "live": the strap is off or NOOP has stopped syncing.
const LIVE_HR_MAX_AGE_SECS: i64 = 120;
/// Allows for a small clock difference between the strap and the Mac.
const LIVE_HR_MAX_FUTURE_SECS: i64 = 60;
const LIVE_HR_MIN_BPM: i64 = 30;
const LIVE_HR_MAX_BPM: i64 = 240;

/// The trace on the Home screen covers 89 seconds. Sending 144 seconds lets it play through a
/// late batch from NOOP without its left edge running out.
const LIVE_HR_TRACE_WINDOW_SECS: i64 = 144;
/// Most rows read for the trace. At about one sample a second this is one full window; a
/// sparser strap fills less of it and the seconds window is what bounds the trace.
const LIVE_HR_TRACE_ROWS: i64 = 144;

/// One heart rate sample. Never log these values.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeartSample {
    pub bpm: u16,
    pub at: i64,
}

/// The live heart rate for the Home screen, oldest sample first. Never log this.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LiveHeartRate {
    pub bpm: u16,
    pub at: i64,
    pub trace: Vec<HeartSample>,
}

fn plausible_bpm(bpm: i64) -> bool {
    (LIVE_HR_MIN_BPM..=LIVE_HR_MAX_BPM).contains(&bpm)
}

/// Uses the (deviceId, ts) primary key, so this stays cheap on a very long table. Future-dated rows
/// are skipped, because NOOP stores do hold some and one would otherwise hide the line for good.
/// A stale or off-wrist newest sample means None and the screen hides the line. Older off-wrist
/// samples are dropped from the trace.
fn latest_heart_rate(
    path: &Path,
    device_id: &str,
    now: DateTime<Utc>,
) -> rusqlite::Result<Option<LiveHeartRate>> {
    let conn = open_store(path)?;
    let now = now.timestamp();
    let mut statement = conn.prepare(
        "SELECT ts, bpm FROM hrSample WHERE deviceId = ?1 AND ts <= ?2 ORDER BY ts DESC LIMIT ?3",
    )?;
    let rows = statement
        .query_map(
            params![device_id, now + LIVE_HR_MAX_FUTURE_SECS, LIVE_HR_TRACE_ROWS],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let Some(&(at, bpm)) = rows.first() else {
        return Ok(None);
    };
    if now - at > LIVE_HR_MAX_AGE_SECS || !plausible_bpm(bpm) {
        return Ok(None);
    }
    let oldest = at - LIVE_HR_TRACE_WINDOW_SECS;
    let trace = rows
        .iter()
        .rev()
        .filter(|(ts, sample)| *ts >= oldest && plausible_bpm(*sample))
        .map(|&(at, bpm)| HeartSample {
            bpm: bpm as u16,
            at,
        })
        .collect();
    Ok(Some(LiveHeartRate {
        bpm: bpm as u16,
        at,
        trace,
    }))
}

/// Runs off the main thread: the screen polls this, and NOOP may be holding the store for a moment.
#[tauri::command]
pub async fn live_heart_rate() -> Option<LiveHeartRate> {
    tauri::async_runtime::spawn_blocking(|| {
        let device_id = device_id_from_environment();
        let (_, path) = NoopPaths::from_environment().resolve(&device_id).ok()?;
        latest_heart_rate(&path, &device_id, Utc::now()).ok()?
    })
    .await
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::fs;
    use uuid::Uuid;

    fn heart_rate_database(rows: &[(&str, i64, i64)]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("terrence-hr-{}.sqlite", Uuid::new_v4()));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE hrSample (deviceId TEXT NOT NULL, ts INTEGER NOT NULL, bpm INTEGER NOT NULL,
             synced INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (deviceId, ts));",
        )
        .unwrap();
        for (device, ts, bpm) in rows {
            conn.execute(
                "INSERT INTO hrSample (deviceId, ts, bpm) VALUES (?1, ?2, ?3)",
                params![device, ts, bpm],
            )
            .unwrap();
        }
        path
    }

    #[test]
    fn live_heart_rate_takes_the_newest_sample_for_this_device() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let at = now.timestamp();
        let path = heart_rate_database(&[
            ("my-whoop", at - 30, 61),
            ("my-whoop", at - 10, 64),
            ("other", at - 1, 99),
        ]);
        let reading = latest_heart_rate(&path, "my-whoop", now).unwrap().unwrap();
        assert_eq!((reading.bpm, reading.at), (64, at - 10));
        let expected: Vec<_> = reading.trace.iter().map(|s| s.bpm).collect();
        assert_eq!(expected, vec![61, 64]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn live_heart_rate_hides_stale_future_and_implausible_samples() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let at = now.timestamp();
        for (ts, bpm) in [
            (at - LIVE_HR_MAX_AGE_SECS - 1, 64),
            (at + LIVE_HR_MAX_FUTURE_SECS + 1, 64),
            (at - 5, 0),
            (at - 5, 300),
        ] {
            let path = heart_rate_database(&[("my-whoop", ts, bpm)]);
            assert_eq!(latest_heart_rate(&path, "my-whoop", now).unwrap(), None);
            let _ = fs::remove_file(path);
        }
    }

    #[test]
    fn live_heart_rate_skips_a_future_dated_row() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let at = now.timestamp();
        let path = heart_rate_database(&[
            ("my-whoop", at - 5, 66),
            ("my-whoop", at + 1_000_000_000, 70),
        ]);
        let reading = latest_heart_rate(&path, "my-whoop", now).unwrap();
        assert_eq!(reading.map(|r| r.bpm), Some(66));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn live_heart_rate_trace_is_oldest_first_bounded_and_skips_off_wrist_samples() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let at = now.timestamp();
        let mut rows: Vec<(&str, i64, i64)> = (0..400).map(|i| ("my-whoop", at - i, 70)).collect();
        rows[3].2 = 0;
        rows.push(("my-whoop", at - 3_000, 90));
        let path = heart_rate_database(&rows);
        let trace = latest_heart_rate(&path, "my-whoop", now)
            .unwrap()
            .unwrap()
            .trace;
        assert_eq!(trace.len() as i64, LIVE_HR_TRACE_ROWS - 1);
        assert!(trace.windows(2).all(|pair| pair[0].at < pair[1].at));
        assert!(trace.iter().all(|sample| sample.bpm == 70));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn live_heart_rate_trace_window_is_seconds_not_rows() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let at = now.timestamp();
        // One sample every 2 s: the row cap alone would reach 288 s back.
        let rows: Vec<(&str, i64, i64)> = (0..200).map(|i| ("my-whoop", at - 2 * i, 70)).collect();
        let path = heart_rate_database(&rows);
        let trace = latest_heart_rate(&path, "my-whoop", now)
            .unwrap()
            .unwrap()
            .trace;
        let span = trace.last().unwrap().at - trace.first().unwrap().at;
        assert!(span <= LIVE_HR_TRACE_WINDOW_SECS, "span {span}s");
        assert_eq!(trace.len() as i64, LIVE_HR_TRACE_WINDOW_SECS / 2 + 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn live_heart_rate_is_none_when_there_are_no_samples() {
        let now = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let path = heart_rate_database(&[]);
        assert_eq!(latest_heart_rate(&path, "my-whoop", now).unwrap(), None);
        let _ = fs::remove_file(path);
    }

    fn seeded_database() -> PathBuf {
        let path = std::env::temp_dir().join(format!("terrence-noop-{}.sqlite", Uuid::new_v4()));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE grdb_migrations(identifier TEXT);
            CREATE TABLE dailyMetric (
                deviceId TEXT NOT NULL, day TEXT NOT NULL, totalSleepMin REAL, efficiency REAL,
                deepMin REAL, remMin REAL, lightMin REAL, restingHr INTEGER, avgHrv REAL,
                recovery REAL, strain REAL, spo2Pct REAL, skinTempDevC REAL, respRateBpm REAL
            );
            CREATE TABLE sleepSession (
                deviceId TEXT NOT NULL, startTs INTEGER NOT NULL, endTs INTEGER NOT NULL,
                efficiency REAL, restingHr INTEGER, avgHrv REAL, stagesJSON TEXT
            );
            CREATE TABLE battery (deviceId TEXT NOT NULL, ts INTEGER NOT NULL, soc REAL, charging INTEGER);
            INSERT INTO dailyMetric VALUES
              ('my-whoop-noop', '2026-08-24', 410, .89, 80, 95, 235, 56, 54, 70, 9, 97, .1, 14.2),
              ('my-whoop',      '2026-08-24', 430, .91, 85, 100, 245, 54, 59, 75, 8, 98, .0, 14.1),
              ('my-whoop',      '2026-08-25', 395, .86, 70, 90, 235, 58, 48, 63, 11, 97, .2, 14.4),
              ('my-whoop',      '2026-08-26', 425, .90, 82, 104, 239, 55, 62, 78, 7, 98, -.1, 14.0);
            INSERT INTO sleepSession VALUES ('my-whoop', 1787670000, 1787695500, .90, 55, 62, NULL);
            INSERT INTO battery VALUES ('my-whoop', 1787700000, 14, 0);
            ",
        )
        .unwrap();
        path
    }

    #[test]
    fn reads_noop_metrics_without_creating_a_parallel_store() {
        let path = seeded_database();
        let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
        let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
        assert_eq!(snapshot.status, "ready");
        assert_eq!(snapshot.source, "noop");
        assert_eq!(snapshot.latest_day.as_deref(), Some("2026-08-26"));
        assert_eq!(snapshot.recent_days.len(), 3);
        assert_eq!(snapshot.recent_days[0].total_sleep_min, Some(430.0));
        assert_eq!(
            snapshot.today.as_ref().and_then(|day| day.recovery),
            Some(78.0)
        );
        assert_eq!(
            snapshot.battery.as_ref().map(|battery| battery.percentage),
            Some(14.0)
        );
        assert_eq!(
            snapshot
                .battery
                .as_ref()
                .and_then(|battery| battery.charging),
            Some(false)
        );
        assert!(snapshot
            .insights
            .iter()
            .any(|insight| insight.metric == "recovery"));
        assert!(path.exists());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn marks_old_noop_data_stale_without_calling_it_current() {
        let path = seeded_database();
        let now = Utc.with_ymd_and_hms(2026, 8, 31, 12, 0, 0).unwrap();
        let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
        assert_eq!(snapshot.status, "stale");
        assert!(snapshot.message.unwrap().contains("more than 2 days old"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn waits_for_a_real_overnight_record_before_marking_noop_ready() {
        let path = seeded_database();
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE dailyMetric SET totalSleepMin = NULL, recovery = NULL, avgHrv = NULL, restingHr = NULL WHERE day = '2026-08-26'",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM sleepSession", []).unwrap();
        drop(conn);

        let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
        let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
        assert_eq!(snapshot.status, "waiting_for_nightly_data");
        assert!(snapshot.message.unwrap().contains("overnight sleep record"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn bridges_adjacent_noop_sleep_segments_for_the_displayed_night() {
        let path = seeded_database();
        let conn = Connection::open(&path).unwrap();
        conn.execute("DELETE FROM sleepSession", []).unwrap();
        conn.execute(
            "INSERT INTO sleepSession VALUES ('my-whoop-noop', 1788611871, 1788627827, .94, 77, 264, ?1)",
            params![r#"[{"start":1788611871,"end":1788626931,"stage":"light"},{"start":1788626931,"end":1788627827,"stage":"wake"}]"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sleepSession VALUES ('my-whoop-noop', 1788628977, 1788651087, .93, 68, 344, ?1)",
            params![r#"[{"start":1788628977,"end":1788640000,"stage":"light"},{"start":1788640000,"end":1788643600,"stage":"deep"},{"start":1788643600,"end":1788649577,"stage":"rem"},{"start":1788649577,"end":1788651087,"stage":"wake"}]"#],
        )
        .unwrap();
        drop(conn);

        let now = Utc.with_ymd_and_hms(2026, 9, 6, 1, 0, 0).unwrap();
        let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
        let brief = runtime_health_brief_from_snapshot(&snapshot, 1_200);
        assert!(brief.contains("9h 54m asleep across 2 sessions"));
        assert!(brief.contains(
            "Latest recorded sleep-segment signals: HRV 344 ms, resting heart rate 68 bpm"
        ));
        assert!(brief
            .contains("Merged sleep stages: light 7h 15m, deep 1h 00m, REM 1h 40m, awake 0h 40m"));
        let sleep = snapshot.latest_sleep.as_ref().unwrap();
        assert_eq!(sleep.started_at, timestamp_iso(1788611871));
        assert_eq!(sleep.ended_at, timestamp_iso(1788651087));
        assert!((sleep.duration_min - 594.33).abs() < 0.01);
        assert_eq!(sleep.session_count, 2);
        assert!((sleep.light_min.unwrap() - 434.72).abs() < 0.01);
        assert_eq!(sleep.deep_min, Some(60.0));
        assert!((sleep.rem_min.unwrap() - 99.62).abs() < 0.01);
        assert!((sleep.awake_min.unwrap() - 40.10).abs() < 0.01);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn keeps_a_later_daytime_nap_separate_from_the_primary_overnight_sleep() {
        let path = seeded_database();
        let conn = Connection::open(&path).unwrap();
        conn.execute("DELETE FROM sleepSession", []).unwrap();
        // Overnight is judged by the Mac's local hour, so the fixture is written in local time.
        let local = |day, hour, minute| {
            Local
                .with_ymd_and_hms(2026, 9, day, hour, minute, 0)
                .unwrap()
                .timestamp()
        };
        let night_start = local(6, 23, 30);
        let night_end = local(7, 7, 30);
        let nap_start = local(7, 13, 30);
        let nap_end = local(7, 17, 0);
        conn.execute(
            "INSERT INTO sleepSession VALUES ('my-whoop-noop', ?1, ?2, .91, 61, 82, NULL)",
            params![night_start, night_end],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sleepSession VALUES ('my-whoop-noop', ?1, ?2, .88, 64, 73, NULL)",
            params![nap_start, nap_end],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO dailyMetric VALUES ('my-whoop-noop', '2026-09-07', 480, .91, 60, 90, 330, 61, 82, NULL, 8, NULL, NULL, 14)",
            [],
        )
        .unwrap();
        drop(conn);

        let now = DateTime::<Utc>::from_timestamp(nap_end + 2 * 60 * 60, 0).unwrap();
        let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
        let latest = snapshot.latest_sleep.as_ref().unwrap();
        let night = snapshot.latest_night.as_ref().unwrap();
        assert_eq!(latest.started_at, timestamp_iso(nap_start));
        assert_eq!(latest.ended_at, timestamp_iso(nap_end));
        assert_eq!(night.started_at, timestamp_iso(night_start));
        assert_eq!(night.ended_at, timestamp_iso(night_end));
        assert_eq!(night.duration_min, 480.0);
        let brief = runtime_health_brief_from_snapshot(&snapshot, 1_200);
        assert!(brief.contains("Latest NOOP overnight sleep: 8h 00m"));
        assert!(brief.contains("Latest additional sleep session: 3h 30m"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn decodes_imported_sleep_stage_minute_dictionary() {
        let stages =
            decode_sleep_stage_minutes(Some(r#"{"awake":22.5,"light":245,"deep":71.5,"rem":96}"#))
                .unwrap();
        assert_eq!(stages.awake, 22.5);
        assert_eq!(stages.light, 245.0);
        assert_eq!(stages.deep, 71.5);
        assert_eq!(stages.rem, 96.0);
        assert_eq!(stages.asleep(), 412.5);
    }

    #[test]
    fn current_battery_outranks_future_dated_history() {
        let path = seeded_database();
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO battery VALUES ('my-whoop', 1800000000, 9, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO battery VALUES ('my-whoop', 1787700300, 69, NULL)",
            [],
        )
        .unwrap();
        drop(conn);

        let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
        let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
        assert_eq!(
            snapshot.battery.as_ref().map(|battery| battery.percentage),
            Some(14.0)
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_a_future_only_noop_battery_timestamp() {
        let path = seeded_database();
        let conn = Connection::open(&path).unwrap();
        conn.execute("DELETE FROM battery", []).unwrap();
        conn.execute(
            "INSERT INTO battery VALUES ('my-whoop', 1800000000, 9, 0)",
            [],
        )
        .unwrap();
        drop(conn);

        let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
        let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
        assert!(snapshot.battery.is_none());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn accepts_a_future_battery_timestamp_after_a_matching_local_confirmation() {
        let path = seeded_database();
        let conn = Connection::open(&path).unwrap();
        conn.execute("DELETE FROM battery", []).unwrap();
        conn.execute(
            "INSERT INTO battery VALUES ('my-whoop', 1800000000, 9, 0)",
            [],
        )
        .unwrap();
        drop(conn);

        let now = Utc.with_ymd_and_hms(2026, 8, 26, 12, 0, 0).unwrap();
        let confirmation_path = std::env::temp_dir().join(format!(
            "terrence-noop-confirmation-{}.json",
            Uuid::new_v4()
        ));
        fs::write(
            &confirmation_path,
            format!(
                r#"{{"battery_clock_probe":{{"record_key":"1800000000:9.0:not_charging","confirmed_at":"{}"}}}}"#,
                now.to_rfc3339()
            ),
        )
        .unwrap();

        let snapshot =
            snapshot_from_database(&path, "my-whoop".to_string(), Some(&confirmation_path), now)
                .unwrap();
        assert_eq!(
            snapshot.battery.as_ref().map(|battery| battery.percentage),
            Some(9.0)
        );
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(confirmation_path);
    }

    /// A fake home with the given stores: (kind, latest day or None for an
    /// empty dailyMetric, or "garbage" for a non-SQLite file).
    fn fake_home(stores: &[(NoopStoreKind, Option<&str>)]) -> NoopPaths {
        let home = std::env::temp_dir().join(format!("nexus-noop-home-{}", Uuid::new_v4()));
        let paths = NoopPaths {
            explicit: None,
            home,
        };
        for (kind, day) in stores {
            let path = paths
                .candidates()
                .into_iter()
                .find(|(k, _)| k == kind)
                .unwrap()
                .1;
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if *day == Some("garbage") {
                fs::write(&path, "not sqlite").unwrap();
                continue;
            }
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE dailyMetric (deviceId TEXT, day TEXT);")
                .unwrap();
            if let Some(day) = day {
                conn.execute(
                    "INSERT INTO dailyMetric VALUES ('my-whoop-noop', ?1)",
                    params![day],
                )
                .unwrap();
            }
        }
        paths
    }

    fn set_mtime(path: &Path, secs_ago: u64) {
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(secs_ago);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    fn path_of(paths: &NoopPaths, kind: NoopStoreKind) -> PathBuf {
        paths
            .candidates()
            .into_iter()
            .find(|(k, _)| *k == kind)
            .unwrap()
            .1
    }

    #[test]
    fn picks_the_store_with_the_newest_day_over_the_container() {
        // The real layout on 2026-10-28: a stale container and a live app-support store.
        let paths = fake_home(&[
            (NoopStoreKind::Container, Some("2026-09-20")),
            (NoopStoreKind::AppSupport, Some("2026-10-28")),
        ]);
        // The stale store being touched more recently must not matter.
        set_mtime(&path_of(&paths, NoopStoreKind::AppSupport), 3600);
        let (kind, path) = paths.resolve("my-whoop").unwrap();
        assert_eq!(kind, NoopStoreKind::AppSupport);
        assert_eq!(path, path_of(&paths, NoopStoreKind::AppSupport));
        let _ = fs::remove_dir_all(&paths.home);
    }

    #[test]
    fn equal_days_fall_back_to_the_newer_db_or_wal_mtime() {
        let paths = fake_home(&[
            (NoopStoreKind::Container, Some("2026-10-28")),
            (NoopStoreKind::AppSupport, Some("2026-10-28")),
        ]);
        let container = path_of(&paths, NoopStoreKind::Container);
        let app = path_of(&paths, NoopStoreKind::AppSupport);
        set_mtime(&container, 7200);
        set_mtime(&app, 3600);
        assert_eq!(
            paths.resolve("my-whoop").unwrap().0,
            NoopStoreKind::AppSupport
        );
        // A fresh -wal makes the container newer even though its main file is older.
        let mut wal = container.as_os_str().to_owned();
        wal.push("-wal");
        fs::write(PathBuf::from(wal), "").unwrap();
        assert_eq!(
            paths.resolve("my-whoop").unwrap().0,
            NoopStoreKind::Container
        );
        let _ = fs::remove_dir_all(&paths.home);
    }

    #[test]
    fn an_unreadable_or_empty_store_loses_to_a_readable_one() {
        let paths = fake_home(&[
            (NoopStoreKind::Container, Some("garbage")),
            (NoopStoreKind::AppSupport, None),
        ]);
        set_mtime(&path_of(&paths, NoopStoreKind::AppSupport), 7200);
        // Both have no day; the newer mtime (the garbage file) wins the tie...
        assert_eq!(
            paths.resolve("my-whoop").unwrap().0,
            NoopStoreKind::Container
        );
        let paths2 = fake_home(&[
            (NoopStoreKind::Container, Some("garbage")),
            (NoopStoreKind::AppSupport, Some("2026-01-01")),
        ]);
        // ...but any real day beats it.
        assert_eq!(
            paths2.resolve("my-whoop").unwrap().0,
            NoopStoreKind::AppSupport
        );
        let _ = fs::remove_dir_all(&paths.home);
        let _ = fs::remove_dir_all(&paths2.home);
    }

    #[test]
    fn explicit_path_overrides_and_staging_is_never_a_candidate() {
        let paths = fake_home(&[
            (NoopStoreKind::Container, Some("2026-09-20")),
            (NoopStoreKind::AppSupport, Some("2026-10-28")),
        ]);
        let staging = paths.home.join(
            "Library/Containers/com.noopapp.noop.staging/Data/Library/Application Support/OpenWhoop/whoop.sqlite",
        );
        assert!(paths
            .candidates()
            .iter()
            .all(|(_, p)| !p.starts_with(staging.parent().unwrap())));
        assert!(paths
            .candidates()
            .iter()
            .all(|(_, p)| !p.to_string_lossy().contains(".staging")));
        let explicit = NoopPaths {
            explicit: Some(path_of(&paths, NoopStoreKind::Container)),
            home: paths.home.clone(),
        };
        let (kind, path) = explicit.resolve("my-whoop").unwrap();
        assert_eq!(kind, NoopStoreKind::Explicit);
        assert_eq!(path, path_of(&paths, NoopStoreKind::Container));
        let _ = fs::remove_dir_all(&paths.home);
    }

    #[test]
    fn reading_day_skips_empty_rows_for_at_most_two_days() {
        let d = |day: &str| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok();
        fn rows<'a>(
            pairs: &'a [(&'static str, bool)],
        ) -> impl Iterator<Item = (&'static str, bool)> + 'a {
            pairs.iter().copied()
        }
        let filled = ("2026-11-03", true);
        let empty = |day| (day, false);

        assert_eq!(
            reading_day(rows(&[filled, empty("2026-11-04")]), d("2026-11-04")).as_deref(),
            Some("2026-11-03")
        );
        // Exactly two days old counts, three does not (mindBody.test.ts readingDay).
        let two = [filled, empty("2026-11-04"), empty("2026-11-05")];
        assert_eq!(
            reading_day(rows(&two), d("2026-11-05")).as_deref(),
            Some("2026-11-03")
        );
        let three = [filled, empty("2026-11-05"), empty("2026-11-06")];
        assert_eq!(reading_day(rows(&three), d("2026-11-06")), None);
        // The real day counts even when no row exists for it yet.
        assert_eq!(reading_day(rows(&[filled]), d("2026-11-06")), None);
        // Unknown current day: the rows alone decide, as in the frontend.
        assert_eq!(
            reading_day(rows(&[filled]), None).as_deref(),
            Some("2026-11-03")
        );
        // A clock behind the rows cannot make an old reading look fresh.
        assert_eq!(reading_day(rows(&three), d("2026-11-01")), None);
        assert_eq!(reading_day(rows(&[empty("2026-11-05")]), None), None);
        assert_eq!(reading_day(rows(&[]), None), None);
        assert_eq!(reading_day(rows(&[("not-a-day", true)]), None), None);
    }

    #[test]
    fn snapshot_and_brief_label_the_reading_day_not_the_empty_row() {
        let path = seeded_database();
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO dailyMetric (deviceId, day) VALUES ('my-whoop', '2026-08-27')",
            [],
        )
        .unwrap();
        drop(conn);
        let now = Utc.with_ymd_and_hms(2026, 8, 27, 12, 0, 0).unwrap();
        let snapshot = snapshot_from_database(&path, "my-whoop".to_string(), None, now).unwrap();
        assert_eq!(snapshot.latest_day.as_deref(), Some("2026-08-27"));
        assert_eq!(snapshot.reading_day.as_deref(), Some("2026-08-26"));
        let brief = runtime_health_brief_from_snapshot(&snapshot, 1_200);
        assert!(brief.contains("latest daily data 2026-08-26"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn data_two_days_old_is_not_stale_but_three_is() {
        let path = seeded_database();
        let two = Utc.with_ymd_and_hms(2026, 8, 28, 12, 0, 0).unwrap();
        let three = Utc.with_ymd_and_hms(2026, 8, 29, 12, 0, 0).unwrap();
        let s2 = snapshot_from_database(&path, "my-whoop".to_string(), None, two).unwrap();
        let s3 = snapshot_from_database(&path, "my-whoop".to_string(), None, three).unwrap();
        assert_ne!(s2.status, "stale");
        assert_eq!(s3.status, "stale");
        let _ = fs::remove_file(path);
    }

    /// Live smoke against the real NOOP store (read-only). Prints only row
    /// counts, latestDay, the chosen candidate kind and elapsed ms — never
    /// health values. Run:
    /// cargo test --manifest-path src-tauri/Cargo.toml noop_live -- --ignored --nocapture
    #[test]
    #[ignore = "reads the real NOOP store"]
    fn noop_live_smoke() {
        let started = std::time::Instant::now();
        let snapshot = health_snapshot();
        let elapsed = started.elapsed().as_millis();
        let device_id =
            env::var("NOOP_DEVICE_ID").unwrap_or_else(|_| DEFAULT_DEVICE_ID.to_string());
        let (kind, path) = NoopPaths::from_environment().resolve(&device_id).unwrap();
        let conn = open_store(&path).unwrap();
        let count = |table: &str| -> i64 {
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .unwrap_or(-1)
        };
        println!(
            "noop_live_smoke: kind={} dailyMetric={} sleepSession={} battery={} recentDays={} latestDay={} status={} elapsed_ms={}",
            kind.label(),
            count("dailyMetric"),
            count("sleepSession"),
            count("battery"),
            snapshot.recent_days.len(),
            snapshot.latest_day.as_deref().unwrap_or("none"),
            snapshot.status,
            elapsed
        );
        let latest =
            NaiveDate::parse_from_str(snapshot.latest_day.as_deref().unwrap(), "%Y-%m-%d").unwrap();
        let age = (Local::now().date_naive() - latest).num_days();
        assert!(age <= 2, "latestDay is {age} days old");
        assert!(elapsed < 500, "snapshot took {elapsed} ms");
    }

    #[test]
    fn rejects_a_foreign_database_without_writing_to_it() {
        let path =
            std::env::temp_dir().join(format!("terrence-noop-foreign-{}.sqlite", Uuid::new_v4()));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE dailyMetric (deviceId TEXT, day TEXT);")
            .unwrap();
        drop(conn);
        let result = snapshot_from_database(&path, "my-whoop".to_string(), None, Utc::now());
        assert!(result.unwrap_err().contains("migration metadata"));
        assert!(path.exists());
        let _ = fs::remove_file(path);
    }
}

/// Failing repro for TEST_PLAN bug #7 (imported row blanks NOOP-computed fields).
#[cfg(test)]
mod repro_tests {
    use super::*;

    #[test]
    fn repro_imported_null_field_does_not_hide_noop_value() {
        let path =
            std::env::temp_dir().join(format!("nexus-noop-merge-{}.sqlite", uuid::Uuid::new_v4()));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE dailyMetric (
                deviceId TEXT NOT NULL, day TEXT NOT NULL, totalSleepMin REAL, efficiency REAL,
                deepMin REAL, remMin REAL, lightMin REAL, restingHr INTEGER, avgHrv REAL,
                recovery REAL, strain REAL, spo2Pct REAL, skinTempDevC REAL, respRateBpm REAL);
             INSERT INTO dailyMetric VALUES
               ('my-whoop-noop','2026-10-20',410,.89,80,95,235,56,54,62,9,97,.1,14.2),
               ('my-whoop',     '2026-10-20',430,.91,85,100,245,54,59,NULL,8,98,.0,14.1);",
        )
        .unwrap();
        let days = read_daily_metrics(&conn, "my-whoop").unwrap();
        drop(conn);
        let _ = std::fs::remove_file(&path);
        assert_eq!(days.len(), 1);
        assert_eq!(
            days[0].total_sleep_min,
            Some(430.0),
            "imported value should win where present"
        );
        assert_eq!(
            days[0].recovery,
            Some(62.0),
            "imported NULL blanked the NOOP-computed recovery"
        );
    }
}
