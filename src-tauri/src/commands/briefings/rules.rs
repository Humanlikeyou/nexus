//! Cross-domain rules (decision 0066). Fixed rules and arithmetic over real records, no model.
//!
//! Every rule returns a [`Draft`] with all six parts the brief requires: the observation, the
//! supporting records with the reporting period, an interpretation, a confidence with its basis and
//! limitations, a suggested action, and a measurable evaluation. Wording rules, pinned by tests:
//! - two things that moved in the same weeks are described as that, never as one causing the other;
//! - WHOOP figures are measurements from the strap via NOOP, never a readiness or mental-state verdict;
//! - a week with no records is unknown, never zero.

use super::super::amara_store::{
    store_for, Created, NewEvidence, NewRecommendation, Scope, Suppress,
};
use super::inputs::{day, monday_of, Personal};
use chrono::{Datelike, Duration, NaiveDate};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

pub(crate) const PRODUCER: &str = "rules:cross_domain";
/// Check-in tags that count as workload (personal records only, Taisun 2026-11-26).
pub(crate) const WORKLOAD_TAGS: &[&str] = &[
    "work",
    "busy",
    "stress",
    "stressed",
    "deadline",
    "deadlines",
    "travel",
    "overwhelmed",
    "meetings",
];
pub(crate) const BIOMETRIC_LIMIT: &str = "WHOOP figures are measurements from the strap, read through NOOP. They are not a readiness, health or psychological assessment.";
pub(crate) const CORRELATION_LIMIT: &str =
    "These moved in the same period. That does not show that one caused the other.";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Ev {
    pub record_ref: String,
    pub role: &'static str,
    pub quality: &'static str,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Draft {
    pub kind: &'static str,
    pub domain: &'static str,
    pub title: String,
    pub observation: String,
    pub interpretation: String,
    pub limitations: String,
    pub action: String,
    pub evaluation: Value,
    pub confidence: f64,
    pub confidence_basis: String,
    pub period: (NaiveDate, NaiveDate),
    pub evidence: Vec<Ev>,
    pub target_ref: Option<String>,
    /// What makes two drafts the same finding (with the kind).
    pub key: String,
    pub cooldown_days: i64,
}

impl Draft {
    pub(crate) fn fingerprint(&self) -> String {
        let h = Sha256::digest(format!("{}|{}", self.kind, self.key).as_bytes());
        format!("{}:{}", self.kind, &hex::encode(h)[..24])
    }
}

pub(crate) fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn start_of(d: NaiveDate) -> String {
    format!("{d}T00:00:00+09:00")
}

