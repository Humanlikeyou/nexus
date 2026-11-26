//! Goal progress readout (A4). Read-only `goal_progress_week`.
//!
//! Built only from the daily table's `daily_goal` rows (one per goal and JST day with at least one
//! finished roadmap step), the goals store (titles) and the generic patterns A3 has already published
//! (Benjamini-Hochberg and the confidence floor). Nothing here is a new store and nothing is guessed:
//! every count lists the `daily_goal:<day>:<goal id>` rows behind it, and the "in common" readout
//! quotes only published findings.
//!
//! - Per goal: roadmap steps finished in the 7 days ending `week_end` and in the 7 days before.
//! - Stalled goals: a goal in the goals store (personal, not a habit) with no finished step in the
//!   21 days ending `week_end` (inclusive), with its last finished day or `null` ("never").
//! - In common: for each goal with at least 3 finished-step days in the stored history, the 1, 2 and
//!   3 days before each finished-step day are looked at (decision 0022; lag 1, 2 and 3 are separate
//!   candidates, each tested on its own set of before-days). A published generic finding's cause is reported
//!   only when its cause-high days cover those before-days more often than that cause's own base
//!   rate (its share of cause days) and an exact one-sided binomial test agrees after Benjamini-Hochberg
//!   correction over every goal x finding x lag tested in the run (adjusted p at most 0.05). Before-days with no value for the cause count as not high, never as high.
//!   When nothing qualifies the readout is `state: "nothing_stands_out_yet"` with no entries.

