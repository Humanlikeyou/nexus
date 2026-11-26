//! Decision 0066 tests. Each domain's rules on their own (pure, from built inputs), then the
//! approved cross-domain flows against real files in temp folders (gather → rules → store →
//! briefing → measurement), and the isolation between personal and business.

use super::inputs::{Personal, Workout};
use super::rules::*;
use super::*;
use crate::commands::checkin::CheckInRecord;
use crate::commands::daily_table::DailyRow;
use chrono::{NaiveDate, TimeZone};
use serde_json::json;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmp(tag: &str) -> Tmp {
    let p = std::env::temp_dir().join(format!("nexus-briefings-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    Tmp(p)
}

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

/// Thursday 2026-11-26, 08:00 JST.
fn now() -> DateTime<FixedOffset> {
    super::super::local_store::jst()
        .with_ymd_and_hms(2026, 11, 26, 8, 0, 0)
        .unwrap()
}

fn at(day: &str, h: u32) -> DateTime<FixedOffset> {
    let x = d(day);
    super::super::local_store::jst()
        .from_local_datetime(&x.and_hms_opt(h, 0, 0).unwrap())
        .unwrap()
}

fn w(id: &str, day: &str, sets: u32) -> Workout {
    Workout {
        id: id.into(),
        day: d(day),
        done_sets: sets,
        total_reps: sets * 10,
        active_min: 20.0,
        ..Default::default()
    }
}

fn ci(id: &str, day: &str, mood: u8, energy: u8, tags: &[&str]) -> CheckInRecord {
    CheckInRecord {
        id: id.into(),
        at: format!("{day}T21:00:00+09:00"),
        day: day.into(),
        mood,
        energy,
        tags: tags.iter().map(|t| t.to_string()).collect(),
        text: String::new(),
        transcribed: false,
    }
}

fn base() -> Personal {
    Personal {
        today: d("2026-11-26"),
        ..Default::default()
    }
}

/// Words that would claim causation or a body/mind verdict.
const FORBIDDEN: &[&str] = &[
    "because",
    "caused by",
    "causes",
    "due to",
    "led to",
    "leads to",
    "ready to train",
    "readiness score",
    "you are stressed",
    "you are tired",
    "you are depressed",
    "burnout",
    "overtrained",
];

fn assert_honest(dr: &Draft) {
    let text = format!(
        "{} {} {} {} {}",
        dr.title, dr.observation, dr.interpretation, dr.limitations, dr.action
    )
    .to_lowercase();
    for f in FORBIDDEN {
        // "does not show that one caused the other" is the required disclaimer, not a claim.
        let cleaned = text.replace("that one caused the other", "");
        assert!(!cleaned.contains(f), "{}: contains “{f}”: {text}", dr.kind);
    }
    assert!(!dr.evidence.is_empty(), "{} has evidence", dr.kind);
    assert!(dr.evidence.iter().any(|e| e.role == "observation"));
    assert!(dr.period.0 <= dr.period.1);
    assert!((0.0..=1.0).contains(&dr.confidence) && !dr.confidence_basis.is_empty());
    let eval = dr.evaluation.to_string();
    super::super::amara_store::check_evaluation(&eval)
        .unwrap_or_else(|e| panic!("{}: {e}", dr.kind));
}

// ───────────────────────────── each domain alone ─────────────────────────────

#[test]
fn workout_volume_rule_needs_history_and_a_real_jump() {
    let mut p = base();
    p.workouts = vec![
        w("a", "2026-10-31", 8),
        w("b", "2026-11-07", 8),
        w("c", "2026-11-14", 8),
        w("d", "2026-11-25", 30),
    ];
    let dr = training_volume(&p).expect("30 sets against 8 a week");
    assert_eq!(dr.kind, "training_volume_rising");
    assert_eq!(dr.evaluation["direction"], "at_most");
    assert_eq!(dr.evaluation["target"], 9.0);
    assert_eq!(dr.evaluation["measure_after"], "2026-12-03");
    assert!(dr.limitations.contains("not a readiness"));
    assert_honest(&dr);
    // Steady training: nothing.
    p.workouts[3].done_sets = 10;
    assert!(training_volume(&p).is_none());
    // A single earlier week is not enough history.
    p.workouts = vec![w("c", "2026-11-14", 8), w("d", "2026-11-25", 30)];
    assert!(
        training_volume(&p).is_none(),
        "one earlier week is too little"
    );
}

fn day_row(day: &str, sleep_h: f64, rec: f64) -> DailyRow {
    DailyRow {
        day: day.into(),
        sleep_total_min: Some(sleep_h * 60.0),
        recovery: Some(rec),
        ..Default::default()
    }
}

#[test]
fn whoop_and_check_ins_compare_without_claiming_cause_or_readiness() {
    let mut p = base();
    // Without the WHOOP table there is no comparison at all.
    assert!(sleep_energy(&p).is_none());
    let mut days = Vec::new();
    let mut cis = Vec::new();
    for i in 0..7 {
        let prev = d("2026-11-13") + Duration::days(i);
        let cur = d("2026-11-20") + Duration::days(i);
        days.push(day_row(&ymd(prev), 7.6, 70.0));
        days.push(day_row(&ymd(cur), 6.4, 52.0));
        cis.push(ci(&format!("p{i}"), &ymd(prev), 4, 4, &[]));
        cis.push(ci(&format!("c{i}"), &ymd(cur), 3, 2, &[]));
    }
    p.days = Some(days.clone());
    p.checkins = cis.clone();
    let dr = sleep_energy(&p).expect("both dropped");
    assert_eq!(dr.kind, "sleep_energy_down_together");
    assert!(dr.interpretation.contains(CORRELATION_LIMIT));
    assert!(dr.limitations.contains(BIOMETRIC_LIMIT));
    assert!(dr
        .evidence
        .iter()
        .any(|e| e.record_ref.starts_with("daily:") && e.quality == "measured"));
    assert!(dr
        .evidence
        .iter()
        .any(|e| e.record_ref.starts_with("checkin:") && e.quality == "self_report"));
    assert_honest(&dr);
    // Sleep steady, energy down: a different finding, still no cause.
    p.days = Some(days.iter().map(|r| day_row(&r.day, 7.5, 68.0)).collect());
    let dr = sleep_energy(&p).expect("energy fell, sleep held");
    assert_eq!(dr.kind, "energy_down_sleep_steady");
    assert_honest(&dr);
    // Too few check-ins in a week: unknown, nothing said.
    p.checkins = cis
        .into_iter()
        .filter(|c| c.id.starts_with('p') || c.id == "c0")
        .collect();
    assert!(sleep_energy(&p).is_none());
}

fn date(id: &str, day: &str, goal: Option<&str>) -> crate::commands::life_dates::LifeDateRecord {
    crate::commands::life_dates::LifeDateRecord {
        id: id.into(),
        title: format!("Date {id}"),
        date: day.into(),
        lane: "personal".into(),
        sub: String::new(),
        lead_days: vec![],
        done: false,
        goal_id: goal.map(str::to_string),
    }
}

#[test]
fn life_workload_against_training_and_commitments_against_the_priority() {
    let mut p = base();
    p.dates = vec![
        date("d1", "2026-11-21", None),
        date("d2", "2026-11-24", None),
    ];
    p.checkins = vec![
        ci("c1", "2026-11-23", 3, 3, &["work"]),
        ci("c2", "2026-11-25", 3, 3, &["Busy"]),
    ];
    p.workouts = vec![
        w("a", "2026-11-14", 8),
        w("b", "2026-11-16", 8),
        w("c", "2026-11-22", 8),
    ];
    let dr = workload_training(&p).expect("4 workload items against 0, 1 session against 2");
    assert_eq!(dr.evaluation["metric"], "workout.sessions_7d");
    assert_eq!(dr.evaluation["target"], 2.0);
    assert_honest(&dr);
    // Commitments: needs a priority this week.
    p.dates = vec![
        date("e1", "2026-11-27", None),
        date("e2", "2026-11-28", None),
        date("e3", "2026-11-28", None),
        date("e4", "2026-11-30", Some("g1")),
    ];
    assert!(commitment_conflict(&p).is_none(), "no plan, no priority");
    p.plan = Some(crate::commands::life_plan::WeekPlan {
        week_start: "2026-11-23".into(),
        priority: Some("Akiya offer".into()),
        priority_goal_id: Some("g1".into()),
        actions: vec![crate::commands::life_plan::PlannedAction {
            text: "Call the agent".into(),
            goal_id: Some("g1".into()),
            done: false,
        }],
        note: None,
        updated_at: Some("2026-11-23T08:00:00+09:00".into()),
    });
    let dr = commitment_conflict(&p).expect("3 unlinked dates");
    assert!(
        dr.observation.contains("3 commitments"),
        "{}",
        dr.observation
    );
    assert!(
        dr.action.contains("2026-11-28"),
        "busiest day named: {}",
        dr.action
    );
    assert_eq!(dr.evaluation["measure_after"], "2026-11-29");
    assert_honest(&dr);
    // All planned actions done: no conflict raised.
    p.plan.as_mut().unwrap().actions[0].done = true;
    assert!(commitment_conflict(&p).is_none());
}

fn habit_goal() -> crate::commands::goals::Goal {
    serde_json::from_value(json!({"id":"h1","title":"Meditate","category":"habit","target":7,"current":0,"unit":"days","deadline":null,
        "life":{"frequency":{"times":5,"per":"week"},"minimumVersion":"one breath"}}))
    .unwrap()
}

fn hd(day: &str, done: bool) -> crate::commands::habits::HabitDayRow {
    crate::commands::habits::HabitDayRow {
        habit_id: "h1".into(),
        day: day.into(),
        done,
        source: "manual".into(),
        at: format!("{day}T21:00:00+09:00"),
    }
}

#[test]
fn repeated_obstacles_come_from_real_history_and_unknown_days_stay_unknown() {
    let mut p = base();
    p.goals = vec![habit_goal()];
    // Wednesdays 11-04, 11-11, 11-18 and 11-25 missed; Mondays done.
    p.habit_days = Some(vec![
        hd("2026-11-04", false),
        hd("2026-11-11", false),
        hd("2026-11-18", false),
        hd("2026-11-25", false),
        hd("2026-11-02", true),
        hd("2026-11-09", true),
        hd("2026-11-16", true),
    ]);
    p.checkins = vec![
        ci("t1", "2026-11-03", 2, 2, &["travel"]),
        ci("t2", "2026-11-10", 3, 1, &["travel"]),
        ci("t3", "2026-11-19", 2, 3, &["travel"]),
        ci("t4", "2026-11-20", 4, 4, &["gym"]),
    ];
    let all = repeated_obstacles(&p);
    let kinds: Vec<_> = all.iter().map(|x| x.kind).collect();
    assert_eq!(kinds, ["habit_missed_same_weekday", "tag_with_low_energy"]);
    assert!(
        all[0].title.contains("4 of the last 4 Wednesdays"),
        "{}",
        all[0].title
    );
    assert!(all[0].action.contains("one breath"));
    assert_eq!(all[0].evidence.len(), 4);
    for x in &all {
        assert_honest(x);
    }
    // Only 2 marked Wednesdays: unknown, so nothing.
    p.habit_days = Some(vec![hd("2026-11-18", false), hd("2026-11-25", false)]);
    p.checkins.clear();
    assert!(repeated_obstacles(&p).is_empty());
}

#[test]
fn goal_trajectories_and_the_behind_pace_rule() {
    let mut p = base();
    p.goals = vec![
        serde_json::from_value(json!({"id":"g1","title":"Akiya offer","category":"personal","target":10,"current":1,"unit":"steps","deadline":"2027-01-31","created_at":"2026-09-01T00:00:00+09:00"})).unwrap(),
        serde_json::from_value(json!({"id":"g2","title":"Read 12 books","category":"personal","target":12,"current":11,"unit":"books","deadline":"2026-12-31","created_at":"2026-01-01T00:00:00+09:00"})).unwrap(),
        serde_json::from_value(json!({"id":"g3","title":"No deadline","category":"personal","target":5,"current":1,"unit":"x","deadline":null})).unwrap(),
        serde_json::from_value(json!({"id":"b1","title":"Biz goal","category":"business","target":5,"current":0,"unit":"x","deadline":"2026-12-31","created_at":"2026-09-01"})).unwrap(),
    ];
    let t = super::summaries::trajectories(&p);
    let state = |id: &str| t.iter().find(|x| x.goal_id == id).map(|x| x.state.clone());
    assert_eq!(state("g1").as_deref(), Some("behind"));
    assert_eq!(state("g2").as_deref(), Some("on_pace"));
    assert_eq!(state("g3").as_deref(), Some("no_deadline"));
    assert_eq!(state("b1"), None, "business goals stay in Enterprise");
    let drafts = goals_behind(&p);
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].target_ref.as_deref(), Some("goals:g1"));
    assert_honest(&drafts[0]);
}

