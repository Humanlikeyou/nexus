//! Home's heart-rate take (docs/design/HANDOFF-1111u.md §3, §4, §8 steps 1-3).
//!
//! Where generation lives: here in Rust, not in the webview. §4 requires the 30-minute gate to be
//! enforced by the backend ("a reload, two windows or rapid focus changes can never make two
//! takes"). The gate, the in-flight lock and the TakeLog append must be one atomic step, and only the
//! backend can own that. So Rust reads the window, calls the local router through the same
//! `chat_with_router` command the UI used (`force_provider: light`, `local_only: true`), validates the
//! text and falls back to the template. The TS helpers in `src-console/hrTake/adapters.ts` stay
//! Design's reference; the Rust ports below are pinned to them by tests.
//!
//! Privacy: only aggregates (avg, min, max, first, last, window times, usual band) reach the model;
//! raw samples never leave the NOOP read. Never log bpm values or take text.

use super::take_log::{self, TakeLogEntry};
use chrono::{DateTime, FixedOffset, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const WINDOW_MIN: u32 = 15;
pub const MIN_COVERED: u32 = 10;
pub const SOURCE_STALE_MS: i64 = 10 * 60_000;
pub const TAKE_GAP_MS: i64 = 30 * 60_000;
const USUAL_DAYS: i64 = 14;
const MIN_USUAL_DAYS: i64 = 7;
const MAX_TAKE_CHARS: usize = 600;
const QUESTION: &str = "What's behind my heart rate right now?";
const ASK_TIMEOUT_SECS: u64 = 45;

/// One lock for the Home page. The second concurrent caller waits, then sees the take the first one
/// wrote and returns it unchanged, so two calls make one take.
static HOME_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct HrMinute {
    pub t: i64,
    pub bpm: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct HrStats {
    pub avg: i64,
    pub min: i64,
    pub max: i64,
    pub first: i64,
    pub last: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct HrUsual {
    pub lo: i64,
    pub hi: i64,
    pub days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HrWindow {
    pub from: i64,
    pub to: i64,
    pub source: String,
    pub last_sample_at: Option<i64>,
    pub per_minute: Vec<HrMinute>,
    pub covered_min: u32,
    pub stats: Option<HrStats>,
    pub usual: Option<HrUsual>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HrTake {
    pub id: String,
    pub generated_at: i64,
    pub window: HrWindow,
    pub text: String,
    pub memory: Option<String>,
    pub question: String,
}

/// Exactly the `HrTakeStatus` union in `src-console/hrTake/adapters.ts`.
#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HrTakeStatus {
    Take {
        take: HrTake,
        due: bool,
    },
    NoneYet {
        due: bool,
    },
    #[serde(rename_all = "camelCase")]
    NotEnough {
        previous: Option<HrTake>,
        window: HrWindow,
        due: bool,
    },
    #[serde(rename_all = "camelCase")]
    SourceDown {
        previous: Option<HrTake>,
        last_sample_at: Option<i64>,
        due: bool,
    },
}

// ---------------------------------------------------------------- window maths

/// Per-minute means over `(to - minutes, to]`; `None` = no samples that minute. `samples` are
/// `(ts seconds, bpm)`.
fn buckets(samples: &[(i64, i64)], to_ms: i64, minutes: u32) -> Vec<Option<i64>> {
    let from = to_ms - i64::from(minutes) * 60_000;
    let mut sum = vec![0i64; minutes as usize];
    let mut count = vec![0i64; minutes as usize];
    for &(ts, bpm) in samples {
        let ms = ts * 1000;
        if ms <= from || ms > to_ms {
            continue;
        }
        let idx = (((ms - from - 1) / 60_000) as usize).min(minutes as usize - 1);
        sum[idx] += bpm;
        count[idx] += 1;
    }
    (0..minutes as usize)
        .map(|i| (count[i] > 0).then(|| (sum[i] as f64 / count[i] as f64).round() as i64))
        .collect()
}

fn stats_of(values: &[i64]) -> Option<HrStats> {
    let (&first, &last) = (values.first()?, values.last()?);
    Some(HrStats {
        avg: (values.iter().sum::<i64>() as f64 / values.len() as f64).round() as i64,
        min: *values.iter().min()?,
        max: *values.iter().max()?,
        first,
        last,
    })
}

fn percentile(sorted: &[i64], p: f64) -> f64 {
    let idx = p * (sorted.len() - 1) as f64;
    let (lo, hi) = (idx.floor() as usize, idx.ceil() as usize);
    sorted[lo] as f64 + (sorted[hi] - sorted[lo]) as f64 * (idx - lo as f64)
}

fn tokyo_time(ms: i64) -> DateTime<FixedOffset> {
    super::shared::time::jst().from_utc_datetime(
        &DateTime::<Utc>::from_timestamp_millis(ms)
            .unwrap_or_default()
            .naive_utc(),
    )
}

/// Same clock window on each of the previous 14 days, waking hours only (06:00-22:59 at the window
/// end). A day counts when it has at least `MIN_COVERED` minutes. 25th-75th percentile of the
/// per-minute means. `None` until 7 such days exist.
fn usual_band(to_ms: i64, minutes: u32, prior: &[Vec<(i64, i64)>]) -> Option<HrUsual> {
    let hour = tokyo_time(to_ms).hour();
    if !(6..=22).contains(&hour) {
        return None;
    }
    let mut pooled: Vec<i64> = Vec::new();
    let mut days: i64 = 0;
    for (i, samples) in prior.iter().enumerate() {
        let day_to = to_ms - (i as i64 + 1) * 86_400_000;
        let b: Vec<i64> = buckets(samples, day_to, minutes)
            .into_iter()
            .flatten()
            .collect();
        if b.len() as u32 >= MIN_COVERED {
            days += 1;
            pooled.extend(b);
        }
    }
    if days < MIN_USUAL_DAYS {
        return None;
    }
    pooled.sort_unstable();
    Some(HrUsual {
        lo: percentile(&pooled, 0.25).round() as i64,
        hi: percentile(&pooled, 0.75).round() as i64,
        days,
    })
}

/// Builds the window from already-read samples. `to_ms` is the newest sample time.
pub(crate) fn build_window(
    current: &[(i64, i64)],
    to_ms: i64,
    minutes: u32,
    last_sample_ms: Option<i64>,
    prior: &[Vec<(i64, i64)>],
) -> HrWindow {
    let from = to_ms - i64::from(minutes) * 60_000;
    let b = buckets(current, to_ms, minutes);
    let per_minute: Vec<HrMinute> = b
        .iter()
        .enumerate()
        .map(|(i, bpm)| HrMinute {
            t: from + i as i64 * 60_000,
            bpm: *bpm,
        })
        .collect();
    let covered: Vec<i64> = b.iter().flatten().copied().collect();
    HrWindow {
        from,
        to: to_ms,
        source: "whoop".into(),
        last_sample_at: last_sample_ms,
        per_minute,
        covered_min: covered.len() as u32,
        stats: stats_of(&covered),
        usual: usual_band(to_ms, minutes, prior),
    }
}

/// Why a take cannot be written from this window, if it cannot. Source down wins over not enough.
pub(crate) fn block_reason(w: &HrWindow, now_ms: i64) -> Option<&'static str> {
    match w.last_sample_at {
        None => Some("source_down"),
        Some(at) if now_ms - at > SOURCE_STALE_MS => Some("source_down"),
        _ if w.covered_min < MIN_COVERED || w.stats.is_none() => Some("not_enough"),
        _ => None,
    }
}

/// Reads the window from a NOOP store. `None` when the store has no plausible sample at all.
pub(crate) fn read_window(
    path: &Path,
    device_id: &str,
    now_ms: i64,
    minutes: u32,
) -> Option<HrWindow> {
    use super::noop::{heart_rate_samples, newest_heart_rate_ts};
    let minutes = minutes.clamp(1, 60);
    let newest = newest_heart_rate_ts(path, device_id, now_ms / 1000 + 60).ok()??;
    let to_ms = newest * 1000;
    let current =
        heart_rate_samples(path, device_id, newest - i64::from(minutes) * 60, newest).ok()?;
    let prior: Vec<Vec<(i64, i64)>> = (1..=USUAL_DAYS)
        .map(|d| {
            let to = newest - d * 86_400;
            heart_rate_samples(path, device_id, to - i64::from(minutes) * 60, to)
                .unwrap_or_default()
        })
        .collect();
    Some(build_window(&current, to_ms, minutes, Some(to_ms), &prior))
}

/// The last `minutes` of heart rate for Home. Read-only. Never log the result.
#[tauri::command]
pub async fn hr_window(minutes: u32) -> Option<HrWindow> {
    tauri::async_runtime::spawn_blocking(move || {
        let (device, path) = super::noop::current_store()?;
        read_window(&path, &device, Utc::now().timestamp_millis(), minutes)
    })
    .await
    .ok()
    .flatten()
}

// ---------------------------------------------------------------- text (ports of hrTake/adapters.ts)

fn trend(s: &HrStats) -> &'static str {
    let d = s.last - s.first;
    if d >= 5 {
        "rising"
    } else if d <= -5 {
        "falling"
    } else {
        "steady"
    }
}

/// Port of `hrTemplate`: what Amara says when the model is unavailable or its text fails the check.
pub(crate) fn template(w: &HrWindow) -> String {
    let Some(s) = w.stats else {
        return String::new();
    };
    let first = match trend(&s) {
        "falling" => format!(
            "Your heart rate eased from **{}** to **{}** over the last 15 minutes, averaging **{}**.",
            s.first, s.last, s.avg
        ),
        "rising" => format!(
            "Your heart rate rose from **{}** to **{}** over the last 15 minutes, averaging **{}**.",
            s.first, s.last, s.avg
        ),
        _ => format!(
            "Your heart rate held steady around **{}** over the last 15 minutes (**{}–{}**).",
            s.avg, s.min, s.max
        ),
    };
    let Some(u) = w.usual else {
        return first;
    };
    let band = format!("your usual for this time of day (**{}–{}**)", u.lo, u.hi);
    let second = if s.avg > u.hi {
        format!("That's above {band}. If you've been sitting, a few slow breaths or a short walk may help.")
    } else if s.avg < u.lo {
        format!("That's below {band}, a quiet stretch.")
    } else {
        format!("That's inside {band}: a calm stretch that may suit focused work.")
    };
    format!("{first} {second}")
}

fn clock(ms: i64) -> String {
    tokyo_time(ms).format("%-I:%M %p").to_string()
}

/// Port of `hrMemoryLine`: a template over the previous stored take, never model recall.
pub(crate) fn memory_line(prev: Option<&HrTake>) -> Option<String> {
    let prev = prev?;
    let s = prev.window.stats?;
    let word = match trend(&s) {
        "rising" => "climbing",
        "falling" => "settling",
        _ => "steady",
    };
    Some(format!(
        "At **{}** you were at **{}** and {word}.",
        clock(prev.generated_at),
        s.last
    ))
}

fn facts(w: &HrWindow) -> Vec<i64> {
    let mut out = vec![i64::from(WINDOW_MIN), i64::from(w.covered_min)];
    if let Some(s) = w.stats {
        out.extend([
            s.avg,
            s.min,
            s.max,
            s.first,
            s.last,
            (s.last - s.first).abs(),
        ]);
    }
    if let Some(u) = w.usual {
        out.extend([u.lo, u.hi]);
    }
    out
}

/// Digit runs in `text`, ignoring clock times like "2:00" or "2:00 PM" (same rule as `hrTextOk`).
pub(crate) fn numbers_in(text: &str) -> Vec<i64> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        let run = i - start;
        let is_time = run <= 2
            && chars.get(i) == Some(&':')
            && chars.get(i + 1).is_some_and(char::is_ascii_digit)
            && chars.get(i + 2).is_some_and(char::is_ascii_digit);
        if is_time {
            i += 3;
            continue;
        }
        let digits: String = chars[start..i].iter().collect();
        if let Ok(n) = digits.parse::<i64>() {
            out.push(n);
        }
    }
    out
}

