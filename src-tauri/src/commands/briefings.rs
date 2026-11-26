//! Cross-domain intelligence: briefings, summaries, rules, outcome measurement (decision 0066,
//! Phase 15). It connects the domain owners through the shared recommendation store (0053) and
//! rebuilds nothing: every source is read through its owner's reader.
//!
//! **Runtime, exactly:** an in-app Rust timer (`start`, from `lib.rs` setup) ticks every minute
//! while Amara OS is running. The daily personal briefing is due at 07:00 JST and the Enterprise
//! briefing at 07:30 JST; the first tick after launch builds whatever is due and missing (catch-up),
//! and opening a briefing builds it if it's due (event-triggered). Nothing runs while the app is
//! closed: no LaunchAgent or background service exists for this. No language model is called;
//! everything is fixed rules and arithmetic.
//!
//! **Isolation (0053):** personal and neural inputs feed personal briefings and personal
//! recommendations; the Enterprise briefing reads business stores only and writes business
//! recommendations into the business file.

pub(crate) mod enterprise;
pub(crate) mod evaluate;
pub(crate) mod inputs;
pub(crate) mod rules;
pub(crate) mod summaries;
#[cfg(test)]
mod tests;

use super::amara_store::commands::{RecommendationDetail, RecommendationRow};
use super::amara_store::{store_for, Scope};
use super::local_store::{iso, read_json, write_json_atomic};
use super::shared::task::{blocking, JoinErr};
use super::shared::time::now_jst;
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) use enterprise::EnterpriseBriefing;
pub(crate) use summaries::DailyBriefing;

const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Minutes after midnight JST.
pub(crate) const PERSONAL_SLOT: u32 = 7 * 60;
pub(crate) const ENTERPRISE_SLOT: u32 = 7 * 60 + 30;
const KEEP_DAYS: i64 = 60;
const TICK: std::time::Duration = std::time::Duration::from_secs(60);
const FIRST_DELAY: std::time::Duration = std::time::Duration::from_secs(20);
const MANUAL_GAP_S: i64 = 60;
const OPEN: &[&str] = &["proposed", "shown", "accepted", "modified"];

static RUN: Mutex<()> = Mutex::new(());