#[test]
fn summaries_say_what_is_missing_and_never_show_zero_for_unknown() {
    let mut p = base();
    p.missing = vec!["WHOOP daily table (NOOP)".into()];
    let b = super::summaries::daily(&p, &[], "t", "manual");
    let sec = |id: &str| b.sections.iter().find(|s| s.id == id).unwrap().clone();
    assert_eq!(sec("body").state, "unavailable");
    assert!(sec("today")
        .note
        .unwrap()
        .contains("No calendar is connected"));
    assert_eq!(sec("money").state, "unavailable");
    assert_eq!(
        sec("habits").state,
        "unavailable",
        "the habit log wasn't read"
    );
    assert_eq!(b.missing, ["WHOOP daily table (NOOP)"]);
    assert!(b.method.contains("No language model"));
    // Training: WHOOP minutes stay None without a table; with one they are summed separately.
    p.workouts = vec![w("a", "2026-11-25", 10)];
    assert_eq!(
        super::summaries::training(&p, p.today).detected_workout_min,
        None
    );
    p.days = Some(vec![DailyRow {
        day: "2026-11-25".into(),
        workout_min: Some(31.0),
        strain: Some(12.0),
        ..Default::default()
    }]);
    let t = super::summaries::training(&p, p.today);
    assert_eq!(
        (t.sessions, t.done_sets, t.detected_workout_min),
        (1, 10, Some(31.0))
    );
    assert!(
        t.source_ids.contains(&"workout:a".to_string())
            && t.source_ids.contains(&"daily:2026-11-25".to_string())
    );
    let n = super::summaries::neuro(&p, p.today, 7);
    assert_eq!(n.sessions, 0);
    assert!(n.note.contains("not a measure of mood"));
}