fn has_banned_word(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("proof that") || lower.contains("because of") {
        return true;
    }
    lower.split(|c: char| !c.is_alphanumeric()).any(|w| {
        ["caus", "guarantee", "diagnos", "arrhythmi"]
            .iter()
            .any(|p| w.starts_with(p))
            || matches!(
                w,
                "prove" | "proves" | "proved" | "proven" | "fixed" | "cure" | "cured"
            )
    })
}

/// True when the reply is short, has no currency, no causal or medical words, every number is a fact
/// and at least one fact is cited. Rejected text is never patched.
pub(crate) fn text_ok(text: &str, w: &HrWindow) -> bool {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_TAKE_CHARS {
        return false;
    }
    if text.chars().any(|c| "¥$€£".contains(c)) || has_banned_word(text) {
        return false;
    }
    let allowed = facts(w);
    let nums = numbers_in(text);
    !nums.is_empty() && nums.iter().all(|n| allowed.contains(n))
}

fn prompt(w: &HrWindow) -> String {
    let mut lines = vec![
        "Write Amara's take for Taisun's Home page from his last 15 minutes of heart rate: exactly 2 plain-text sentences, no markdown.".to_string(),
        "Sentence 1 says what happened, with numbers. Sentence 2 says how that compares with his usual for this time of day (only when a usual is given) and may add one gentle, optional suggestion using \"may\", never \"should\".".to_string(),
        "Use only the facts below. Quote at least one number exactly. Do not invent numbers, causes, advice beyond them, or medical claims.".to_string(),
        "Facts:".to_string(),
    ];
    if let Some(s) = w.stats {
        lines.push(format!(
            "- Window: {} to {} Tokyo time, readings in {} of {} minutes",
            clock(w.from),
            clock(w.to),
            w.covered_min,
            WINDOW_MIN
        ));
        lines.push(format!("- Average: {} bpm", s.avg));
        lines.push(format!("- Lowest: {} bpm, highest: {} bpm", s.min, s.max));
        lines.push(format!(
            "- Started at {} bpm, ended at {} bpm ({})",
            s.first,
            s.last,
            trend(&s)
        ));
    }
    if let Some(u) = w.usual {
        lines.push(format!(
            "- His usual for this time of day (last {} days): {} to {} bpm",
            u.days, u.lo, u.hi
        ));
    }
    lines.join("\n")
}