fn end_of(d: NaiveDate) -> String {
    format!("{d}T23:59:59+09:00")
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn mean(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluation(
    metric: &str,
    params: Value,
    baseline: Option<f64>,
    target: f64,
    direction: &str,
    unit: &str,
    measure_after: NaiveDate,
    window_days: i64,
    how: &str,
) -> Value {
    json!({
        "metric": metric,
        "params": params,
        "baseline": baseline,
        "target": target,
        "direction": direction,
        "unit": unit,
        "measure_after": ymd(measure_after),
        "window_days": window_days,
        "how": how,
    })
}

/// `[end - 6, end]`.
fn week_ending(end: NaiveDate) -> (NaiveDate, NaiveDate) {
    (end - Duration::days(6), end)
}

fn within(d: NaiveDate, w: (NaiveDate, NaiveDate)) -> bool {
    d >= w.0 && d <= w.1
}

// ───────────────────────────── training volume ─────────────────────────────

/// Logged training volume this week against the previous three weeks (workout engine only).
pub(crate) fn training_volume(p: &Personal) -> Option<Draft> {
    let cur = week_ending(p.today);
    let prev: Vec<(NaiveDate, NaiveDate)> = (1..=3)
        .map(|k| week_ending(p.today - Duration::days(7 * k)))
        .collect();
    let sets = |w: (NaiveDate, NaiveDate)| -> (u32, usize) {
        let in_w: Vec<_> = p.workouts.iter().filter(|x| within(x.day, w)).collect();
        (in_w.iter().map(|x| x.done_sets).sum(), in_w.len())
    };
    let (cur_sets, cur_n) = sets(cur);
    let recorded: Vec<f64> = prev
        .iter()
        .map(|w| sets(*w))
        .filter(|(_, n)| *n > 0)
        .map(|(s, _)| s as f64)
        .collect();
    if cur_n == 0 || recorded.len() < 2 {
        return None;
    }
    let chronic = mean(&recorded)?;
    if (cur_sets as f64) < chronic * 1.5 || (cur_sets as f64 - chronic) < 10.0 {
        return None;
    }
    let target = (chronic * 1.1).round();
    let ratio = r1(cur_sets as f64 / chronic);
    let mut observation = format!(
        "You logged {cur_sets} sets in {cur_n} session(s) from {} to {}, {ratio}× your average of {} sets a week over the {} earlier week(s) with logged training.",
        cur.0, cur.1, r1(chronic), recorded.len()
    );
    let mut evidence: Vec<Ev> = p
        .workouts
        .iter()
        .filter(|x| within(x.day, cur))
        .map(|x| Ev {
            record_ref: format!("workout:{}", x.id),
            role: "observation",
            quality: "measured",
            start: cur.0,
            end: cur.1,
        })
        .collect();
    evidence.extend(
        p.workouts
            .iter()
            .filter(|x| within(x.day, (prev[2].0, prev[0].1)))
            .map(|x| Ev {
                record_ref: format!("workout:{}", x.id),
                role: "calculation",
                quality: "measured",
                start: prev[2].0,
                end: prev[0].1,
            }),
    );
    if let Some(days) = &p.days {
        let min = |w: (NaiveDate, NaiveDate)| -> Option<f64> {
            let v: Vec<f64> = days
                .iter()
                .filter(|d| day(&d.day).is_some_and(|x| within(x, w)))
                .filter_map(|d| d.workout_min)
                .collect();
            (!v.is_empty()).then(|| v.iter().sum())
        };
        if let (Some(a), Some(b)) = (min(cur), min(prev[0])) {
            observation.push_str(&format!(
                " WHOOP detected {} workout minutes this week and {} the week before.",
                a.round(),
                b.round()
            ));
        }
    }
    let n_weeks = recorded.len();
    Some(Draft {
        kind: "training_volume_rising",
        domain: "fitness",
        title: format!("Training volume is {ratio}× your recent weekly average"),
        observation,
        interpretation: "Volume is rising much faster than your recent weeks. A large jump from one week to the next is a common reason to hold the next week steady. This is a general training guideline, not a reading of your body.".into(),
        limitations: format!("Only sessions logged in the workout engine are counted; sets don't capture effort or load. Based on {n_weeks} earlier week(s). {BIOMETRIC_LIMIT}"),
        action: format!("Keep next week at or below {target} sets."),
        evaluation: evaluation(
            "workout.done_sets_7d",
            json!({}),
            Some(cur_sets as f64),
            target,
            "at_most",
            "sets",
            p.today + Duration::days(7),
            7,
            "Sets logged in the workout engine in the 7 days ending on the measure date.",
        ),
        confidence: if n_weeks >= 3 { 0.6 } else { 0.45 },
        confidence_basis: format!("{n_weeks} earlier week(s) of logged sessions; a rule threshold of 1.5× and 10 sets"),
        period: (prev[2].0, cur.1),
        evidence,
        target_ref: None,
        key: ymd(monday_of(p.today)),
        cooldown_days: 7,
    })
}

// ───────────────────────────── sleep and recovery against energy ─────────────────────────────

struct WeekBody {
    sleep_h: Option<f64>,
    recovery: Option<f64>,
    energy: Option<f64>,
    nights: usize,
    energy_days: usize,
}

fn body_week(p: &Personal, w: (NaiveDate, NaiveDate)) -> WeekBody {
    let days = p.days.as_deref().unwrap_or(&[]);
    let rows: Vec<_> = days
        .iter()
        .filter(|d| day(&d.day).is_some_and(|x| within(x, w)))
        .collect();
    let sleep: Vec<f64> = rows
        .iter()
        .filter_map(|d| d.sleep_total_min.map(|m| m / 60.0))
        .collect();
    let rec: Vec<f64> = rows.iter().filter_map(|d| d.recovery).collect();
    let mut by_day: std::collections::BTreeMap<NaiveDate, Vec<f64>> = Default::default();
    for c in &p.checkins {
        if let Some(d) = day(&c.day).filter(|d| within(*d, w)) {
            by_day.entry(d).or_default().push(c.energy as f64);
        }
    }
    let energy: Vec<f64> = by_day.values().filter_map(|v| mean(v)).collect();
    WeekBody {
        sleep_h: (sleep.len() >= 3).then(|| mean(&sleep)).flatten(),
        recovery: (rec.len() >= 3).then(|| mean(&rec)).flatten(),
        energy: (energy.len() >= 3).then(|| mean(&energy)).flatten(),
        nights: sleep.len(),
        energy_days: energy.len(),
    }
}

/// Sleep and recovery this week against the energy Taisun reported in check-ins.
pub(crate) fn sleep_energy(p: &Personal) -> Option<Draft> {
    p.days.as_ref()?;
    let cur = week_ending(p.today);
    let prev = week_ending(p.today - Duration::days(7));
    let a = body_week(p, cur);
    let b = body_week(p, prev);
    let (sa, sb, ea, eb) = (a.sleep_h?, b.sleep_h?, a.energy?, b.energy?);
    let ds = sa - sb;
    let de = ea - eb;
    let dr = a.recovery.zip(b.recovery).map(|(x, y)| x - y);
    let together = (ds <= -0.5 || dr.is_some_and(|r| r <= -10.0)) && de <= -0.5;
    let apart = ds > -0.25 && dr.is_none_or(|r| r > -5.0) && de <= -1.0;
    if !together && !apart {
        return None;
    }
    let rec_text = match (a.recovery, b.recovery) {
        (Some(x), Some(y)) => format!(", recovery averaged {} against {}", x.round(), y.round()),
        _ => String::new(),
    };
    let observation = format!(
        "From {} to {} you slept {} h a night on average against {} h the week before{rec_text}. Your check-in energy averaged {} of 5 against {}.",
        cur.0, cur.1, r1(sa), r1(sb), r1(ea), r1(eb)
    );
    let mut evidence: Vec<Ev> = Vec::new();
    for w in [cur, prev] {
        for d in p.days.as_deref().unwrap_or(&[]) {
            if let Some(x) = day(&d.day).filter(|x| within(*x, w)) {
                if d.sleep_total_min.is_some() || d.recovery.is_some() {
                    evidence.push(Ev {
                        record_ref: format!("daily:{}", d.day),
                        role: "observation",
                        quality: "measured",
                        start: x,
                        end: x,
                    });
                }
            }
        }
        for c in &p.checkins {
            if let Some(x) = day(&c.day).filter(|x| within(*x, w)) {
                evidence.push(Ev {
                    record_ref: format!("checkin:{}", c.id),
                    role: "observation",
                    quality: "self_report",
                    start: x,
                    end: x,
                });
            }
        }
    }
    let n = a.nights.min(b.nights).min(a.energy_days).min(b.energy_days);
    let confidence = r1(0.25 + 0.05 * n as f64).min(0.6);
    let basis = format!(
        "{} and {} nights, {} and {} check-in days in the two weeks",
        a.nights, b.nights, a.energy_days, b.energy_days
    );
    let (kind, title, interpretation, action, eval) = if together {
        (
            "sleep_energy_down_together",
            "Sleep and reported energy both dropped this week".to_string(),
            format!("Less sleep and lower energy showed up in the same week. {CORRELATION_LIMIT} Other things this week could explain either one."),
            "Give yourself 30 more minutes for sleep on at least 4 nights this week.".to_string(),
            evaluation(
                "sleep.avg_h_7d",
                json!({}),
                Some(r1(sa)),
                r1(sb),
                "at_least",
                "hours",
                p.today + Duration::days(7),
                7,
                "Average nightly sleep from WHOOP via NOOP in the 7 days ending on the measure date.",
            ),
        )
    } else {
        (
            "energy_down_sleep_steady",
            "Your reported energy fell while sleep held steady".to_string(),
            format!("Sleep stayed about the same, so this week's lower energy isn't matched by a sleep change in the records. {CORRELATION_LIMIT}"),
            "For your next 5 check-ins, add a tag for what used your energy that day.".to_string(),
            evaluation(
                "checkin.tagged_count_7d",
                json!({}),
                None,
                5.0,
                "at_least",
                "check-ins",
                p.today + Duration::days(7),
                7,
                "Check-ins with at least one tag in the 7 days ending on the measure date.",
            ),
        )
    };
    Some(Draft {
        kind,
        domain: "cross",
        title,
        observation,
        interpretation,
        limitations: format!("Energy is your own 1–5 rating, so it reflects the day as you saw it. {BIOMETRIC_LIMIT}"),
        action,
        evaluation: eval,
        confidence,
        confidence_basis: basis,
        period: (prev.0, cur.1),
        evidence,
        target_ref: None,
        key: ymd(monday_of(p.today)),
        cooldown_days: 7,
    })
}

// ───────────────────────────── workload against training ─────────────────────────────

fn workload_items(p: &Personal, w: (NaiveDate, NaiveDate)) -> Vec<(String, NaiveDate)> {
    let mut out: Vec<(String, NaiveDate)> = p
        .dates
        .iter()
        .filter_map(|d| {
            day(&d.date)
                .filter(|x| within(*x, w))
                .map(|x| (format!("date:{}", d.id), x))
        })
        .collect();
    for c in &p.checkins {
        let tagged = c
            .tags
            .iter()
            .any(|t| WORKLOAD_TAGS.contains(&t.trim().to_lowercase().as_str()));
        if let Some(x) = day(&c.day).filter(|x| tagged && within(*x, w)) {
            out.push((format!("checkin:{}", c.id), x));
        }
    }
    out
}

/// Personal workload (Life dates due and check-ins tagged as work) against logged training.
pub(crate) fn workload_training(p: &Personal) -> Option<Draft> {
    let cur = week_ending(p.today);
    let prev = week_ending(p.today - Duration::days(7));
    let wa = workload_items(p, cur);
    let wb = workload_items(p, prev);
    let ta: Vec<_> = p.workouts.iter().filter(|x| within(x.day, cur)).collect();
    let tb: Vec<_> = p.workouts.iter().filter(|x| within(x.day, prev)).collect();
    if wa.len() < wb.len() + 2 || tb.is_empty() || ta.len() >= tb.len() {
        return None;
    }
    let observation = format!(
        "From {} to {}: {} commitments and work-tagged check-ins against {} the week before, and {} logged training session(s) against {}.",
        cur.0, cur.1, wa.len(), wb.len(), ta.len(), tb.len()
    );
    let mut evidence: Vec<Ev> = Vec::new();
    for (items, w) in [(&wa, cur), (&wb, prev)] {
        for (r, x) in items.iter() {
            evidence.push(Ev {
                record_ref: r.clone(),
                role: "observation",
                quality: "self_report",
                start: *x,
                end: *x,
            });
        }
        for t in p.workouts.iter().filter(|x| within(x.day, w)) {
            evidence.push(Ev {
                record_ref: format!("workout:{}", t.id),
                role: "observation",
                quality: "measured",
                start: t.day,
                end: t.day,
            });
        }
    }
    let target = tb.len() as f64;
    Some(Draft {
        kind: "workload_up_training_down",
        domain: "cross",
        title: "More commitments this week, fewer training sessions".into(),
        observation,
        interpretation: format!("Your recorded workload rose in the same week your logged training fell. {CORRELATION_LIMIT}"),
        limitations: "Workload counts only Life dates and check-ins tagged work, busy, stress or similar; meetings and tasks kept elsewhere aren't seen. Training counts only workout-engine sessions.".into(),
        action: format!("Plan {} short session(s) this week on the days with the fewest commitments.", tb.len()),
        evaluation: evaluation(
            "workout.sessions_7d",
            json!({}),
            Some(ta.len() as f64),
            target,
            "at_least",
            "sessions",
            p.today + Duration::days(7),
            7,
            "Ended workout-engine sessions in the 7 days ending on the measure date.",
        ),
        confidence: 0.4,
        confidence_basis: "two weeks of personal records; a rule threshold of 2 more workload items and 1 fewer session".into(),
        period: (prev.0, cur.1),
        evidence,
        target_ref: None,
        key: ymd(monday_of(p.today)),
        cooldown_days: 7,
    })
}

// ───────────────────────────── commitments against the week's priority ─────────────────────────────

/// Life dates in the next 7 days that aren't linked to this week's priority.
pub(crate) fn commitment_conflict(p: &Personal) -> Option<Draft> {
    let plan = p.plan.as_ref()?;
    let priority = plan.priority.as_deref().filter(|s| !s.trim().is_empty())?;
    let pg = plan.priority_goal_id.clone();
    let horizon = (p.today, p.today + Duration::days(6));
    let upcoming: Vec<_> = p
        .dates
        .iter()
        .filter(|d| !d.done)
        .filter(|d| day(&d.date).is_some_and(|x| within(x, horizon)))
        .filter(|d| pg.is_none() || d.goal_id != pg)
        .collect();
    if upcoming.len() < 3 {
        return None;
    }
    let for_priority: Vec<_> = plan
        .actions
        .iter()
        .filter(|a| pg.is_none() || a.goal_id == pg)
        .collect();
    let done = for_priority.iter().filter(|a| a.done).count();
    if !for_priority.is_empty() && done == for_priority.len() {
        return None;
    }
    let week_start = monday_of(p.today);
    let week_end = week_start + Duration::days(6);
    let mut busiest: std::collections::BTreeMap<String, usize> = Default::default();
    for d in &upcoming {
        *busiest.entry(d.date.clone()).or_default() += 1;
    }
    let busiest_day = busiest
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(d, _)| d.clone())
        .unwrap_or_default();
    let names: Vec<String> = upcoming
        .iter()
        .take(5)
        .map(|d| format!("{} ({})", d.title, d.date))
        .collect();
    let observation = format!(
        "{} commitments fall due between {} and {}, none linked to this week's priority “{priority}”: {}. {done} of {} planned action(s) for it are done.",
        upcoming.len(),
        horizon.0,
        horizon.1,
        names.join(", "),
        for_priority.len()
    );
    let mut evidence: Vec<Ev> = upcoming
        .iter()
        .filter_map(|d| {
            day(&d.date).map(|x| Ev {
                record_ref: format!("date:{}", d.id),
                role: "observation",
                quality: "self_report",
                start: x,
                end: x,
            })
        })
        .collect();
    if let Some(g) = &pg {
        if p.goals.iter().any(|x| &x.id == g) {
            evidence.push(Ev {
                record_ref: format!("goals:{g}"),
                role: "observation",
                quality: "self_report",
                start: week_start,
                end: week_end,
            });
        }
    }
    Some(Draft {
        kind: "commitments_vs_priority",
        domain: "life",
        title: format!("{} commitments this week compete with “{priority}”", upcoming.len()),
        observation,
        interpretation: "These dates may take the time this week's priority needs. Nothing in the records links them to it.".into(),
        limitations: "Only Life dates are seen; no calendar is connected, so meetings and events kept elsewhere aren't counted. A date may be quick to handle.".into(),
        action: format!("Block one session for “{priority}” before {busiest_day}, your busiest day."),
        evaluation: evaluation(
            "plan.priority_actions_done",
            json!({"week_start": ymd(week_start), "goal_id": pg}),
            Some(done as f64),
            (done + 1) as f64,
            "at_least",
            "planned actions done",
            week_end,
            7,
            "Planned actions for this week's priority marked done in the week plan.",
        ),
        confidence: 0.5,
        confidence_basis: format!("{} Life dates and this week's plan", upcoming.len()),
        period: horizon,
        evidence,
        target_ref: pg.map(|g| format!("goals:{g}")),
        key: format!("{}|{priority}", ymd(week_start)),
        cooldown_days: 7,
    })
}

// ───────────────────────────── repeated obstacles ─────────────────────────────

const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// The same habit missed on the same weekday, and a check-in tag that keeps coming with low energy.
pub(crate) fn repeated_obstacles(p: &Personal) -> Vec<Draft> {
    let mut out = Vec::new();
    let w = (p.today - Duration::days(27), p.today);
    if let Some(rows) = &p.habit_days {
        for g in p.goals.iter().filter(|g| {
            g.category == "habit" && !g.paused && super::super::goals::status_of(g) == "active"
        }) {
            for wd in 0..7u32 {
                let on: Vec<_> = rows
                    .iter()
                    .filter(|r| r.habit_id == g.id)
                    .filter(|r| {
                        day(&r.day).is_some_and(|d| {
                            within(d, w) && d.weekday().num_days_from_monday() == wd
                        })
                    })
                    .collect();
                let missed = on.iter().filter(|r| !r.done).count();
                if on.len() < 3 || missed < 3 {
                    continue;
                }
                let name = WEEKDAYS[wd as usize];
                let done = on.len() - missed;
                out.push(Draft {
                    kind: "habit_missed_same_weekday",
                    domain: "life",
                    title: format!("{} was missed on {missed} of the last {} {name}s", g.title, on.len()),
                    observation: format!(
                        "Between {} and {}, {} was marked not done on {missed} {name}s and done on {done}.",
                        w.0, w.1, g.title
                    ),
                    interpretation: format!("The misses cluster on one weekday, which points at something about {name}s rather than the habit as a whole."),
                    limitations: "Only days you marked are counted; unmarked days are unknown. The records don't say what got in the way.".into(),
                    action: format!(
                        "On {name}s, do the smaller version{} or move it to a different time that day.",
                        g.life.minimum_version.as_deref().map(|m| format!(" ({m})")).unwrap_or_default()
                    ),
                    evaluation: evaluation(
                        "habit.done_on_weekday",
                        json!({"habit_id": g.id, "weekday": wd}),
                        Some(done as f64),
                        1.0,
                        "at_least",
                        &format!("{name}s done"),
                        p.today + Duration::days(14),
                        14,
                        &format!("{name}s marked done in the 14 days ending on the measure date."),
                    ),
                    confidence: if on.len() >= 4 { 0.55 } else { 0.4 },
                    confidence_basis: format!("{} marked {name}s in 28 days", on.len()),
                    period: w,
                    evidence: on
                        .iter()
                        .filter_map(|r| {
                            day(&r.day).map(|d| Ev {
                                record_ref: format!("habit_day:{}:{}", r.habit_id, r.day),
                                role: "observation",
                                quality: "self_report",
                                start: d,
                                end: d,
                            })
                        })
                        .collect(),
                    target_ref: Some(format!("goals:{}", g.id)),
                    key: format!("{}|{wd}", g.id),
                    cooldown_days: 14,
                });
            }
        }
    }
    let mut tags: std::collections::BTreeMap<
        String,
        (Vec<&super::super::checkin::CheckInRecord>, usize),
    > = Default::default();
    for c in p
        .checkins
        .iter()
        .filter(|c| day(&c.day).is_some_and(|d| within(d, w)))
    {
        for t in &c.tags {
            let e = tags.entry(t.trim().to_lowercase()).or_default();
            e.1 += 1;
            if c.energy <= 2 || c.mood <= 2 {
                e.0.push(c);
            }
        }
    }
    for (tag, (low, all)) in tags {
        if tag.is_empty() || low.len() < 3 || low.len() * 2 < all {
            continue;
        }
        out.push(Draft {
            kind: "tag_with_low_energy",
            domain: "life",
            title: format!("“{tag}” came up in {} low-energy or low-mood check-ins", low.len()),
            observation: format!(
                "Between {} and {}, {} of your {all} check-ins tagged “{tag}” had energy or mood at 2 or below.",
                w.0,
                w.1,
                low.len()
            ),
            interpretation: format!("“{tag}” keeps appearing on harder days. {CORRELATION_LIMIT}"),
            limitations: "Tags and ratings are your own words and scores; days without a check-in are unknown.".into(),
            action: format!("Next time “{tag}” comes up, plan one thing that protects your energy that day and note it in the check-in."),
            evaluation: evaluation(
                "checkin.low_with_tag_28d",
                json!({"tag": tag}),
                Some(low.len() as f64),
                (low.len() as f64 - 1.0).max(0.0),
                "at_most",
                "low check-ins with the tag",
                p.today + Duration::days(28),
                28,
                "Check-ins with this tag and energy or mood at 2 or below, in the 28 days ending on the measure date.",
            ),
            confidence: 0.4,
            confidence_basis: format!("{} of {all} tagged check-ins in 28 days", low.len()),
            period: w,
            evidence: low
                .iter()
                .filter_map(|c| {
                    day(&c.day).map(|d| Ev {
                        record_ref: format!("checkin:{}", c.id),
                        role: "observation",
                        quality: "self_report",
                        start: d,
                        end: d,
                    })
                })
                .collect(),
            target_ref: None,
            key: tag.clone(),
            cooldown_days: 28,
        });
    }
    out
}

// ───────────────────────────── goal trajectory ─────────────────────────────

/// Goals clearly behind the pace their deadline needs (from [`super::summaries::trajectories`]).
pub(crate) fn goals_behind(p: &Personal) -> Vec<Draft> {
    super::summaries::trajectories(p)
        .into_iter()
        .filter(|t| t.state == "behind")
        .filter_map(|t| {
            let (actual, expected, left) = (t.progress_pct?, t.expected_pct?, t.days_left?);
            if !(8..=180).contains(&left) || expected - actual < 25.0 {
                return None;
            }
            let per_14 = ((100.0 - actual) / left as f64 * 14.0).ceil();
            let g = p.goals.iter().find(|g| g.id == t.goal_id)?;
            let d0 = super::inputs::day(&g.created_at).unwrap_or(p.today - Duration::days(30));
            Some(Draft {
                kind: "goal_behind_pace",
                domain: "life",
                title: format!("“{}” is {}% done with {}% of its time gone", t.title, actual.round(), expected.round()),
                observation: format!(
                    "“{}” is at {}% ({}), and {} of its time to {} has passed, with {left} days left.",
                    t.title, actual.round(), t.measure, format_pct(expected), t.deadline.clone().unwrap_or_default()
                ),
                interpretation: format!("At the pace so far it won't reach 100% by the deadline. To get there, it needs about {per_14} percentage points every two weeks."),
                limitations: "Progress is what you recorded (roadmap steps or the goal's own count). Steps differ in size, so percentages are a rough measure.".into(),
                action: t.next_step.clone().map(|s| format!("Do the next step this week: {s}.")).unwrap_or_else(|| "Pick the next step and put it in this week's plan.".into()),
                evaluation: evaluation(
                    "goal.progress_pct",
                    json!({"goal_id": t.goal_id}),
                    Some(actual.round()),
                    (actual + per_14).min(100.0).round(),
                    "at_least",
                    "% done",
                    p.today + Duration::days(14),
                    14,
                    "The goal's recorded progress on the measure date.",
                ),
                confidence: 0.5,
                confidence_basis: "the goal's deadline, start date and recorded progress".into(),
                period: (d0.min(p.today), p.today),
                evidence: vec![Ev {
                    record_ref: format!("goals:{}", t.goal_id),
                    role: "observation",
                    quality: "self_report",
                    start: d0.min(p.today),
                    end: p.today,
                }],
                target_ref: Some(format!("goals:{}", t.goal_id)),
                key: t.goal_id.clone(),
                cooldown_days: 14,
            })
        })
        .collect()
}

fn format_pct(x: f64) -> String {
    format!("{}%", x.round())
}

/// All personal rules, in a fixed order.
pub(crate) fn personal_drafts(p: &Personal) -> Vec<Draft> {
    let mut out: Vec<Draft> = [
        training_volume(p),
        sleep_energy(p),
        workload_training(p),
        commitment_conflict(p),
    ]
    .into_iter()
    .flatten()
    .collect();
    out.extend(repeated_obstacles(p));
    out.extend(goals_behind(p));
    out
}

// ───────────────────────────── writing ─────────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BriefingRuleResult {
    pub kind: String,
    /// created | suppressed | covered | refused
    pub result: String,
    pub recommendation_id: Option<String>,
    pub reason: Option<String>,
}