// ───────────────────────────── cross-domain flows on real files ─────────────────────────────

fn session_json(id: &str, day: &str, sets: u32) -> serde_json::Value {
    let logs: Vec<_> = (0..sets)
        .map(|i| json!({"item":0,"set":i,"exerciseId":"knee-push-up","outcome":"done","reps":10,"holdS":null,"loadKg":null,
                         "finishedAt":format!("{day}T08:{:02}:00+09:00", i % 60),"restBeforeS":null}))
        .collect();
    json!({"id":id,"templateId":"t1","templateName":"Push","source":"user","sourceRef":null,
           "plan":[{"exerciseId":"knee-push-up","sets":sets,"reps":10,"restS":60}],"restBetweenS":90,"state":"completed",
           "plannedAt":format!("{day}T07:59:00+09:00"),"startedAt":format!("{day}T08:00:00+09:00"),"endedAt":format!("{day}T08:40:00+09:00"),
           "pauses":[],"sets":logs,"nextItem":1,"nextSet":0,"engine":"workout-engine-v1-0062"})
}

/// Real files for every personal domain the rules read.
fn world(data: &Path) {
    let write = |rel: &str, v: serde_json::Value| {
        let p = data.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, v.to_string()).unwrap();
    };
    write(
        "goals.json",
        json!([
            {"id":"g1","title":"Akiya offer","category":"personal","target":10,"current":1,"unit":"steps","deadline":"2027-01-31","created_at":"2026-09-01T00:00:00+09:00"},
            {"id":"h1","title":"Meditate","category":"habit","target":7,"current":0,"unit":"days","deadline":null,"life":{"frequency":{"times":5,"per":"week"}}}
        ]),
    );
    let marks: Vec<_> = ["2026-11-04", "2026-11-11", "2026-11-18", "2026-11-25"]
        .iter()
        .map(|x| json!({"habitId":"h1","day":x,"done":false,"source":"manual","at":format!("{x}T21:00:00+09:00")}))
        .collect();
    write("habits/days.json", json!({ "days": marks }));
    write(
        "checkins/checkins.json",
        json!({"entries": [
            {"id":"c1","at":"2026-11-23T21:00:00+09:00","day":"2026-11-23","mood":3,"energy":2,"tags":["work"],"text":"long day"},
            {"id":"c2","at":"2026-11-24T21:00:00+09:00","day":"2026-11-24","mood":2,"energy":2,"tags":["work"],"text":""},
            {"id":"c3","at":"2026-11-25T21:00:00+09:00","day":"2026-11-25","mood":2,"energy":1,"tags":["work"],"text":""}
        ]}),
    );
    write(
        "life/dates.json",
        json!({"dates": [
            {"id":"d1","title":"Visa renewal","date":"2026-11-27","lane":"residence"},
            {"id":"d2","title":"Tax form","date":"2026-11-28","lane":"personal"},
            {"id":"d3","title":"Dentist","date":"2026-11-28","lane":"personal"}
        ]}),
    );
    write(
        "life/week_plan.json",
        json!({"weeks": [{"weekStart":"2026-11-23","priority":"Akiya offer","priorityGoalId":"g1",
        "actions":[{"text":"Call the agent","goalId":"g1","done":false}],"updatedAt":"2026-11-23T08:00:00+09:00"}]}),
    );
    write(
        "workout/sessions.json",
        json!({"sessions": [
            session_json("w1", "2026-10-31", 8), session_json("w2", "2026-11-07", 8),
            session_json("w3", "2026-11-14", 4), session_json("w4", "2026-11-16", 4),
            session_json("w5", "2026-11-25", 30)
        ]}),
    );
}