// ---------------------------------------------------------------- gate

pub(crate) trait HrDeps {
    fn now_ms(&self) -> i64;
    async fn window(&self) -> Option<HrWindow>;
    /// `(reply, came_from_cloud)`; `None` when the model is unavailable.
    async fn ask(&self, prompt: &str) -> Option<(String, bool)>;
}

fn tokyo_day(ms: i64) -> String {
    tokyo_time(ms).format("%Y-%m-%d").to_string()
}

fn to_take(entry: &TakeLogEntry) -> Option<HrTake> {
    #[derive(Deserialize)]
    struct Extra {
        window: HrWindow,
        memory: Option<String>,
        question: String,
    }
    let extra: Extra = serde_json::from_value(entry.hr.clone()?).ok()?;
    Some(HrTake {
        id: entry.id.clone(),
        generated_at: entry.generated_at,
        window: extra.window,
        text: entry.text.clone(),
        memory: extra.memory,
        question: extra.question,
    })
}

fn last_take(dir: &Path) -> Option<HrTake> {
    take_log::list_in(dir)
        .iter()
        .filter(|e| e.page == "home" && e.forgotten_at.is_none())
        .filter_map(to_take)
        .max_by_key(|t| t.generated_at)
}

fn due(prev: Option<&HrTake>, now_ms: i64) -> bool {
    prev.is_none_or(|p| now_ms - p.generated_at >= TAKE_GAP_MS)
}

