//! 0053 store tests: real SQLite files in a temp folder; checks rows on disk, not just returns.

use super::*;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmp(tag: &str) -> Tmp {
    let p = std::env::temp_dir().join(format!("nexus-amara-store-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    Tmp(p)
}

fn habit_done<'a>(subject: &'a str, at: &'a str) -> NewActivity<'a> {
    NewActivity {
        scope: Scope::Personal,
        kind: "habit.completed",
        subject_ref: subject,
        occurred_at: at,
        source: "user",
        source_ref: None,
        actor: "taisun",
        summary: Some("Meditation done"),
        data_json: Some(r#"{"streak":12}"#),
    }
}

fn rec<'a>(scope: Scope, kind: &'a str) -> NewRecommendation<'a> {
    NewRecommendation {
        id: None,
        scope,
        domain: "life",
        kind,
        title: "Move meditation to mornings",
        body: "You completed it 9/10 mornings vs 3/10 evenings.",
        expires_at: None,
        produced_by: "rules:habit_timing",
        model_tier: None,
        confidence: Some(0.7),
        confidence_basis: Some("n=20 days"),
        target_ref: Some("goals:g-1"),
        supersedes_id: None,
    }
}

fn obs<'a>(r: &'a str) -> NewEvidence<'a> {
    NewEvidence {
        record_ref: r,
        window_start: Some("2026-11-01T00:00:00Z"),
        window_end: Some("2026-11-20T00:00:00Z"),
        role: "observation",
        quality: "self_report",
        note: None,
        snapshot_sha256: None,
    }
}

#[test]
fn init_creates_both_files_once_logs_it_and_is_idempotent() {
    let t = tmp("init");
    init_on_startup(&t.0);
    init_on_startup(&t.0);
    assert!(t.0.join("amara.db").is_file());
    assert!(t.0.join("business/amara_business.db").is_file());
    let log = std::fs::read_to_string(t.0.join("migrations/log.jsonl")).unwrap();
    assert_eq!(
        log.lines().count(),
        2,
        "one line per file, first creation only"
    );
    use std::os::unix::fs::PermissionsExt;
    for f in [
        "amara.db",
        "business/amara_business.db",
        "migrations/log.jsonl",
    ] {
        assert_eq!(
            std::fs::metadata(t.0.join(f)).unwrap().permissions().mode() & 0o777,
            0o600,
            "{f}"
        );
    }
}

#[test]
fn activity_is_idempotent_append_only_and_scope_checked() {
    let t = tmp("activity");
    let s = store_for(&t.0, Scope::Personal);
    let a = habit_done("habit_day:g-1:2026-11-20", "2026-11-20T07:00:00+09:00");
    let first = s.record_activity(&a).unwrap();
    let again = s.record_activity(&a).unwrap();
    let Recorded::Inserted(id) = first else {
        panic!()
    };
    assert_eq!(again, Recorded::Duplicate(id));
    let conn = s.open().unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    assert!(conn.execute("UPDATE activity SET summary='x'", []).is_err());
    // Business never lands in the personal file, and a business subject is refused here.
    let mut b = habit_done("ledger:venture:v1", "2026-11-20T07:00:00Z");
    b.scope = Scope::Business;
    assert!(s.record_activity(&b).is_err());
    let mut b2 = habit_done("ledger:venture:v1", "2026-11-20T07:00:00Z");
    b2.kind = "business.action_done";
    assert!(s
        .record_activity(&b2)
        .unwrap_err()
        .contains("another boundary"));
    // The DB itself refuses a business row in the personal file.
    assert!(conn
        .execute(
            "INSERT INTO activity VALUES ('x','business','habit.completed','goals:g','t','t','user',NULL,'taisun',NULL,NULL,1)",
            [],
        )
        .is_err());
    // Unknown kinds, sources and refs are refused.
    let mut bad = habit_done("habit_day:g-1:2026-11-21", "2026-11-21T07:00:00Z");
    bad.kind = "anything.goes";
    assert!(s.record_activity(&bad).is_err());
    assert!(s
        .record_activity(&habit_done("shell:rm", "2026-11-21T07:00:00Z"))
        .is_err());
}