fn personal_recs(data: &Path) -> Vec<RecommendationRow> {
    store_for(data, Scope::Personal)
        .list_recommendations(Scope::Personal, None, 200)
        .unwrap()
        .into_iter()
        .filter(|r| r.produced_by == PRODUCER)
        .collect()
}

#[test]
fn the_personal_flow_writes_complete_recommendations_once_and_a_briefing() {
    let t = tmp("flow");
    world(&t.0);
    let b = build_personal(&t.0, now(), "manual").unwrap();
    let recs = personal_recs(&t.0);
    let mut kinds: Vec<_> = recs.iter().map(|r| r.kind.clone()).collect();
    kinds.sort();
    assert_eq!(
        kinds,
        [
            "commitments_vs_priority",
            "goal_behind_pace",
            "habit_missed_same_weekday",
            "tag_with_low_energy",
            "training_volume_rising",
            "workload_up_training_down"
        ],
        "every personal domain fed a rule"
    );
    let store = store_for(&t.0, Scope::Personal);
    for r in &recs {
        // The six parts, stored.
        assert!(r.observation.is_some() && r.rationale.is_some() && r.uncertainty.is_some());
        assert!(
            r.suggested_action.is_some() && r.confidence.is_some() && r.confidence_basis.is_some()
        );
        assert!(r.evaluation_json.is_some() && r.period_start.is_some() && r.period_end.is_some());
        let d = store
            .get_recommendation(&r.recommendation_id)
            .unwrap()
            .unwrap();
        assert!(!d.evidence.is_empty());
        for e in &d.evidence {
            assert!(e.window_start.is_some() && e.window_end.is_some());
            // Real, resolvable records carry the resolver's hash (daily rows can't be checked).
            if !e.record_ref.starts_with("daily:") {
                assert!(e.snapshot_sha256.is_some(), "{} resolved", e.record_ref);
            }
        }
    }
    // Commitments conflict cites the three real Life dates and the goal.
    let c = recs
        .iter()
        .find(|r| r.kind == "commitments_vs_priority")
        .unwrap();
    let ev = store
        .get_recommendation(&c.recommendation_id)
        .unwrap()
        .unwrap()
        .evidence;
    assert_eq!(
        ev.iter()
            .filter(|e| e.record_ref.starts_with("date:"))
            .count(),
        3
    );
    // The briefing.
    assert_eq!(b.day, "2026-11-26");
    assert_eq!(b.trigger, "manual");
    assert_eq!(b.open_recommendations.len(), 6);
    assert!(
        b.missing.iter().any(|m| m.contains("WHOOP")),
        "no daily table in the temp folder"
    );
    assert!(t.0.join("briefings/daily/2026-11-26.json").exists());
    let rt = runtime_in(&t.0, now());
    assert!(!rt.runs_while_closed);
    let job = rt.jobs.iter().find(|j| j.id == "personal").unwrap();
    assert_eq!(job.state.rule_counts.get("created"), Some(&6));
    assert_eq!(job.state.last_result.as_deref(), Some("partial"));
    // Built again: every finding is suppressed, none duplicated.
    build_personal(&t.0, now() + Duration::hours(2), "manual").unwrap();
    assert_eq!(personal_recs(&t.0).len(), 6);
    let job = runtime_in(&t.0, now())
        .jobs
        .into_iter()
        .find(|j| j.id == "personal")
        .unwrap();
    assert_eq!(job.state.rule_counts.get("suppressed"), Some(&6));
    // Nothing personal reached the business file.
    let biz = store_for(&t.0, Scope::Business)
        .list_recommendations(Scope::Business, None, 50)
        .unwrap();
    assert!(biz.is_empty());
}