/// Read only: the newest Home heart-rate take and whether one is due.
pub(crate) fn status_in(dir: &Path, now_ms: i64) -> HrTakeStatus {
    match last_take(dir) {
        Some(take) => {
            let d = due(Some(&take), now_ms);
            HrTakeStatus::Take { take, due: d }
        }
        None => HrTakeStatus::NoneYet { due: true },
    }
}

/// The gate. Under the Home lock: a take younger than 30 minutes is returned unchanged; otherwise the
/// window is read and, unless it is blocked, one take is generated and appended. A blocked result
/// writes nothing, so the clock does not restart.
pub(crate) async fn maybe_refresh_with(deps: &impl HrDeps, dir: &Path) -> HrTakeStatus {
    let _guard = HOME_LOCK.lock().await;
    let prev = last_take(dir);
    let now = deps.now_ms();
    if let Some(p) = prev.as_ref().filter(|p| !due(Some(p), now)) {
        return HrTakeStatus::Take {
            take: p.clone(),
            due: false,
        };
    }
    let Some(window) = deps.window().await else {
        return HrTakeStatus::SourceDown {
            previous: prev,
            last_sample_at: None,
            due: true,
        };
    };
    match block_reason(&window, now) {
        Some("source_down") => {
            return HrTakeStatus::SourceDown {
                previous: prev,
                last_sample_at: window.last_sample_at,
                due: true,
            }
        }
        Some(_) => {
            return HrTakeStatus::NotEnough {
                previous: prev,
                window,
                due: true,
            }
        }
        None => {}
    }
    let (text, tier) = match deps.ask(&prompt(&window)).await {
        Some((reply, false)) if text_ok(&reply, &window) => (reply.trim().to_string(), "light"),
        _ => (template(&window), "template"),
    };
    let memory = memory_line(prev.as_ref());
    let entry = TakeLogEntry {
        id: format!("home-hr-{now}"),
        page: "home".into(),
        day: tokyo_day(now),
        generated_at: now,
        kind: "daily".into(),
        text,
        cited: Vec::new(),
        fingerprint: format!("hr-{}", window.to),
        tier: tier.into(),
        trial_id: None,
        memory_line: memory.clone(),
        forgotten_at: None,
        hr: Some(serde_json::json!({
            "window": window,
            "memory": memory,
            "question": QUESTION,
        })),
    };
    // If the log cannot be written, nothing is claimed: the old take stays and the next return retries.
    if !matches!(take_log::append_in(dir, &entry), Ok(true)) {
        return match prev {
            Some(take) => HrTakeStatus::Take { take, due: true },
            None => HrTakeStatus::NoneYet { due: true },
        };
    }
    match to_take(&entry) {
        Some(take) => HrTakeStatus::Take { take, due: false },
        None => HrTakeStatus::NoneYet { due: true },
    }
}

struct RealDeps;