// ───────────────────────────── runtime state ─────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct BriefingJobState {
    /// The JST day the job last built for.
    pub last_day: Option<String>,
    pub last_run_at: Option<String>,
    /// timer | catch_up | opened | manual
    pub last_trigger: Option<String>,
    /// ok | partial | failed
    pub last_result: Option<String>,
    pub last_error: Option<String>,
    /// What the rules did on the last run: created, suppressed, covered, refused.
    pub rule_counts: BTreeMap<String, u32>,
    pub measured: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RuntimeFile {
    jobs: BTreeMap<String, BriefingJobState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BriefingJob {
    pub id: String,
    pub label: String,
    pub schedule: String,
    pub next_due_at: String,
    pub state: BriefingJobState,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BriefingRuntime {
    /// Always false: nothing runs while Amara OS is closed.
    pub runs_while_closed: bool,
    /// When this app process started the timer (`None` in a browser preview or before setup).
    pub timer_started_at: Option<String>,
    pub jobs: Vec<BriefingJob>,
    pub model_inference: String,
    pub note: String,
}

static TIMER_STARTED: Mutex<Option<String>> = Mutex::new(None);

fn dir(data: &Path) -> PathBuf {
    data.join("briefings")
}

fn runtime_file(data: &Path) -> PathBuf {
    dir(data).join("runtime.json")
}

fn read_runtime(data: &Path) -> RuntimeFile {
    read_json(&runtime_file(data), 256 * 1024).unwrap_or_default()
}

fn save_job(data: &Path, id: &str, st: BriefingJobState) {
    let mut f = read_runtime(data);
    f.jobs.insert(id.into(), st);
    if let Err(e) = write_json_atomic(&runtime_file(data), &f, 256 * 1024) {
        eprintln!("[briefings] runtime state not saved: {e}");
    }
}

fn slot_at(day: NaiveDate, minute: u32) -> DateTime<FixedOffset> {
    super::local_store::jst()
        .from_local_datetime(
            &day.and_hms_opt(minute / 60, minute % 60, 0)
                .expect("valid slot"),
        )
        .single()
        .expect("fixed offset")
}

use chrono::TimeZone;

/// Due: past today's slot and not built for today yet.
pub(crate) fn is_due(now: DateTime<FixedOffset>, minute: u32, last_day: Option<&str>) -> bool {
    let today = now.date_naive();
    now.hour() * 60 + now.minute() >= minute
        && last_day != Some(today.format("%Y-%m-%d").to_string().as_str())
}

fn next_due(now: DateTime<FixedOffset>, minute: u32, last_day: Option<&str>) -> String {
    let today = now.date_naive();
    let d = if is_due(now, minute, last_day) {
        return iso(&now);
    } else if now.hour() * 60 + now.minute() < minute {
        today
    } else {
        today + Duration::days(1)
    };
    iso(&slot_at(d, minute))
}

pub(crate) fn runtime_in(data: &Path, now: DateTime<FixedOffset>) -> BriefingRuntime {
    let f = read_runtime(data);
    let job = |id: &str, label: &str, minute: u32, schedule: &str| {
        let st = f.jobs.get(id).cloned().unwrap_or_default();
        BriefingJob {
            id: id.into(),
            label: label.into(),
            schedule: schedule.into(),
            next_due_at: next_due(now, minute, st.last_day.as_deref()),
            state: st,
        }
    };
    BriefingRuntime {
        runs_while_closed: false,
        timer_started_at: TIMER_STARTED.lock().ok().and_then(|g| g.clone()),
        jobs: vec![
            job("personal", "Daily personal briefing", PERSONAL_SLOT, "Every day at 07:00 JST while Amara OS is open"),
            job("enterprise", "Enterprise briefing", ENTERPRISE_SLOT, "Every day at 07:30 JST while Amara OS is open"),
        ],
        model_inference: "None. Briefings and suggestions are fixed rules and arithmetic over your records.".into(),
        note: "Briefings are built by Amara OS while it is open. When it is closed nothing runs; the first minute after the next launch builds anything that was missed. The weekly Life review section is built when you open it.".into(),
    }
}

// ───────────────────────────── personal ─────────────────────────────

fn open_personal(data: &Path) -> Vec<RecommendationRow> {
    store_for(data, Scope::Personal)
        .list_recommendations(Scope::Personal, None, 300)
        .map(|v| {
            v.into_iter()
                .filter(|r| r.kind != "take" && OPEN.contains(&r.status.as_str()))
                .collect()
        })
        .unwrap_or_default()
}

fn daily_file(data: &Path, day: &str) -> PathBuf {
    dir(data).join("daily").join(format!("{day}.json"))
}

fn prune(folder: &Path, today: NaiveDate) {
    let Ok(rd) = std::fs::read_dir(folder) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(d) = name.strip_suffix(".json").and_then(inputs::day) {
            if (today - d).num_days() > KEEP_DAYS {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Gathers, runs the personal rules, measures due evaluations, builds and stores the briefing.
pub(crate) fn build_personal(
    data: &Path,
    now: DateTime<FixedOffset>,
    trigger: &str,
) -> Result<DailyBriefing, String> {
    let _g = RUN.lock().unwrap_or_else(|p| p.into_inner());
    let p = inputs::gather(data, now);
    let drafts = rules::personal_drafts(&p);
    let results = rules::emit(data, Scope::Personal, &drafts);
    let measured = evaluate::measure(data, Scope::Personal, p.today, |plan, end| {
        evaluate::personal_metric(data, &p, plan, end)
    });
    let open = open_personal(data);
    let b = summaries::daily(&p, &open, &iso(&now), trigger);
    let stored = write_json_atomic(&daily_file(data, &b.day), &b, MAX_BYTES);
    prune(&dir(data).join("daily"), p.today);
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for r in &results {
        *counts.entry(r.result.clone()).or_default() += 1;
    }
    save_job(
        data,
        "personal",
        BriefingJobState {
            last_day: Some(b.day.clone()),
            last_run_at: Some(iso(&now)),
            last_trigger: Some(trigger.into()),
            last_result: Some(match (&stored, b.missing.is_empty()) {
                (Err(_), _) => "failed".into(),
                (Ok(_), true) => "ok".into(),
                (Ok(_), false) => "partial".into(),
            }),
            last_error: stored.as_ref().err().cloned(),
            rule_counts: counts,
            measured: measured as u32,
        },
    );
    stored?;
    Ok(b)
}

pub(crate) fn read_personal(
    data: &Path,
    now: DateTime<FixedOffset>,
) -> Result<DailyBriefing, String> {
    let today = now.date_naive();
    let st = read_runtime(data)
        .jobs
        .get("personal")
        .cloned()
        .unwrap_or_default();
    if is_due(now, PERSONAL_SLOT, st.last_day.as_deref()) {
        return build_personal(data, now, "opened");
    }
    for d in [today, today - Duration::days(1)] {
        // Missing reads as None (never an empty briefing).
        if let Ok(Some(b)) = read_json::<Option<DailyBriefing>>(
            &daily_file(data, &d.format("%Y-%m-%d").to_string()),
            MAX_BYTES,
        ) {
            return Ok(b);
        }
    }
    build_personal(data, now, "opened")
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WeekCrossDomain {
    pub week_start: String,
    pub week_end: String,
    pub training: summaries::TrainingSummary,
    pub neuro: summaries::NeuroSummary,
    pub trajectories: Vec<summaries::GoalTrajectory>,
    /// Cross-domain recommendations whose reporting period ends in this week.
    pub recommendations: Vec<RecommendationRow>,
    pub missing: Vec<String>,
}

/// The weekly Life review's cross-domain part, built on read (it adds to the existing review).
pub(crate) fn week_in(
    data: &Path,
    week_start: &str,
    now: DateTime<FixedOffset>,
) -> Result<WeekCrossDomain, String> {
    let start = super::life_plan::monday(week_start)?;
    let (s, e) = summaries::week_bounds(start);
    let p = inputs::gather(data, now);
    let end = e.min(p.today);
    let recs = store_for(data, Scope::Personal)
        .list_recommendations(Scope::Personal, None, 300)?
        .into_iter()
        .filter(|r| r.produced_by == rules::PRODUCER)
        .filter(|r| {
            r.period_end
                .as_deref()
                .and_then(inputs::day)
                .is_some_and(|d| d >= s && d <= e)
        })
        .collect();
    Ok(WeekCrossDomain {
        week_start: rules::ymd(s),
        week_end: rules::ymd(e),
        training: summaries::training(&p, end),
        neuro: summaries::neuro(&p, end, 7),
        trajectories: summaries::trajectories(&p),
        recommendations: recs,
        missing: p.missing,
    })
}

// ───────────────────────────── enterprise ─────────────────────────────

fn enterprise_file(data: &Path, business: &str, day: &str) -> PathBuf {
    data.join("business")
        .join("briefings")
        .join(business)
        .join(format!("{day}.json"))
}

pub(crate) fn build_enterprise_one(
    data: &Path,
    business_id: &str,
    now: DateTime<FixedOffset>,
    trigger: &str,
) -> Result<(EnterpriseBriefing, Vec<rules::BriefingRuleResult>, usize), String> {
    let t = super::business_workspace::store_in(data).tenant(business_id)?;
    let bstore = store_for(data, Scope::Business);
    let b = enterprise::gather(
        data,
        &t,
        bstore.list_business_activity(business_id, 2000),
        now.date_naive(),
    );
    let mut drafts = Vec::new();
    drafts.extend(enterprise::objectives_vs_execution(&b));
    let search_note = match enterprise::search_vs_needs(&b) {
        Ok(d) => {
            drafts.extend(d);
            None
        }
        Err(n) => Some(n),
    };
    let results = rules::emit(data, Scope::Business, &drafts);
    let measured = evaluate::measure(data, Scope::Business, b.today, |plan, end| {
        (plan["params"]["business_id"].as_str() == Some(business_id))
            .then(|| evaluate::business_metric(&b, plan, end))
            .flatten()
    });
    let open: Vec<RecommendationRow> = bstore
        .list_business_recommendations(business_id, 200)?
        .into_iter()
        .filter(|r| r.kind != "take" && OPEN.contains(&r.status.as_str()))
        .collect();
    let brief = enterprise::briefing(&b, &open, search_note, &iso(&now), trigger);
    write_json_atomic(
        &enterprise_file(data, business_id, &brief.day),
        &brief,
        MAX_BYTES,
    )?;
    if let Some(parent) = enterprise_file(data, business_id, &brief.day).parent() {
        prune(parent, b.today);
    }
    Ok((brief, results, measured))
}

pub(crate) fn build_enterprise(
    data: &Path,
    now: DateTime<FixedOffset>,
    trigger: &str,
) -> Result<Vec<EnterpriseBriefing>, String> {
    let _g = RUN.lock().unwrap_or_else(|p| p.into_inner());
    let list = super::business_workspace::store_in(data).list_businesses();
    let mut out = Vec::new();
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut measured = 0usize;
    let mut errors = Vec::new();
    match &list {
        Ok(bs) => {
            for b in bs.iter().filter(|b| b.status != "archived") {
                match build_enterprise_one(data, &b.business_id, now, trigger) {
                    Ok((brief, results, m)) => {
                        for r in results {
                            *counts.entry(r.result).or_default() += 1;
                        }
                        measured += m;
                        out.push(brief);
                    }
                    Err(e) => errors.push(format!("{}: {e}", b.business_id)),
                }
            }
        }
        Err(e) => errors.push(e.clone()),
    }
    save_job(
        data,
        "enterprise",
        BriefingJobState {
            last_day: Some(now.date_naive().format("%Y-%m-%d").to_string()),
            last_run_at: Some(iso(&now)),
            last_trigger: Some(trigger.into()),
            last_result: Some(
                if errors.is_empty() {
                    "ok"
                } else if out.is_empty() {
                    "failed"
                } else {
                    "partial"
                }
                .into(),
            ),
            last_error: (!errors.is_empty()).then(|| errors.join("; ")),
            rule_counts: counts,
            measured: measured as u32,
        },
    );
    list.map(|_| out)
}

pub(crate) fn read_enterprise(
    data: &Path,
    business_id: &str,
    now: DateTime<FixedOffset>,
) -> Result<EnterpriseBriefing, String> {
    let today = now.date_naive();
    let st = read_runtime(data)
        .jobs
        .get("enterprise")
        .cloned()
        .unwrap_or_default();
    if !is_due(now, ENTERPRISE_SLOT, st.last_day.as_deref()) {
        for d in [today, today - Duration::days(1)] {
            if let Ok(Some(b)) = read_json::<Option<EnterpriseBriefing>>(
                &enterprise_file(data, business_id, &d.format("%Y-%m-%d").to_string()),
                MAX_BYTES,
            ) {
                return Ok(b);
            }
        }
    }
    let _g = RUN.lock().unwrap_or_else(|p| p.into_inner());
    build_enterprise_one(data, business_id, now, "opened").map(|x| x.0)
}

/// Cross-domain recommendations with their evidence and outcomes (open first, then recent).
pub(crate) fn recommendations_in(
    data: &Path,
    scope: Scope,
    business_id: Option<&str>,
) -> Result<Vec<RecommendationDetail>, String> {
    let store = store_for(data, scope);
    let rows = match (scope, business_id) {
        (Scope::Business, Some(b)) => store.list_business_recommendations(b, 300)?,
        (Scope::Business, None) => return Err("a business id is needed".into()),
        _ => store.list_recommendations(scope, None, 300)?,
    };
    let mut rows: Vec<RecommendationRow> = rows
        .into_iter()
        .filter(|r| r.produced_by == rules::PRODUCER)
        .collect();
    rows.sort_by_key(|r| {
        (
            !OPEN.contains(&r.status.as_str()),
            std::cmp::Reverse(r.created_at.clone()),
        )
    });
    Ok(rows
        .iter()
        .take(40)
        .filter_map(|r| {
            store
                .get_recommendation(&r.recommendation_id)
                .ok()
                .flatten()
        })
        .collect())
}

// ───────────────────────────── timer ─────────────────────────────

/// The in-app timer. Runs only while the app process runs.
pub fn start() {
    if let Ok(mut g) = TIMER_STARTED.lock() {
        *g = Some(iso(&now_jst()));
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        let mut first = true;
        loop {
            let data = crate::app_paths::data_dir();
            let trigger = if first { "catch_up" } else { "timer" };
            let _ = blocking(JoinErr::Plain, move || {
                tick(&data, now_jst(), trigger);
                Ok(())
            })
            .await;
            first = false;
            tokio::time::sleep(TICK).await;
        }
    });
}

/// One tick: builds each job that is due. Failures are recorded in the runtime state, never fatal.
pub(crate) fn tick(data: &Path, now: DateTime<FixedOffset>, trigger: &str) {
    let f = read_runtime(data);
    let last = |id: &str| f.jobs.get(id).and_then(|s| s.last_day.clone());
    if is_due(now, PERSONAL_SLOT, last("personal").as_deref()) {
        if let Err(e) = build_personal(data, now, trigger) {
            eprintln!("[briefings] personal briefing failed: {e}");
        }
    }
    if is_due(now, ENTERPRISE_SLOT, last("enterprise").as_deref()) {
        if let Err(e) = build_enterprise(data, now, trigger) {
            eprintln!("[briefings] enterprise briefing failed: {e}");
        }
    }
}

fn manual_ok(data: &Path, id: &str, now: DateTime<FixedOffset>) -> Result<(), String> {
    let st = read_runtime(data).jobs.get(id).cloned().unwrap_or_default();
    if let Some(t) = st
        .last_run_at
        .as_deref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
    {
        if (now - t).num_seconds() < MANUAL_GAP_S {
            return Err("Built less than a minute ago. Try again shortly.".into());
        }
    }
    Ok(())
}

// ───────────────────────────── commands ─────────────────────────────

fn data() -> PathBuf {
    crate::app_paths::data_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn briefing_daily_read() -> Result<DailyBriefing, String> {
    blocking(JoinErr::Plain, || read_personal(&data(), now_jst())).await
}

#[tauri::command]
#[specta::specta]
pub async fn briefing_daily_refresh() -> Result<DailyBriefing, String> {
    blocking(JoinErr::Plain, || {
        let (d, now) = (data(), now_jst());
        manual_ok(&d, "personal", now)?;
        build_personal(&d, now, "manual")
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn briefing_enterprise_read(business_id: String) -> Result<EnterpriseBriefing, String> {
    blocking(JoinErr::Plain, move || {
        read_enterprise(&data(), &business_id, now_jst())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn briefing_enterprise_refresh(
    business_id: String,
) -> Result<EnterpriseBriefing, String> {
    blocking(JoinErr::Plain, move || {
        let (d, now) = (data(), now_jst());
        let _g = RUN.lock().unwrap_or_else(|p| p.into_inner());
        build_enterprise_one(&d, &business_id, now, "manual").map(|x| x.0)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn briefing_week(week_start: String) -> Result<WeekCrossDomain, String> {
    blocking(JoinErr::Plain, move || {
        week_in(&data(), &week_start, now_jst())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn briefing_runtime() -> Result<BriefingRuntime, String> {
    blocking(JoinErr::Plain, || Ok(runtime_in(&data(), now_jst()))).await
}

#[tauri::command]
#[specta::specta]
pub async fn briefing_recommendations(
    scope: String,
    business_id: Option<String>,
) -> Result<Vec<RecommendationDetail>, String> {
    blocking(JoinErr::Plain, move || {
        let s = match scope.as_str() {
            "personal" => Scope::Personal,
            "business" => Scope::Business,
            _ => return Err("scope must be personal or business".into()),
        };
        recommendations_in(&data(), s, business_id.as_deref())
    })
    .await
}