#[test]
fn a_dismissed_finding_stays_quiet_and_a_due_evaluation_is_measured_once() {
    let t = tmp("measure");
    world(&t.0);
    build_personal(&t.0, now(), "manual").unwrap();
    let recs = personal_recs(&t.0);
    let vol = recs
        .iter()
        .find(|r| r.kind == "training_volume_rising")
        .unwrap();
    let habit = recs
        .iter()
        .find(|r| r.kind == "habit_missed_same_weekday")
        .unwrap();
    let store = store_for(&t.0, Scope::Personal);
    store
        .add_outcome(
            &habit.recommendation_id,
            "dismissed",
            "taisun",
            None,
            None,
            None,
            None,
        )
        .unwrap();
    store
        .add_outcome(
            &vol.recommendation_id,
            "accepted",
            "taisun",
            None,
            None,
            None,
            None,
        )
        .unwrap();
    // A week later: 12 sets logged since; the volume evaluation (at most 9) is due.
    let path = t.0.join("workout/sessions.json");
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v["sessions"]
        .as_array_mut()
        .unwrap()
        .push(session_json("w6", "2026-12-01", 6));
    std::fs::write(&path, v.to_string()).unwrap();
    let later = at("2026-12-03", 8);
    build_personal(&t.0, later, "timer").unwrap();
    let d = store
        .get_recommendation(&vol.recommendation_id)
        .unwrap()
        .unwrap();
    let m: Vec<_> = d
        .outcomes
        .iter()
        .filter(|o| o.outcome == "measured")
        .collect();
    assert_eq!(m.len(), 1);
    // 2026-11-27..12-03 holds only w6 (6 sets) → met (at most 9).
    assert_eq!(m[0].evaluation.as_deref(), Some("met"));
    assert!(m[0].note.as_deref().unwrap().contains("Measured 6 sets"));
    assert_eq!(
        d.recommendation.status, "accepted",
        "a measurement never moves the status"
    );
    // The habit finding (same habit, same weekday) was dismissed: not raised again.
    assert_eq!(
        personal_recs(&t.0)
            .iter()
            .filter(|r| r.kind == "habit_missed_same_weekday")
            .count(),
        1
    );
    // Measured once only.
    build_personal(&t.0, later + Duration::hours(3), "manual").unwrap();
    let d = store
        .get_recommendation(&vol.recommendation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        d.outcomes
            .iter()
            .filter(|o| o.outcome == "measured")
            .count(),
        1
    );
}