use super::daily_table::{read_range_in, DailyGoalRow, WINDOW_DAYS};
use super::goals::Goal;
use super::local_store::read_json;
use super::patterns_generic::{bh_adjust, generic_patterns_in, GenericResult, Q};
use super::shared::task::{blocking, JoinErr};
use super::shared::time::{day_key, now_jst};
use chrono::{Duration, NaiveDate};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const GOALS_MAX_BYTES: u64 = 256 * 1024 * 8;
pub(crate) const STALLED_DAYS: i64 = 21;
pub(crate) const MIN_FINISHED_DAYS: usize = 3;
/// Before-days that can be compared with a cause (they must have a value for it) needed to report.
const MIN_TESTABLE_BEFORE_DAYS: usize = 3;
/// How many days before a finished step are looked at (lags 1 to MAX_BEFORE_LAG).
pub(crate) const MAX_BEFORE_LAG: i64 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoalWeek {
    pub goal_id: String,
    /// None when the goal is no longer in the goals store.
    pub title: Option<String>,
    pub this_week: i64,
    pub prev_week: i64,
    /// `daily_goal:<day>:<goal id>` rows behind both counts, ascending.
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StalledGoal {
    pub goal_id: String,
    pub title: String,
    /// None = never (no finished step in the stored history).
    pub last_finished_day: Option<String>,
    pub days_since: Option<i64>,
    /// The `daily_goal` row of the last finished day (empty for "never").
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommonCause {
    pub cause: String,
    /// The published finding this comes from: its effect and lag, with its adjusted p and confidence.
    pub effect: String,
    pub lag_days: u8,
    /// How many days before each finished-step day this cause was looked at (1 to 3).
    pub before_lag_days: u8,
    /// The published finding's own adjusted p (A3), for reference.
    pub finding_p_adj: f64,
    pub confidence: u8,
    /// Before-days (at `before_lag_days`) that have a value for the cause / of those, cause-high.
    pub before_days_tested: usize,
    pub before_days_cause_high: usize,
    pub share: f64,
    /// Share of all the finding's cause days that are cause-high.
    pub base_rate: f64,
    /// Exact one-sided binomial P(at least this many cause-high before-days | base rate).
    pub p_raw: f64,
    /// `p_raw` after Benjamini-Hochberg over every goal x finding x lag tested in this run; reported only at 0.05 or less.
    pub p_adj: f64,
    pub cause_high_before_days: Vec<String>,
    /// `daily:<day>` for each cause-high before-day, then the `daily_goal` rows of the finished days.
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommonGoal {
    pub goal_id: String,
    pub title: Option<String>,
    pub finished_days: usize,
    /// The day before each finished-step day (lag 1), ascending. Each cause names its own lag in `before_lag_days`.
    pub before_days: Vec<String>,
    pub causes: Vec<CommonCause>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InCommon {
    /// "found" | "nothing_stands_out_yet"
    pub state: String,
    /// Goals with at least 3 finished-step days that were looked at.
    pub goals_considered: usize,
    /// Only goals that have at least one qualifying cause.
    pub goals: Vec<CommonGoal>,
    /// Presence causes the generic patterns left out because their source failed to load.
    pub skipped_causes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GoalProgress {
    pub week_start: String,
    pub week_end: String,
    pub prev_week_start: String,
    pub goals: Vec<GoalWeek>,
    pub stalled: Vec<StalledGoal>,
    pub in_common: InCommon,
}

fn fmt(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn parse(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

fn ref_of(r: &DailyGoalRow) -> String {
    format!("daily_goal:{}:{}", r.day, r.goal_id)
}

/// Exact one-sided binomial tail P(X >= k) for X ~ Bin(n, p), 0 < p < 1.
fn binom_tail(n: usize, k: usize, p: f64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    if k > n {
        return 0.0;
    }
    let mut ln_fact = vec![0.0_f64; n + 1];
    for i in 1..=n {
        ln_fact[i] = ln_fact[i - 1] + (i as f64).ln();
    }
    let (lp, lq) = (p.ln(), (1.0 - p).ln());
    (k..=n)
        .map(|i| {
            (ln_fact[n] - ln_fact[i] - ln_fact[n - i] + i as f64 * lp + (n - i) as f64 * lq).exp()
        })
        .sum::<f64>()
        .clamp(0.0, 1.0)
}

/// Pure. `goals` is the goals store, `rows` every `daily_goal` row read (any order, may include days
/// after `week_end`, which are ignored), `generic` the published generic patterns.
pub(crate) fn compute(
    week_end: NaiveDate,
    goals: &[Goal],
    rows: &[DailyGoalRow],
    generic: &GenericResult,
) -> GoalProgress {
    let week_start = week_end - Duration::days(6);
    let prev_start = week_start - Duration::days(7);
    let stalled_from = week_end - Duration::days(STALLED_DAYS - 1);

    // Business goals stay in Enterprise: never read here.
    let business: BTreeSet<&str> = goals
        .iter()
        .filter(|g| super::weekly_review::is_business_goal(g))
        .map(|g| g.id.as_str())
        .collect();
    let title_of: BTreeMap<&str, &str> = goals
        .iter()
        .map(|g| (g.id.as_str(), g.title.as_str()))
        .collect();

    // goal -> finished-step rows (steps > 0), keyed by day.
    let mut finished: BTreeMap<&str, BTreeMap<NaiveDate, &DailyGoalRow>> = BTreeMap::new();
    for r in rows {
        let (Some(d), Some(n)) = (parse(&r.day), r.roadmap_steps_done) else {
            continue;
        };
        if d > week_end || n <= 0 || business.contains(r.goal_id.as_str()) {
            continue;
        }
        finished.entry(r.goal_id.as_str()).or_default().insert(d, r);
    }

    // Per goal, this week and the week before.
    let mut weeks = Vec::new();
    let ids: BTreeSet<&str> = goals
        .iter()
        .filter(|g| {
            !business.contains(g.id.as_str()) && g.category.trim().to_lowercase() != "habit"
        })
        .map(|g| g.id.as_str())
        .chain(finished.keys().copied())
        .collect();
    for id in ids {
        let (mut this_week, mut prev_week) = (0, 0);
        let mut refs = Vec::new();
        for (d, r) in finished.get(id).into_iter().flatten() {
            let n = r.roadmap_steps_done.unwrap_or(0);
            if *d >= week_start {
                this_week += n;
            } else if *d >= prev_start {
                prev_week += n;
            } else {
                continue;
            }
            refs.push(ref_of(r));
        }
        // A goal with nothing in either week still shows (as 0 and 0) when it is in the store.
        let in_store = title_of.contains_key(id);
        if this_week + prev_week == 0 && !in_store {
            continue;
        }
        weeks.push(GoalWeek {
            goal_id: id.to_string(),
            title: title_of.get(id).map(|t| t.to_string()),
            this_week,
            prev_week,
            refs,
        });
    }

    // Stalled: in the store, personal, not a habit, no finished step in the last 21 days.
    let mut stalled = Vec::new();
    for g in goals {
        if business.contains(g.id.as_str()) || g.category.trim().to_lowercase() == "habit" {
            continue;
        }
        let last = finished
            .get(g.id.as_str())
            .and_then(|m| m.iter().next_back());
        if last.is_some_and(|(d, _)| *d >= stalled_from) {
            continue;
        }
        stalled.push(StalledGoal {
            goal_id: g.id.clone(),
            title: g.title.clone(),
            last_finished_day: last.map(|(d, _)| fmt(*d)),
            days_since: last.map(|(d, _)| (week_end - *d).num_days()),
            refs: last.map(|(_, r)| vec![ref_of(r)]).unwrap_or_default(),
        });
    }

    // In common: the 1-3 days before finished-step days vs published generic findings.
    let mut first_by_cause: BTreeMap<&str, &super::patterns_generic::GenericFinding> =
        BTreeMap::new();
    for f in &generic.findings {
        // Findings arrive sorted best first: the first per cause is the strongest.
        first_by_cause.entry(f.cause.as_str()).or_insert(f);
    }
    let mut considered = 0;
    // Every goal x published finding x lag with enough testable before-days is one test in a single family
    // (Benjamini-Hochberg over all of them), so looking at many goals, causes and lags cannot inflate claims.
    struct Cand<'a> {
        goal: &'a str,
        days: usize,
        before: Vec<String>,
        cause: CommonCause,
    }
    let mut cands: Vec<Cand> = Vec::new();
    for (id, days) in &finished {
        if days.len() < MIN_FINISHED_DAYS {
            continue;
        }
        considered += 1;
        let goal_before: Vec<String> = days.keys().map(|d| fmt(*d - Duration::days(1))).collect();
        for (cause, f) in &first_by_cause {
            for lag in 1..=MAX_BEFORE_LAG {
                let before: BTreeSet<NaiveDate> =
                    days.keys().map(|d| *d - Duration::days(lag)).collect();
                let cause_days: BTreeSet<&str> = f.cause_days.iter().map(String::as_str).collect();
                let high: BTreeSet<&str> = f.cause_high_days.iter().map(String::as_str).collect();
                if cause_days.is_empty() {
                    continue;
                }
                let tested: Vec<String> = before
                    .iter()
                    .map(|d| fmt(*d))
                    .filter(|d| cause_days.contains(d.as_str()))
                    .collect();
                let hits: Vec<String> = tested
                    .iter()
                    .filter(|d| high.contains(d.as_str()))
                    .cloned()
                    .collect();
                if tested.len() < MIN_TESTABLE_BEFORE_DAYS {
                    continue;
                }
                let share = hits.len() as f64 / tested.len() as f64;
                let base_rate = high.len() as f64 / cause_days.len() as f64;
                if !(base_rate > 0.0 && base_rate < 1.0) {
                    continue;
                }
                let p_raw = binom_tail(tested.len(), hits.len(), base_rate);
                let mut refs: Vec<String> = hits.iter().map(|d| format!("daily:{d}")).collect();
                refs.extend(days.values().map(|r| ref_of(r)));
                cands.push(Cand {
                    goal: id,
                    days: days.len(),
                    before: goal_before.clone(),
                    cause: CommonCause {
                        cause: cause.to_string(),
                        effect: f.effect.clone(),
                        lag_days: f.lag_days,
                        before_lag_days: lag as u8,
                        finding_p_adj: f.p_adj,
                        confidence: f.confidence,
                        before_days_tested: tested.len(),
                        before_days_cause_high: hits.len(),
                        share,
                        base_rate,
                        p_raw,
                        p_adj: p_raw,
                        cause_high_before_days: hits,
                        refs,
                    },
                });
            }
        }
    }
    let adj = bh_adjust(&cands.iter().map(|c| c.cause.p_raw).collect::<Vec<_>>());
    let mut common: Vec<CommonGoal> = Vec::new();
    for (mut c, p_adj) in cands.into_iter().zip(adj) {
        c.cause.p_adj = p_adj;
        // Above its own base rate and surviving the correction across the whole family.
        if c.cause.share <= c.cause.base_rate || p_adj > Q {
            continue;
        }
        match common.iter_mut().find(|g| g.goal_id == c.goal) {
            Some(g) => g.causes.push(c.cause),
            None => common.push(CommonGoal {
                goal_id: c.goal.to_string(),
                title: title_of.get(c.goal).map(|t| t.to_string()),
                finished_days: c.days,
                before_days: c.before,
                causes: vec![c.cause],
            }),
        }
    }

    GoalProgress {
        week_start: fmt(week_start),
        week_end: fmt(week_end),
        prev_week_start: fmt(prev_start),
        goals: weeks,
        stalled,
        in_common: InCommon {
            state: if common.is_empty() {
                "nothing_stands_out_yet"
            } else {
                "found"
            }
            .into(),
            goals_considered: considered,
            goals: common,
            skipped_causes: generic.skipped_causes.clone(),
        },
    }
}

/// Reads the stored table, the goals store and the generic patterns under `dir`. A missing table is an
/// empty readout; a store that cannot be read is an error.
pub(crate) fn goal_progress_in(dir: &Path, week_end: NaiveDate) -> Result<GoalProgress, String> {
    let from = fmt(week_end - Duration::days(WINDOW_DAYS - 1));
    let range = read_range_in(dir, &from, &fmt(week_end))?;
    let generic = generic_patterns_in(dir, week_end)?;
    let goals = read_json::<Vec<Goal>>(&dir.join("goals.json"), GOALS_MAX_BYTES)?;
    Ok(compute(week_end, &goals, &range.goals, &generic))
}

/// `week_end`: a JST day `YYYY-MM-DD` (default today). The week is the 7 days ending then.
#[tauri::command]
#[specta::specta]
pub async fn goal_progress_week(week_end: Option<String>) -> Result<GoalProgress, String> {
    let end = match week_end {
        Some(s) => parse(&s).ok_or_else(|| "bad_week_end".to_string())?,
        None => day_key(&now_jst()),
    };
    blocking(JoinErr::Plain, move || {
        goal_progress_in(&crate::app_paths::data_dir(), end)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::super::daily_table::{write_in, DailyRow};
    use super::super::local_store::tests::temp_dir;
    use super::*;

    fn d(s: &str) -> NaiveDate {
        parse(s).unwrap()
    }
    fn goal(id: &str, title: &str, cat: &str) -> Goal {
        Goal {
            id: id.into(),
            title: title.into(),
            category: cat.into(),
            target: 1.0,
            current: 0.0,
            unit: "".into(),
            deadline: None,
            created_at: String::new(),
            updated_at: String::new(),
            venture_id: None,
            why: None,
            remind_at: None,
            paused: false,
            life: Default::default(),
        }
    }
    fn gr(day: &str, id: &str, n: i64) -> DailyGoalRow {
        DailyGoalRow {
            day: day.into(),
            goal_id: id.into(),
            roadmap_steps_done: Some(n),
            ..Default::default()
        }
    }
    fn empty() -> GenericResult {
        GenericResult {
            tests_run: 0,
            findings: vec![],
            withheld: 0,
            rows: 0,
            skipped_causes: vec![],
        }
    }
    fn finding(
        cause: &str,
        cause_days: &[String],
        high: &[String],
    ) -> super::super::patterns_generic::GenericFinding {
        super::super::patterns_generic::GenericFinding {
            cause: cause.into(),
            effect: "focus_mean".into(),
            lag_days: 0,
            split: "at_or_above_mean".into(),
            threshold: 0.0,
            n_with: high.len(),
            n_without: cause_days.len() - high.len(),
            mean_with: 0.0,
            mean_without: 0.0,
            delta: 0.0,
            r: 0.5,
            p_raw: 0.001,
            p_adj: 0.01,
            confidence: 4,
            strength: "moderate".into(),
            cause_high_days: high.to_vec(),
            cause_days: cause_days.to_vec(),
            refs: vec![],
            recent: None,
        }
    }

    #[test]
    fn counts_this_week_and_last_week_per_goal_with_refs() {
        let goals = [goal("g1", "Ship", "career"), goal("g2", "Read", "career")];
        let rows = [
            gr("2026-11-12", "g1", 2),
            gr("2026-11-08", "g1", 1), // this week = 11-06..11-12
            gr("2026-11-05", "g1", 4), // previous week = 10-30..11-05
            gr("2026-10-29", "g1", 9), // before that: not counted
            gr("2026-11-13", "g1", 7), // after week_end: ignored
        ];
        let p = compute(d("2026-11-12"), &goals, &rows, &empty());
        let g1 = p.goals.iter().find(|g| g.goal_id == "g1").unwrap();
        assert_eq!((g1.this_week, g1.prev_week), (3, 4));
        assert_eq!(
            g1.refs,
            vec![
                "daily_goal:2026-11-05:g1",
                "daily_goal:2026-11-08:g1",
                "daily_goal:2026-11-12:g1"
            ]
        );
        let g2 = p.goals.iter().find(|g| g.goal_id == "g2").unwrap();
        assert_eq!((g2.this_week, g2.prev_week, g2.refs.len()), (0, 0, 0));
        assert_eq!(g2.title.as_deref(), Some("Read"));
    }

    #[test]
    fn stalled_is_exactly_no_step_in_the_last_21_days() {
        let goals = [
            goal("edge_in", "Edge in", "career"),
            goal("edge_out", "Edge out", "career"),
            goal("never", "Never", "career"),
            goal("habit", "Habit", "habit"),
            goal("biz", "Biz", "business"),
        ];
        // week_end 2026-11-12: the 21-day window is 2026-10-23..=2026-11-12.
        let rows = [
            gr("2026-10-23", "edge_in", 1),
            gr("2026-10-22", "edge_out", 1),
        ];
        let p = compute(d("2026-11-12"), &goals, &rows, &empty());
        let ids: Vec<&str> = p.stalled.iter().map(|s| s.goal_id.as_str()).collect();
        assert_eq!(ids, vec!["edge_out", "never"]);
        assert_eq!(
            p.stalled[0].last_finished_day.as_deref(),
            Some("2026-10-22")
        );
        assert_eq!(p.stalled[0].days_since, Some(21));
        assert_eq!(p.stalled[0].refs, vec!["daily_goal:2026-10-22:edge_out"]);
        assert_eq!(p.stalled[1].last_finished_day, None);
        assert!(p.stalled[1].refs.is_empty());
        // Business goals never appear anywhere.
        assert!(p.goals.iter().all(|g| g.goal_id != "biz"));
    }

    /// 12 finished-step days for g1 (every 3rd day over 36 days); cause "sleep_total_min" is high on the
    /// day before each of them, and has a base rate of 1/3. Another cause is high on the same share as
    /// its base rate.
    fn scenario() -> (Vec<DailyGoalRow>, Vec<String>, Vec<String>, Vec<String>) {
        let start = d("2026-10-01");
        let day = |i: i64| fmt(start + Duration::days(i));
        let all: Vec<String> = (0..40).map(day).collect();
        let finished: Vec<DailyGoalRow> =
            (1..37).step_by(3).map(|i| gr(&day(i), "g1", 1)).collect();
        // before-days are i-1 = 0, 3, 6, ...; sleep high exactly on multiples of 3.
        let sleep_high: Vec<String> = (0..40).step_by(3).map(day).collect();
        // noise cause: high on days with i % 2 == 0.
        let noise_high: Vec<String> = (0..40).filter(|i| i % 2 == 0).map(day).collect();
        (finished, all, sleep_high, noise_high)
    }

    #[test]
    fn in_common_reports_only_published_findings_that_beat_chance() {
        let (rows, all, sleep_high, noise_high) = scenario();
        let goals = [goal("g1", "Ship", "career")];
        let mut g = empty();
        g.findings = vec![
            finding("sleep_total_min", &all, &sleep_high),
            finding("strain", &all, &noise_high),
        ];
        let p = compute(d("2026-11-12"), &goals, &rows, &g);
        assert_eq!(p.in_common.state, "found");
        assert_eq!(p.in_common.goals_considered, 1);
        let c = &p.in_common.goals[0].causes;
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].cause, "sleep_total_min");
        assert_eq!(c[0].before_lag_days, 1);
        assert_eq!(
            (c[0].before_days_tested, c[0].before_days_cause_high),
            (12, 12)
        );
        assert!((c[0].base_rate - 14.0 / 40.0).abs() < 1e-12);
        assert!(c[0].p_adj <= Q && c[0].p_raw <= c[0].p_adj);
        assert_eq!(c[0].cause_high_before_days.len(), 12);
        assert!(c[0].refs.contains(&"daily:2026-10-01".to_string()));
        assert!(c[0].refs.contains(&"daily_goal:2026-10-02:g1".to_string()));
        assert_eq!(p.in_common.goals[0].finished_days, 12);
    }

    #[test]
    fn nothing_published_means_nothing_stands_out_yet() {
        let (rows, ..) = scenario();
        let goals = [goal("g1", "Ship", "career")];
        // The pattern is there in the rows, but no finding was published: nothing is invented.
        let p = compute(d("2026-11-12"), &goals, &rows, &empty());
        assert_eq!(p.in_common.state, "nothing_stands_out_yet");
        assert!(p.in_common.goals.is_empty());
        assert_eq!(p.in_common.goals_considered, 1);
        // A goal with only 2 finished-step days is not considered at all.
        let p = compute(
            d("2026-11-12"),
            &goals,
            &[gr("2026-11-01", "g1", 1), gr("2026-11-05", "g1", 1)],
            &empty(),
        );
        assert_eq!(p.in_common.goals_considered, 0);
    }

    #[test]
    fn empty_when_no_data_and_binomial_tail_is_exact() {
        let dir = temp_dir("gp0");
        let p = goal_progress_in(&dir, d("2026-11-12")).unwrap();
        assert!(p.goals.is_empty() && p.stalled.is_empty());
        assert_eq!(p.in_common.state, "nothing_stands_out_yet");
        assert_eq!(p.week_start, "2026-11-06");
        assert_eq!(p.prev_week_start, "2026-10-30");
        let _ = std::fs::remove_dir_all(dir);
        // P(X >= 2 | n = 3, p = .5) = 3/8 + 1/8
        assert!((binom_tail(3, 2, 0.5) - 0.5).abs() < 1e-12);
        assert!((binom_tail(3, 3, 0.5) - 0.125).abs() < 1e-12);
        assert_eq!(binom_tail(3, 4, 0.5), 0.0);
    }

    #[test]
    fn end_to_end_through_the_stored_table_and_generic_patterns() {
        let dir = temp_dir("gp1");
        let end = d("2026-11-12");
        let from = end - Duration::days(59);
        // 2 goals; g1 finished a step on 4 days, g2 never.
        std::fs::write(
            dir.join("goals.json"),
            serde_json::to_string(&serde_json::json!([
                {"id":"g1","title":"Ship","category":"career","target":1.0,"current":0.0,"unit":"","deadline":null,"created_at":"","updated_at":""},
                {"id":"g2","title":"Read","category":"career","target":1.0,"current":0.0,"unit":"","deadline":null,"created_at":"","updated_at":""}
            ]))
            .unwrap(),
        )
        .unwrap();
        let rows: Vec<DailyRow> = (0..60)
            .map(|i| DailyRow {
                day: fmt(from + Duration::days(i)),
                mood_mean: Some(3.0),
                ..Default::default()
            })
            .collect();
        let goal_rows = vec![
            gr("2026-11-10", "g1", 2),
            gr("2026-11-05", "g1", 1),
            gr("2026-10-30", "g1", 1),
            gr("2026-10-01", "g1", 1),
        ];
        write_in(&dir, &rows, &goal_rows, &[], 0, (from, end)).unwrap();
        let p = goal_progress_in(&dir, end).unwrap();
        let g1 = p.goals.iter().find(|g| g.goal_id == "g1").unwrap();
        assert_eq!((g1.this_week, g1.prev_week), (2, 2));
        assert_eq!(p.stalled.len(), 1);
        assert_eq!(p.stalled[0].goal_id, "g2");
        assert_eq!(p.stalled[0].last_finished_day, None);
        // Flat data: no published generic finding, so nothing stands out.
        assert_eq!(p.in_common.state, "nothing_stands_out_yet");
        assert_eq!(p.in_common.goals_considered, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unreadable_goals_store_is_an_error_not_an_empty_readout() {
        let dir = temp_dir("gp2");
        std::fs::write(dir.join("goals.json"), "{nope").unwrap();
        assert!(goal_progress_in(&dir, d("2026-11-12")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Many goals x one published cause. One goal's 5 of 5 (raw p = 0.031) would pass an uncorrected
    /// 0.05 test; among 20 tests it must not be reported, while a strong planted one still is.
    #[test]
    fn many_chance_candidates_report_nothing_and_a_strong_one_survives_correction() {
        let start = d("2026-10-01");
        let day = |i: i64| fmt(start + Duration::days(i));
        let all: Vec<String> = (0..60).map(day).collect();
        let high: Vec<String> = (0..60).step_by(2).map(day).collect(); // base rate 0.5
        let mut g = empty();
        g.findings = vec![finding("sleep_total_min", &all, &high)];
        let mut goals = Vec::new();
        let mut rows = Vec::new();
        // g0: finished on 1,3,5,7,9 -> before-days 0,2,4,6,8 all high: 5 of 5.
        // g1..g19: finished on 1,2,4,6,8 -> before-days 0,1,3,5,7: 1 of 5.
        for k in 0..20 {
            let id = format!("g{k}");
            goals.push(goal(&id, &id, "career"));
            let days: [i64; 5] = if k == 0 {
                [1, 3, 5, 7, 9]
            } else {
                [1, 2, 4, 6, 8]
            };
            rows.extend(days.iter().map(|i| gr(&day(*i), &id, 1)));
        }
        let end = d("2026-12-01");
        let p = compute(end, &goals, &rows, &g);
        assert_eq!(p.in_common.goals_considered, 20);
        assert_eq!(
            p.in_common.state, "nothing_stands_out_yet",
            "{:?}",
            p.in_common
        );
        // The same g0 alone (a family of one) is reported: the correction is what removed it.
        // (Adapted for 0022 item 4: the family now also holds g0's lag-2 and lag-3 candidates, so
        // g0 alone no longer passes either; the uncorrected tail is still below 0.05.)
        assert!((binom_tail(5, 5, 0.5) - 0.03125).abs() < 1e-9);
        let alone = compute(end, &goals[..1], &rows[..5], &g);
        assert_eq!(alone.in_common.state, "nothing_stands_out_yet");

        // Add a strong one: 12 of 12 (raw 0.5^12) among the same 20.
        goals.push(goal("strong", "Strong", "career"));
        rows.extend((0..12).map(|k| gr(&day(1 + 2 * k), "strong", 1)));
        let p = compute(end, &goals, &rows, &g);
        assert_eq!(p.in_common.goals.len(), 1);
        let c = &p.in_common.goals[0];
        assert_eq!(c.goal_id, "strong");
        let cc = &c.causes[0];
        assert!(cc.p_raw < 3e-4 && cc.p_adj > cc.p_raw && cc.p_adj <= Q);
    }

    /// A cause that is high exactly two days before each finished step is found at lag 2, and at no
    /// other lag; a cause with no tie to the steps reports nothing at any lag.
    #[test]
    fn a_planted_lag_2_cause_is_found_at_lag_2_and_chance_across_lags_reports_nothing() {
        let start = d("2026-10-01");
        let day = |i: i64| fmt(start + Duration::days(i));
        let all: Vec<String> = (0..60).map(day).collect();
        // Steps on days 2, 7, 12, ... (every 5th). Lag-2 days are 0, 5, 10, ...: all high.
        // High days are 0,5,10,... plus none else: base rate 12/60 = 0.2; lag 1 and 3 hit none.
        let high: Vec<String> = (0..60).step_by(5).map(day).collect();
        let rows: Vec<DailyGoalRow> = (0..10).map(|k| gr(&day(2 + 5 * k), "g1", 1)).collect();
        let goals = [goal("g1", "Ship", "career")];
        let mut g = empty();
        g.findings = vec![finding("sleep_total_min", &all, &high)];
        let p = compute(d("2026-12-01"), &goals, &rows, &g);
        assert_eq!(p.in_common.state, "found", "{:?}", p.in_common);
        let causes = &p.in_common.goals[0].causes;
        assert_eq!(causes.len(), 1, "{causes:?}");
        assert_eq!(causes[0].before_lag_days, 2);
        assert_eq!(
            (
                causes[0].before_days_tested,
                causes[0].before_days_cause_high
            ),
            (10, 10)
        );
        assert!(causes[0].cause_high_before_days.contains(&day(0)));
        // Chance: a cause high on every other day hits half the before-days at every lag.
        let alt: Vec<String> = (0..60).step_by(2).map(day).collect();
        let mut g = empty();
        g.findings = vec![finding("strain", &all, &alt)];
        let rows: Vec<DailyGoalRow> = [3, 6, 11, 14, 19, 22, 27, 30]
            .iter()
            .map(|i| gr(&day(*i), "g1", 1))
            .collect();
        let p = compute(d("2026-12-01"), &goals, &rows, &g);
        assert_eq!(
            p.in_common.state, "nothing_stands_out_yet",
            "{:?}",
            p.in_common
        );
    }
}