#[test]
fn a_recommendation_needs_observed_evidence_and_respects_scopes() {
    let t = tmp("reco");
    let s = store_for(&t.0, Scope::Personal);
    // No observation: refused.
    assert!(s
        .create_recommendation(&rec(Scope::Personal, "habit_nudge"), &[])
        .unwrap_err()
        .contains("observation"));
    // Personal + neural evidence: allowed (cross-domain).
    let id = s
        .create_recommendation(
            &rec(Scope::Personal, "habit_nudge"),
            &[obs("habit_day:g-1:2026-11-20"), {
                let mut e = obs("replay:eeg_20261106_073914#t=120..300");
                e.quality = "measured";
                e
            }],
        )
        .unwrap();
    let conn = s.open().unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM recommendation_evidence WHERE recommendation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2);
    // Business evidence never supports a personal recommendation (and nothing is written).
    let before: i64 = conn
        .query_row("SELECT COUNT(*) FROM recommendation", [], |r| r.get(0))
        .unwrap();
    let e = s
        .create_recommendation(
            &rec(Scope::Personal, "habit_nudge"),
            &[obs("goals:g-1"), obs("treasury:income:i-1")],
        )
        .unwrap_err();
    assert!(e.contains("can't support"), "{e}");
    let after: i64 = conn
        .query_row("SELECT COUNT(*) FROM recommendation", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    // Telemetry evidence must name its window.
    let mut no_window = obs("noop:daily:2026-11-20");
    no_window.window_start = None;
    assert!(s
        .create_recommendation(&rec(Scope::Personal, "x"), &[no_window])
        .unwrap_err()
        .contains("window"));
    // Confidence without a basis is refused.
    let mut r = rec(Scope::Personal, "x");
    r.confidence_basis = None;
    assert!(s.create_recommendation(&r, &[obs("goals:g-1")]).is_err());
}

#[test]
fn the_business_file_holds_business_only() {
    let t = tmp("biz");
    let b = store_for(&t.0, Scope::Business);
    let mut r = rec(Scope::Business, "business_opportunity");
    r.domain = "enterprise";
    r.target_ref = None;
    b.create_recommendation(&r, &[obs("treasury:income:i-1")])
        .unwrap();
    assert!(b
        .create_recommendation(&rec(Scope::Personal, "x"), &[obs("goals:g-1")])
        .is_err());
    assert!(b.create_recommendation(&r, &[obs("goals:g-1")]).is_err());
    assert!(t.0.join("business/amara_business.db").is_file());
}

#[test]
fn outcomes_follow_the_status_machine_and_are_append_only() {
    let t = tmp("outcome");
    let s = store_for(&t.0, Scope::Personal);
    let id = s
        .create_recommendation(&rec(Scope::Personal, "habit_nudge"), &[obs("goals:g-1")])
        .unwrap();
    s.add_outcome(&id, "shown", "system", None, None, None, None)
        .unwrap();
    assert!(s
        .add_outcome(&id, "completed", "taisun", None, None, None, None)
        .is_err());
    s.add_outcome(
        &id,
        "modified",
        "taisun",
        Some(r#"{"time":"07:30"}"#),
        None,
        None,
        None,
    )
    .unwrap();
    s.add_outcome(
        &id,
        "completed",
        "taisun",
        None,
        Some("execution:e-1"),
        None,
        None,
    )
    .unwrap();
    assert!(s
        .add_outcome(&id, "evaluated", "taisun", None, None, Some("great"), None)
        .is_err());
    s.add_outcome(
        &id,
        "evaluated",
        "taisun",
        None,
        None,
        Some("helped"),
        Some("12 of 14 mornings"),
    )
    .unwrap();
    let conn = s.open().unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM recommendation WHERE recommendation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "evaluated");
    let outcomes: Vec<String> = conn
        .prepare(
            "SELECT outcome FROM recommendation_outcome WHERE recommendation_id=?1 ORDER BY at",
        )
        .unwrap()
        .query_map([&id], |r| r.get(0))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert_eq!(outcomes, ["shown", "modified", "completed", "evaluated"]);
    assert!(conn
        .execute("UPDATE recommendation_outcome SET outcome='accepted'", [])
        .is_err());
}