#[test]
fn the_week_view_adds_to_the_review_and_lists_that_weeks_findings() {
    let t = tmp("week");
    world(&t.0);
    build_personal(&t.0, now(), "manual").unwrap();
    let wk = week_in(&t.0, "2026-11-23", now()).unwrap();
    assert_eq!(
        (wk.week_start.as_str(), wk.week_end.as_str()),
        ("2026-11-23", "2026-11-29")
    );
    assert_eq!(wk.training.sessions, 1);
    assert_eq!(wk.training.done_sets, 30);
    assert!(!wk.recommendations.is_empty());
    assert!(week_in(&t.0, "2026-11-24", now()).is_err(), "a Monday only");
}

#[test]
fn enterprise_reads_business_only_and_writes_business_recommendations_only() {
    let t = tmp("ent");
    world(&t.0);
    let ws = crate::commands::business_workspace::store_in(&t.0);
    let biz = ws.list_businesses().unwrap();
    let id = biz[0].business_id.clone();
    let tenant = ws.tenant(&id).unwrap();
    let item = tenant
        .context_save(
            &crate::commands::business_workspace::BusinessContextInput {
                item_id: None,
                section: "objectives".into(),
                title: "Launch the cleanser".into(),
                body: "By spring".into(),
                status: "draft".into(),
                expected_version: None,
            },
            "taisun",
            "user",
            None,
        )
        .unwrap();
    tenant
        .context_set_status(&item.item_id, "approved", item.version, "taisun")
        .unwrap();
    // Work recorded in the weeks before.
    let bs = store_for(&t.0, Scope::Business);
    for (i, day) in ["2026-11-10", "2026-11-12", "2026-11-17"]
        .iter()
        .enumerate()
    {
        bs.record_activity(&super::super::amara_store::NewActivity {
            scope: Scope::Business,
            certainty: "confirmed",
            kind: "business.document_added",
            subject_ref: &format!("biz:{id}/document/doc{i}"),
            occurred_at: &format!("{day}T10:00:00+09:00"),
            source: "user",
            source_ref: None,
            actor: "taisun",
            summary: None,
            data_json: None,
        })
        .unwrap();
    }
    let fixed = at("2026-11-29", 8);
    let briefs = build_enterprise(&t.0, fixed, "manual").unwrap();
    assert_eq!(briefs.len(), 1);
    let b = &briefs[0];
    assert_eq!(b.business_id, id);
    let sec = |s: &str| b.sections.iter().find(|x| x.id == s).unwrap().clone();
    assert_eq!(sec("search_needs").state, "unavailable");
    assert!(sec("search_needs")
        .note
        .unwrap()
        .contains("Search Console is deferred"));
    assert!(sec("objectives")
        .lines
        .iter()
        .any(|l| l.text == "Launch the cleanser"));
    // The objective approval itself is business activity "now" (2026-11-27 or later in real time),
    // so whether the execution rule fires depends on the wall clock; check isolation either way.
    let business_recs = bs.list_business_recommendations(&id, 50).unwrap();
    for r in &business_recs {
        assert_eq!(r.scope, "business");
        let dd = bs
            .get_recommendation(&r.recommendation_id)
            .unwrap()
            .unwrap();
        assert!(dd.evidence.iter().all(|e| e.record_ref.starts_with("biz:")));
    }
    // Personal flows never write into the business file, and the business flow never into the personal one.
    let before = personal_recs(&t.0).len();
    build_enterprise(&t.0, fixed + Duration::hours(1), "manual").unwrap();
    assert_eq!(personal_recs(&t.0).len(), before);
    assert!(t
        .0
        .join(format!("business/briefings/{id}/2026-11-29.json"))
        .exists());
}