/// Kinds from other producers that already cover a finding about the same target.
fn covered_by_other_producer(data: &Path, scope: Scope, d: &Draft) -> bool {
    let (Some(t), "goal_behind_pace") = (&d.target_ref, d.kind) else {
        return false;
    };
    let Ok(conn) = store_for(data, scope).open() else {
        return true;
    };
    conn.query_row(
        "SELECT COUNT(*) FROM recommendation WHERE target_ref = ?1 AND kind = 'goal_stalled'
           AND status IN ('proposed','shown','accepted','modified')",
        rusqlite::params![t],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(true)
}

/// Writes each draft as a recommendation unless it is a duplicate.
pub(crate) fn emit(data: &Path, scope: Scope, drafts: &[Draft]) -> Vec<BriefingRuleResult> {
    let store = store_for(data, scope);
    drafts
        .iter()
        .map(|d| {
            let res =
                |result: &str, id: Option<String>, reason: Option<String>| BriefingRuleResult {
                    kind: d.kind.into(),
                    result: result.into(),
                    recommendation_id: id,
                    reason,
                };
            if covered_by_other_producer(data, scope, d) {
                return res(
                    "covered",
                    None,
                    Some("the weekly review already raised this goal".into()),
                );
            }
            let evaluation = d.evaluation.to_string();
            let (ps, pe) = (start_of(d.period.0), end_of(d.period.1));
            let fp = d.fingerprint();
            let ev_strings: Vec<(String, String, String)> = d
                .evidence
                .iter()
                .map(|e| (e.record_ref.clone(), start_of(e.start), end_of(e.end)))
                .collect();
            let evidence: Vec<NewEvidence> = d
                .evidence
                .iter()
                .zip(&ev_strings)
                .map(|(e, (r, s, en))| NewEvidence {
                    record_ref: r,
                    window_start: Some(s),
                    window_end: Some(en),
                    role: e.role,
                    quality: e.quality,
                    note: None,
                })
                .collect();
            let rec = NewRecommendation {
                id: None,
                scope,
                domain: d.domain,
                kind: d.kind,
                title: &d.title,
                body: &d.observation,
                expires_at: None,
                produced_by: PRODUCER,
                model_tier: None,
                confidence: Some(d.confidence),
                confidence_basis: Some(&d.confidence_basis),
                target_ref: d.target_ref.as_deref(),
                supersedes_id: None,
                observation: Some(&d.observation),
                rationale: Some(&d.interpretation),
                suggested_action: Some(&d.action),
                uncertainty: Some(&d.limitations),
                impact: None,
                effort: Some("low"),
                evaluation_json: Some(&evaluation),
                period_start: Some(&ps),
                period_end: Some(&pe),
                fingerprint: Some(&fp),
            };
            match store.create_unless_duplicate(
                &rec,
                &evidence,
                &Suppress {
                    cooldown_days: d.cooldown_days,
                    dismissed_days: 30,
                },
            ) {
                Ok(Created::New(id)) => res("created", Some(id), None),
                Ok(Created::Suppressed { reason, by }) => {
                    res("suppressed", Some(by), Some(reason.into()))
                }
                Err(e) => res("refused", None, Some(e)),
            }
        })
        .collect()
}
