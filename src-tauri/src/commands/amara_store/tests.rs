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