#[test]
fn business_rules_on_their_own() {
    let mut b = super::enterprise::Business {
        business_id: "taiyo-skin".into(),
        name: "Taiyo Skin".into(),
        today: d("2026-11-26"),
        objectives: vec![("o1".into(), "Launch the cleanser".into())],
        ..Default::default()
    };
    let act = |kind: &str, day: &str| super::super::amara_store::commands::ActivityRow {
        activity_id: uuid::Uuid::new_v4().to_string(),
        scope: "business".into(),
        kind: kind.into(),
        subject_ref: "biz:taiyo-skin/document/x".into(),
        occurred_at: format!("{day}T10:00:00+09:00"),
        recorded_at: String::new(),
        source: "user".into(),
        provenance: "manual".into(),
        certainty: "confirmed".into(),
        source_ref: None,
        actor: "taisun".into(),
        summary: None,
        data_json: None,
    };
    // No history: unknown, not idle.
    assert!(super::enterprise::objectives_vs_execution(&b).is_none());
    b.activity = vec![
        act("business.document_added", "2026-11-10"),
        act("business.sync_finished", "2026-11-25"),
    ];
    let dr = super::enterprise::objectives_vs_execution(&b)
        .expect("work before, none this week (a sync isn't work)");
    assert_eq!(dr.domain, "enterprise");
    assert!(dr
        .evidence
        .iter()
        .all(|e| e.record_ref.starts_with("biz:taiyo-skin/")));
    assert_eq!(dr.target_ref.as_deref(), Some("biz:taiyo-skin/business"));
    assert_honest(&dr);
    b.activity
        .push(act("business.context_changed", "2026-11-24"));
    assert!(super::enterprise::objectives_vs_execution(&b).is_none());
    // Search against needs: unavailable without search data; compared once both exist.
    assert!(super::enterprise::search_vs_needs(&b)
        .unwrap_err()
        .contains("deferred"));
    let report = |name: &str, id: &str, rows: Vec<(&str, f64)>| {
        super::super::business_workspace::intelligence::BusinessReport {
            report: name.into(),
            generated_at: String::new(),
            insights: vec![
                super::super::business_workspace::intelligence::BusinessInsight {
                    id: id.into(),
                    group: String::new(),
                    title: String::new(),
                    kind: "measured".into(),
                    status: "available".into(),
                    statement: String::new(),
                    value: None,
                    unit: None,
                    n: Some(40),
                    rows: rows
                        .into_iter()
                        .map(|(l, s)| {
                            super::super::business_workspace::intelligence::BusinessInsightRow {
                                label: l.into(),
                                value: 1.0,
                                share: Some(s),
                            }
                        })
                        .collect(),
                    window: Some("2026-10-01 to 2026-11-20".into()),
                    sources: vec![],
                    note: None,
                },
            ],
            notes: vec![],
        }
    };
    b.website = Some(report(
        "website",
        "search.queries",
        vec![("acne cleanser", 0.5), ("sunscreen", 0.3)],
    ));
    b.customers = Some(report(
        "customers",
        "cust.concerns",
        vec![("Acne", 0.4), ("Dryness", 0.35), ("Redness", 0.05)],
    ));
    let dr = super::enterprise::search_vs_needs(&b)
        .unwrap()
        .expect("dryness isn't searched");
    assert!(
        dr.observation.contains("Dryness")
            && !dr.observation.contains("Acne")
            && !dr.observation.contains("Redness")
    );
    assert_honest(&dr);
}