impl HrDeps for RealDeps {
    fn now_ms(&self) -> i64 {
        Utc::now().timestamp_millis()
    }
    async fn window(&self) -> Option<HrWindow> {
        hr_window(WINDOW_MIN).await
    }
    async fn ask(&self, prompt: &str) -> Option<(String, bool)> {
        use super::llm_router::{chat_with_router, RouterConfig, Tier};
        let call = chat_with_router(
            prompt.to_string(),
            RouterConfig::default(),
            Vec::new(),
            Some("light".into()),
            None,
            None,
            Some(true),
        );
        let resp = tokio::time::timeout(std::time::Duration::from_secs(ASK_TIMEOUT_SECS), call)
            .await
            .ok()?
            .ok()?;
        Some((resp.content, matches!(resp.tier, Tier::Claude)))
    }
}

fn dir() -> PathBuf {
    take_log::log_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn home_hr_take_status() -> HrTakeStatus {
    let now = Utc::now().timestamp_millis();
    tauri::async_runtime::spawn_blocking(move || status_in(&dir(), now))
        .await
        .unwrap_or(HrTakeStatus::NoneYet { due: true })
}

#[tauri::command]
#[specta::specta]
pub async fn home_hr_take_maybe_refresh(trigger: String) -> Result<HrTakeStatus, String> {
    if !matches!(trigger.as_str(), "open" | "focus" | "navigate") {
        return Err("unknown trigger".into());
    }
    Ok(maybe_refresh_with(&RealDeps, &dir()).await)
}

#[cfg(test)]
mod tests {
    use super::take_log::tests::temp_dir;
    use super::*;
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::sync::Mutex;

    const NOW: i64 = 1_800_000_000_000;

    /// Samples one a minute at the given bpm values, ending at `end_s`.
    fn run(end_s: i64, bpm: &[i64]) -> Vec<(i64, i64)> {
        let n = bpm.len() as i64;
        bpm.iter()
            .enumerate()
            .map(|(i, b)| (end_s - (n - 1 - i as i64) * 60, *b))
            .collect()
    }

    #[test]
    fn buckets_take_the_mean_per_minute_and_leave_gaps_null() {
        let to_ms = NOW;
        let to_s = to_ms / 1000;
        // two samples in the newest minute (70, 80 -> 75), one three minutes earlier, nothing else
        let samples = vec![(to_s - 3 * 60 - 30, 60), (to_s - 20, 70), (to_s - 10, 80)];
        let b = buckets(&samples, to_ms, 15);
        assert_eq!(b.len(), 15);
        assert_eq!(b[14], Some(75));
        assert_eq!(b[11], Some(60));
        assert_eq!(b.iter().flatten().count(), 2);
        // samples outside the window are ignored
        assert_eq!(
            buckets(&[(to_s - 16 * 60, 99), (to_s + 5, 99)], to_ms, 15)
                .iter()
                .flatten()
                .count(),
            0
        );
    }

    #[test]
    fn coverage_and_stats_use_covered_minutes_only() {
        let to_s = NOW / 1000;
        let w = build_window(
            &run(to_s, &[70, 72, 74, 76, 78, 80]),
            NOW,
            15,
            Some(NOW),
            &[],
        );
        assert_eq!(w.covered_min, 6);
        let s = w.stats.unwrap();
        assert_eq!((s.avg, s.min, s.max, s.first, s.last), (75, 70, 80, 70, 80));
        assert_eq!(block_reason(&w, NOW), Some("not_enough"));
        let full = build_window(&run(to_s, &[70; 12]), NOW, 15, Some(NOW), &[]);
        assert_eq!(full.covered_min, 12);
        assert_eq!(block_reason(&full, NOW), None);
        assert!(full.usual.is_none(), "no usual without 7 days");
    }

    #[test]
    fn usual_is_the_25th_to_75th_percentile_of_the_same_clock_window_over_14_days() {
        // 12:00 Tokyo = 03:00 UTC. Pick a `to` at that clock time.
        let to_ms = 1_800_000_000_000 - (1_800_000_000_000 % 86_400_000) + 3 * 3_600_000;
        let to_s = to_ms / 1000;
        let day = |level: i64| run(to_s, &[level; 12]);
        // seven prior days at 60, seven at 80 (every day has 12 covered minutes)
        let mut prior: Vec<Vec<(i64, i64)>> = Vec::new();
        for d in 0..14 {
            let level = if d < 7 { 60 } else { 80 };
            let shifted: Vec<(i64, i64)> = day(level)
                .into_iter()
                .map(|(t, b)| (t - (d + 1) * 86_400, b))
                .collect();
            prior.push(shifted);
        }
        let w = build_window(&run(to_s, &[70; 12]), to_ms, 15, Some(to_ms), &prior);
        let u = w.usual.expect("14 days of data");
        assert_eq!((u.lo, u.hi, u.days), (60, 80, 14));
        // six days is not enough
        let few = build_window(&run(to_s, &[70; 12]), to_ms, 15, Some(to_ms), &prior[..6]);
        assert!(few.usual.is_none());
        // a day with under 10 covered minutes does not count
        let mut thin = prior.clone();
        thin[0] = thin[0].iter().take(5).copied().collect();
        thin.truncate(7);
        assert!(
            build_window(&run(to_s, &[70; 12]), to_ms, 15, Some(to_ms), &thin)
                .usual
                .is_none()
        );
    }

    #[test]
    fn usual_is_waking_hours_only() {
        // 03:00 Tokyo = 18:00 UTC the day before
        let to_ms = 1_800_000_000_000 - (1_800_000_000_000 % 86_400_000) + 18 * 3_600_000;
        assert_eq!(tokyo_time(to_ms).hour(), 3);
        let prior: Vec<Vec<(i64, i64)>> = (0..14)
            .map(|d| {
                run(to_ms / 1000, &[60; 12])
                    .into_iter()
                    .map(|(t, b)| (t - (d + 1) * 86_400, b))
                    .collect()
            })
            .collect();
        assert!(usual_band(to_ms, 15, &prior).is_none());
    }

    #[test]
    fn a_stale_source_is_down_even_with_full_coverage() {
        let to_s = NOW / 1000;
        let w = build_window(&run(to_s, &[70; 15]), NOW, 15, Some(NOW), &[]);
        assert_eq!(block_reason(&w, NOW + SOURCE_STALE_MS), None);
        assert_eq!(
            block_reason(&w, NOW + SOURCE_STALE_MS + 1),
            Some("source_down")
        );
        let none = build_window(&[], NOW, 15, None, &[]);
        assert_eq!(block_reason(&none, NOW), Some("source_down"));
    }

    #[test]
    fn read_window_reads_a_noop_store_read_only() {
        let path = std::env::temp_dir().join(format!("hrwin-{}.sqlite", uuid::Uuid::new_v4()));
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE hrSample (deviceId TEXT NOT NULL, ts INTEGER NOT NULL, bpm INTEGER NOT NULL,
             synced INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (deviceId, ts));",
        )
        .unwrap();
        let end = NOW / 1000 - 120;
        for (ts, bpm) in run(end, &[70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81]) {
            conn.execute(
                "INSERT INTO hrSample (deviceId, ts, bpm) VALUES ('d', ?1, ?2)",
                rusqlite::params![ts, bpm],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO hrSample (deviceId, ts, bpm) VALUES ('other', ?1, 99)",
            [end],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO hrSample (deviceId, ts, bpm) VALUES ('d', ?1, 0)",
            [end + 30],
        )
        .unwrap();
        drop(conn);
        let w = read_window(&path, "d", NOW, 15).unwrap();
        assert_eq!(w.covered_min, 12);
        assert_eq!(w.last_sample_at, Some(end * 1000));
        assert_eq!(w.stats.unwrap().last, 81);
        assert!(read_window(&path, "nobody", NOW, 15).is_none());
        let _ = std::fs::remove_file(path);
    }

    fn window(bpm: &[i64]) -> HrWindow {
        build_window(&run(NOW / 1000, bpm), NOW, 15, Some(NOW), &[])
    }

    #[test]
    fn the_validator_rejects_invented_numbers_and_causal_or_medical_words() {
        let mut w = window(&[70, 72, 74, 76, 78, 80, 82, 84, 86, 88, 90]);
        w.usual = Some(HrUsual {
            lo: 62,
            hi: 74,
            days: 14,
        });
        assert!(text_ok(
            "Your heart rate rose from 70 to 90, averaging 80. That is above your usual 62 to 74.",
            &w
        ));
        assert!(
            text_ok("At 2:00 PM it rose from 70 to 90.", &w),
            "times are ignored"
        );
        assert!(
            !text_ok("Your heart rate averaged 81.", &w),
            "81 is not a fact"
        );
        assert!(!text_ok(
            "Your heart rate averaged 80, up 12 from yesterday.",
            &w
        ));
        assert!(!text_ok("It rose because of coffee, averaging 80.", &w));
        assert!(!text_ok("A caused spike to 90.", &w));
        assert!(!text_ok("Possible arrhythmia near 90.", &w));
        assert!(!text_ok("No numbers at all here.", &w));
        assert!(!text_ok("Averaging $80.", &w));
        assert!(
            text_ok("A secure 80 average.", &w),
            "'secure' is not 'cure'"
        );
    }

    #[test]
    fn template_and_memory_line_match_the_design_wording() {
        let mut w = window(&[70, 72, 74, 76, 78, 80, 82, 84, 86, 88, 90]);
        w.usual = Some(HrUsual {
            lo: 62,
            hi: 74,
            days: 14,
        });
        assert_eq!(
            template(&w),
            "Your heart rate rose from **70** to **90** over the last 15 minutes, averaging **80**. That's above your usual for this time of day (**62–74**). If you've been sitting, a few slow breaths or a short walk may help."
        );
        assert!(
            text_ok(&template(&w).replace("**", ""), &w),
            "the fallback passes its own check"
        );
        let steady = window(&[70, 71, 70, 71, 70, 71, 70, 71, 70, 71]);
        assert_eq!(
            template(&steady),
            "Your heart rate held steady around **71** over the last 15 minutes (**70–71**)."
        );
        let take = HrTake {
            id: "x".into(),
            generated_at: NOW,
            window: w,
            text: String::new(),
            memory: None,
            question: QUESTION.into(),
        };
        let line = memory_line(Some(&take)).unwrap();
        assert!(
            line.starts_with("At **") && line.ends_with("you were at **90** and climbing."),
            "{line}"
        );
        assert_eq!(memory_line(None), None);
    }

    struct Mock {
        now: AtomicI64,
        window: Mutex<Option<HrWindow>>,
        asks: AtomicUsize,
        reply: Mutex<Option<(String, bool)>>,
    }

    impl Mock {
        fn new(w: Option<HrWindow>, reply: Option<(String, bool)>) -> Self {
            Mock {
                now: AtomicI64::new(NOW),
                window: Mutex::new(w),
                asks: AtomicUsize::new(0),
                reply: Mutex::new(reply),
            }
        }
    }

    impl HrDeps for Mock {
        fn now_ms(&self) -> i64 {
            self.now.load(Ordering::SeqCst)
        }
        async fn window(&self) -> Option<HrWindow> {
            self.window.lock().unwrap().clone()
        }
        async fn ask(&self, _prompt: &str) -> Option<(String, bool)> {
            self.asks.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            self.reply.lock().unwrap().clone()
        }
    }

    fn good_window() -> HrWindow {
        window(&[70, 72, 74, 76, 78, 80, 82, 84, 86, 88, 90])
    }

    fn hr_count(dir: &Path) -> usize {
        take_log::list_in(dir)
            .iter()
            .filter(|e| e.hr.is_some())
            .count()
    }

    #[tokio::test]
    async fn within_30_minutes_returns_the_same_take_and_after_30_writes_a_new_one() {
        let dir = temp_dir("gate");
        let mock = Mock::new(
            Some(good_window()),
            Some((
                "Your heart rate rose from 70 to 90, averaging 80.".into(),
                false,
            )),
        );
        let HrTakeStatus::Take { take: first, due } = maybe_refresh_with(&mock, &dir).await else {
            panic!("expected a take")
        };
        assert!(!due);
        assert_eq!(
            first.text,
            "Your heart rate rose from 70 to 90, averaging 80."
        );
        assert_eq!(first.memory, None);
        assert_eq!(take_log::list_in(&dir)[0].tier, "light");
        // 29 min 59 s later: same take, no model call, nothing written
        mock.now.store(NOW + TAKE_GAP_MS - 1, Ordering::SeqCst);
        let HrTakeStatus::Take { take: again, due } = maybe_refresh_with(&mock, &dir).await else {
            panic!()
        };
        assert_eq!((again.id.clone(), due), (first.id.clone(), false));
        assert_eq!(mock.asks.load(Ordering::SeqCst), 1);
        assert_eq!(hr_count(&dir), 1);
        // status agrees: not due yet, due at 30 min
        assert!(matches!(
            status_in(&dir, NOW + TAKE_GAP_MS - 1),
            HrTakeStatus::Take { due: false, .. }
        ));
        assert!(matches!(
            status_in(&dir, NOW + TAKE_GAP_MS),
            HrTakeStatus::Take { due: true, .. }
        ));
        // 30 min later: a new take, with a memory line from the previous one
        mock.now.store(NOW + TAKE_GAP_MS, Ordering::SeqCst);
        *mock.window.lock().unwrap() = Some(good_window_at(NOW + TAKE_GAP_MS));
        let HrTakeStatus::Take { take: second, .. } = maybe_refresh_with(&mock, &dir).await else {
            panic!()
        };
        assert_ne!(second.id, first.id);
        assert!(second.memory.unwrap().contains("you were at **90**"));
        assert_eq!(hr_count(&dir), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn blocked_results_write_nothing_and_do_not_restart_the_clock() {
        let dir = temp_dir("blocked");
        let mock = Mock::new(
            Some(good_window()),
            Some(("Your heart rate averaged 80.".into(), false)),
        );
        maybe_refresh_with(&mock, &dir).await;
        // 31 minutes later: only 6 minutes of data
        mock.now.store(NOW + 31 * 60_000, Ordering::SeqCst);
        *mock.window.lock().unwrap() = Some(build_window(
            &run(NOW / 1000 + 31 * 60, &[70; 6]),
            NOW + 31 * 60_000,
            15,
            Some(NOW + 31 * 60_000),
            &[],
        ));
        match maybe_refresh_with(&mock, &dir).await {
            HrTakeStatus::NotEnough {
                previous,
                window,
                due,
            } => {
                assert!(previous.is_some() && due);
                assert_eq!(window.covered_min, 6);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(hr_count(&dir), 1);
        // the clock did not restart: status is still due, and the next return can write
        assert!(matches!(
            status_in(&dir, NOW + 31 * 60_000),
            HrTakeStatus::Take { due: true, .. }
        ));
        // source down: last sample 11 minutes old
        *mock.window.lock().unwrap() = Some(build_window(
            &run(NOW / 1000 + 20 * 60, &[70; 12]),
            NOW + 20 * 60_000,
            15,
            Some(NOW + 20 * 60_000),
            &[],
        ));
        match maybe_refresh_with(&mock, &dir).await {
            HrTakeStatus::SourceDown {
                previous,
                last_sample_at,
                due,
            } => {
                assert!(previous.is_some() && due);
                assert_eq!(last_sample_at, Some(NOW + 20 * 60_000));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(hr_count(&dir), 1);
        // no data at all
        *mock.window.lock().unwrap() = None;
        assert!(matches!(
            maybe_refresh_with(&mock, &dir).await,
            HrTakeStatus::SourceDown {
                last_sample_at: None,
                ..
            }
        ));
        // data returns: a take is written
        *mock.window.lock().unwrap() = Some(good_window_at(NOW + 31 * 60_000));
        assert!(matches!(
            maybe_refresh_with(&mock, &dir).await,
            HrTakeStatus::Take { due: false, .. }
        ));
        assert_eq!(hr_count(&dir), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn good_window_at(at: i64) -> HrWindow {
        build_window(
            &run(at / 1000, &[70, 72, 74, 76, 78, 80, 82, 84, 86, 88, 90]),
            at,
            15,
            Some(at),
            &[],
        )
    }

    #[tokio::test]
    async fn two_concurrent_calls_make_one_take() {
        let dir = temp_dir("conc");
        let mock = Mock::new(
            Some(good_window()),
            Some(("Your heart rate averaged 80.".into(), false)),
        );
        let (a, b) = tokio::join!(
            maybe_refresh_with(&mock, &dir),
            maybe_refresh_with(&mock, &dir)
        );
        assert_eq!(mock.asks.load(Ordering::SeqCst), 1);
        assert_eq!(hr_count(&dir), 1);
        let (HrTakeStatus::Take { take: a, .. }, HrTakeStatus::Take { take: b, .. }) = (a, b)
        else {
            panic!()
        };
        assert_eq!(a.id, b.id);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn an_invented_number_a_cloud_reply_or_no_model_falls_back_to_the_template() {
        for reply in [
            Some((
                "Your heart rate averaged 99, up from yesterday.".to_string(),
                false,
            )),
            Some(("Your heart rate averaged 80.".to_string(), true)),
            None,
        ] {
            let dir = temp_dir("fallback");
            let mock = Mock::new(Some(good_window()), reply);
            let HrTakeStatus::Take { take, .. } = maybe_refresh_with(&mock, &dir).await else {
                panic!()
            };
            assert_eq!(take.text, template(&good_window()));
            assert_eq!(take_log::list_in(&dir)[0].tier, "template");
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn status_serializes_to_the_hr_take_status_shape() {
        let s = serde_json::to_value(HrTakeStatus::SourceDown {
            previous: None,
            last_sample_at: Some(5),
            due: true,
        })
        .unwrap();
        assert_eq!(
            s,
            serde_json::json!({"kind": "source_down", "previous": null, "lastSampleAt": 5, "due": true})
        );
        assert_eq!(
            serde_json::to_value(HrTakeStatus::NoneYet { due: true }).unwrap(),
            serde_json::json!({"kind": "none_yet", "due": true})
        );
        let w = serde_json::to_value(good_window()).unwrap();
        for k in [
            "from",
            "to",
            "source",
            "lastSampleAt",
            "perMinute",
            "coveredMin",
            "stats",
            "usual",
        ] {
            assert!(w.get(k).is_some(), "{k}");
        }
        assert_eq!(w["perMinute"].as_array().unwrap().len(), 15);
    }
}