#[test]
fn forgetting_a_record_removes_what_points_at_it_and_expires_unsupported_recommendations() {
    let t = tmp("forget");
    let s = store_for(&t.0, Scope::Personal);
    s.record_activity(&habit_done("goals:g-1", "2026-11-20T07:00:00Z"))
        .unwrap();
    let id = s
        .create_recommendation(&rec(Scope::Personal, "habit_nudge"), &[obs("goals:g-1")])
        .unwrap();
    let keep = s
        .create_recommendation(
            &rec(Scope::Personal, "habit_nudge"),
            &[obs("goals:g-1"), obs("checkin:c-1")],
        )
        .unwrap();
    assert_eq!(s.forget("goals:g-1").unwrap(), 3);
    let conn = s.open().unwrap();
    let st = |rid: &str| -> String {
        conn.query_row(
            "SELECT status FROM recommendation WHERE recommendation_id=?1",
            [rid],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(st(&id), "expired");
    assert_eq!(st(&keep), "proposed");
    let note: String = conn
        .query_row(
            "SELECT note FROM recommendation_outcome WHERE recommendation_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(note, "evidence_deleted");
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn record_refs_parse_strictly() {
    let r = RecordRef::parse("replay:eeg_20261106_073914#t=120..300").unwrap();
    assert_eq!(
        (
            r.store.as_str(),
            r.id.as_str(),
            r.window.as_deref(),
            r.scope
        ),
        (
            "replay",
            "eeg_20261106_073914",
            Some("t=120..300"),
            Scope::Neural
        )
    );
    let r = RecordRef::parse("goals:g-1@2026-11-20").unwrap();
    assert_eq!(r.version.as_deref(), Some("2026-11-20"));
    for bad in [
        "goals",
        "goals:",
        "unknown:x",
        "goals:a b",
        "goals:x#",
        "goals:x@",
        "",
    ] {
        assert!(RecordRef::parse(bad).is_err(), "{bad}");
    }
    assert!(evidence_allowed(Scope::Personal, Scope::Neural));
    assert!(!evidence_allowed(Scope::Personal, Scope::Business));
    assert!(!evidence_allowed(Scope::Business, Scope::Personal));
    assert!(!evidence_allowed(Scope::Neural, Scope::Business));
}

#[test]
fn a_newer_schema_is_refused_and_left_untouched() {
    let t = tmp("schema");
    let s = store_for(&t.0, Scope::Personal);
    s.open()
        .unwrap()
        .execute(
            "UPDATE schema_meta SET value='9' WHERE key='schema_version'",
            [],
        )
        .unwrap();
    assert!(s.open().unwrap_err().contains("newer"));
}

// ── Step 4: domain owners record activity after their own successful write ────

fn activity_rows(data: &Path, scope: Scope) -> Vec<(String, String)> {
    let c = store_for(data, scope).open().unwrap();
    let mut st = c
        .prepare("SELECT kind, subject_ref FROM activity ORDER BY recorded_at")
        .unwrap();
    st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect()
}

#[test]
fn goal_saves_record_created_progressed_and_completed() {
    let t = tmp("goals");
    let path = t.0.join("goals.json");
    let mut g: crate::commands::goals::Goal = serde_json::from_value(serde_json::json!({
        "id": "g-1", "title": "Run 10 times", "category": "life", "target": 10.0, "current": 0.0,
        "unit": "runs", "deadline": null
    }))
    .unwrap();
    crate::commands::goals::save_goals_at(&path, vec![g.clone()]).unwrap();
    g.current = 4.0;
    crate::commands::goals::save_goals_at(&path, vec![g.clone()]).unwrap();
    g.current = 10.0;
    crate::commands::goals::save_goals_at(&path, vec![g.clone()]).unwrap();
    // No change: no row.
    crate::commands::goals::save_goals_at(&path, vec![g.clone()]).unwrap();
    let kinds: Vec<String> = activity_rows(&t.0, Scope::Personal)
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(kinds, ["goal.created", "goal.progressed", "goal.completed"]);
}

#[test]
fn a_check_in_records_activity_without_its_text() {
    let t = tmp("checkin");
    let draft: crate::commands::checkin::CheckInDraft = serde_json::from_value(
        serde_json::json!({"mood": 4, "energy": 3, "text": "private words"}),
    )
    .unwrap();
    let r = crate::commands::checkin::save_in(
        &crate::commands::checkin::dir_in(&t.0),
        &draft,
        crate::commands::shared::time::now_jst(),
    )
    .unwrap();
    assert_eq!(
        activity_rows(&t.0, Scope::Personal),
        [("checkin.logged".to_string(), format!("checkin:{}", r.id))]
    );
    let raw = std::fs::read(t.0.join("amara.db")).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("private words"));
}

#[test]
fn a_muse_session_records_neural_activity() {
    let t = tmp("session");
    let s: crate::commands::sessions::RecordedSession = serde_json::from_value(serde_json::json!({
        "id": "abc123", "device_type": "muse", "start": "2026-11-20T07:00:00+09:00",
        "end": "2026-11-20T07:20:00+09:00", "duration_secs": 1200, "data_points": 10, "file_path": "",
        "summary": {"avg_focus": null, "avg_relaxation": null, "avg_hr": null, "max_hr": null, "avg_hrv": null}
    }))
    .unwrap();
    crate::commands::sessions::save_session_in(&t.0.join("sessions"), s, serde_json::json!({}))
        .unwrap();
    let c = store_for(&t.0, Scope::Neural).open().unwrap();
    let (kind, scope): (String, String) = c
        .query_row("SELECT kind, scope FROM activity", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(
        (kind.as_str(), scope.as_str()),
        ("neuro.session_finished", "neural")
    );
}

#[test]
fn takes_become_recommendations_and_their_events_outcomes() {
    let t = tmp("takes");
    let dir = t.0.join("takes");
    let entry: crate::commands::take_log::TakeLogEntry = serde_json::from_value(serde_json::json!({
        "id": "fitness-1", "page": "fitness", "day": "2026-11-20", "generatedAt": 1, "kind": "daily",
        "text": "An easy day would help recovery.", "tier": "light"
    }))
    .unwrap();
    assert!(crate::commands::take_log::append_in(&dir, &entry).unwrap());
    for (id, kind, data) in [
        (
            "e1",
            "take_shown",
            serde_json::json!({"takeId": "fitness-1"}),
        ),
        (
            "e2",
            "take_rating",
            serde_json::json!({"takeId": "fitness-1", "helpful": true}),
        ),
    ] {
        let e: crate::commands::take_events::TakeEvent = serde_json::from_value(serde_json::json!({
            "id": id, "kind": kind, "at": 1_700_000_000_000i64, "page": "fitness", "day": "2026-11-20", "data": data
        }))
        .unwrap();
        crate::commands::take_events::append_in(&dir, &e, 1_700_000_000_000).unwrap();
    }
    let c = store_for(&t.0, Scope::Personal).open().unwrap();
    let status: String = c
        .query_row(
            "SELECT status FROM recommendation WHERE recommendation_id='take:fitness-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "evaluated");
    let outs: Vec<(String, Option<String>)> = c
        .prepare("SELECT outcome, evaluation FROM recommendation_outcome ORDER BY at")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert_eq!(
        outs,
        [
            ("shown".into(), None),
            ("evaluated".into(), Some("helped".into()))
        ]
    );
    // The take log itself is unchanged in role: it still holds the take.
    assert_eq!(crate::commands::take_log::list_in(&dir).len(), 1);
}

#[test]
fn a_business_take_goes_to_the_business_file_only() {
    let t = tmp("biztake");
    let entry: crate::commands::take_log::TakeLogEntry = serde_json::from_value(serde_json::json!({
        "id": "business-1", "page": "business", "day": "2026-11-20", "generatedAt": 1, "kind": "daily",
        "text": "Revenue is flat."
    }))
    .unwrap();
    crate::commands::take_log::append_in(&t.0.join("takes"), &entry).unwrap();
    let biz = store_for(&t.0, Scope::Business).open().unwrap();
    let n: i64 = biz
        .query_row("SELECT COUNT(*) FROM recommendation", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    let personal = store_for(&t.0, Scope::Personal).open().unwrap();
    let n: i64 = personal
        .query_row("SELECT COUNT(*) FROM recommendation", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn habit_money_and_date_owners_record_after_their_write() {
    // Source pins: each hook sits after the owner's successful write, with the right kind.
    for (file, write, kind) in [
        (
            include_str!("../habits.rs"),
            "adds::logged_write(\"habit\"",
            "\"habit.completed\"",
        ),
        (
            include_str!("../money_entries.rs"),
            "write_entry(data, &e)?;",
            "\"money.logged\"",
        ),
        (
            include_str!("../life_dates.rs"),
            "write_json_atomic(&file(dir), &store, MAX_BYTES)?;\n    if rec.done",
            "\"date.done\"",
        ),
    ] {
        let w = file.find(write).unwrap_or_else(|| panic!("{write}"));
        let k = file.find(kind).unwrap_or_else(|| panic!("{kind}"));
        assert!(k > w, "{kind} must be recorded after {write}");
    }
}

#[test]
fn a_failing_activity_store_never_fails_the_owner_write() {
    let t = tmp("nostore");
    // A file where amara.db should be a directory's child: the store can't open.
    std::fs::create_dir_all(t.0.join("amara.db")).unwrap();
    let draft: crate::commands::checkin::CheckInDraft =
        serde_json::from_value(serde_json::json!({"mood": 3, "energy": 3})).unwrap();
    let r = crate::commands::checkin::save_in(
        &crate::commands::checkin::dir_in(&t.0),
        &draft,
        crate::commands::shared::time::now_jst(),
    );
    assert!(r.is_ok());
    assert_eq!(
        crate::commands::checkin::list_in(&crate::commands::checkin::dir_in(&t.0), 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn forgetting_a_take_removes_its_recommendation_and_outcomes() {
    let t = tmp("forgettake");
    let dir = t.0.join("takes");
    let entry: crate::commands::take_log::TakeLogEntry = serde_json::from_value(serde_json::json!({
        "id": "life-1", "page": "life", "day": "2026-11-20", "generatedAt": 1, "kind": "daily", "text": "x"
    }))
    .unwrap();
    crate::commands::take_log::append_in(&dir, &entry).unwrap();
    assert!(crate::commands::take_log::forget_in(&dir, "life-1", 5).unwrap());
    let c = store_for(&t.0, Scope::Personal).open().unwrap();
    let n: i64 = c
        .query_row("SELECT COUNT(*) FROM recommendation", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}