#[test]
fn the_timer_builds_only_when_due_and_catches_up_once() {
    let t = tmp("timer");
    world(&t.0);
    // 06:59: nothing is due.
    tick(&t.0, at("2026-11-26", 6), "catch_up");
    assert!(!t.0.join("briefings/daily/2026-11-26.json").exists());
    assert!(is_due(now(), PERSONAL_SLOT, None));
    assert!(!is_due(now(), ENTERPRISE_SLOT, Some("2026-11-26")));
    // 08:00 first tick after launch: catch-up builds both.
    tick(&t.0, now(), "catch_up");
    let rt = runtime_in(&t.0, now());
    let p = rt.jobs.iter().find(|j| j.id == "personal").unwrap();
    assert_eq!(p.state.last_trigger.as_deref(), Some("catch_up"));
    assert_eq!(p.state.last_day.as_deref(), Some("2026-11-26"));
    assert!(p.next_due_at.starts_with("2026-11-27T07:00:00"));
    let e = rt.jobs.iter().find(|j| j.id == "enterprise").unwrap();
    assert_eq!(e.state.last_day.as_deref(), Some("2026-11-26"));
    // The next tick the same day does nothing.
    let before = std::fs::metadata(t.0.join("briefings/daily/2026-11-26.json"))
        .unwrap()
        .modified()
        .unwrap();
    tick(&t.0, now() + Duration::minutes(1), "timer");
    let after = std::fs::metadata(t.0.join("briefings/daily/2026-11-26.json"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);
    assert!(rt.note.contains("When it is closed nothing runs"));
    assert!(rt.model_inference.starts_with("None"));
    // Manual refresh is rate-limited.
    assert!(manual_ok(&t.0, "personal", now() + Duration::seconds(30)).is_err());
    assert!(manual_ok(&t.0, "personal", now() + Duration::minutes(2)).is_ok());
}

#[test]
fn opening_before_the_slot_shows_yesterdays_briefing_and_after_it_builds_todays() {
    let t = tmp("open");
    world(&t.0);
    build_personal(&t.0, at("2026-11-25", 9), "timer").unwrap();
    let early = read_personal(&t.0, at("2026-11-26", 6)).unwrap();
    assert_eq!(early.day, "2026-11-25");
    let late = read_personal(&t.0, now()).unwrap();
    assert_eq!(
        (late.day.as_str(), late.trigger.as_str()),
        ("2026-11-26", "opened")
    );
}

#[test]
fn every_rule_output_on_the_seeded_world_is_honest() {
    let t = tmp("honest");
    world(&t.0);
    let p = super::inputs::gather(&t.0, now());
    let drafts = personal_drafts(&p);
    assert!(drafts.len() >= 6);
    for dr in &drafts {
        assert_honest(dr);
    }
    let fps: std::collections::BTreeSet<_> = drafts.iter().map(|x| x.fingerprint()).collect();
    assert_eq!(
        fps.len(),
        drafts.len(),
        "distinct findings have distinct fingerprints"
    );
}
