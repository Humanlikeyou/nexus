use super::*;
use crate::commands::local_store::tests::temp_dir;

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-11-25T10:00:00+09:00").unwrap()
}

fn env(kind: &str, payload: &str) -> String {
    format!(
        r#"{{"v":1,"kind":"{kind}","source":"telegram","at":"2026-11-25T09:59:00+09:00","payload":{payload}}}"#
    )
}

fn drop_file(data: &Path, name: &str, body: &str) -> PathBuf {
    let dir = inbox_dir(data);
    crate::app_paths::create_private_dir_all(&dir).unwrap();
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    p
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn checkins(data: &Path) -> Vec<super::super::checkin::CheckInRecord> {
    super::super::checkin::list_in(&super::super::checkin::dir_in(data), 50).unwrap()
}

fn goals(data: &Path) {
    fs::write(
        data.join("goals.json"),
        r#"[
 {"id":"h1","title":"Morning stretch","category":"habit","target":5,"current":0,"unit":"","deadline":null},
 {"id":"h2","title":"Evening walk","category":"habit","target":5,"current":0,"unit":"","deadline":null},
 {"id":"h3","title":"Morning journal","category":"habit","target":5,"current":0,"unit":"","deadline":null},
 {"id":"h4","title":"Cold shower","category":"habit","target":5,"current":0,"unit":"","deadline":null,"paused":true},
 {"id":"g1","title":"Save money","category":"finance","target":5,"current":0,"unit":"","deadline":null}]"#,
    )
    .unwrap();
}

fn why(data: &Path, name: &str) -> String {
    fs::read_to_string(
        inbox_dir(data)
            .join("rejected")
            .join(format!("{name}.why.txt")),
    )
    .unwrap()
}

#[test]
fn checkin_is_applied_like_the_app_button_and_filed_in_done() {
    let d = temp_dir("in-ci");
    drop_file(
        &d,
        "1.json",
        &env(
            "checkin",
            r#"{"mood":4,"energy":2,"text":"slow day","tags":["work"," "]}"#,
        ),
    );
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.applied, vec!["checkin"]);
    let c = checkins(&d);
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].mood, c[0].energy, c[0].transcribed), (4, 2, false));
    assert_eq!(c[0].text, "slow day");
    assert_eq!(c[0].tags, vec!["work"]);
    assert_eq!(names(&inbox_dir(&d).join("done")), vec!["1.json"]);
    assert!(!inbox_dir(&d).join("1.json").exists());
    assert!(names(&inbox_dir(&d).join("processing")).is_empty());
}

#[test]
fn a_second_checkin_today_is_added_not_overwritten_and_marked() {
    let d = temp_dir("in-ci2");
    let a = |p: &str| {
        let e: Envelope = serde_json::from_str(&env("checkin", p)).unwrap();
        apply_in(&d, e, now()).unwrap()
    };
    assert_eq!(a(r#"{"mood":3,"energy":3}"#).note, None);
    assert_eq!(
        a(r#"{"mood":5,"energy":5}"#).note,
        Some("second_checkin_today")
    );
    assert_eq!(checkins(&d).len(), 2);
}

#[test]
fn bad_checkins_are_rejected_with_a_reason_and_nothing_saved() {
    let d = temp_dir("in-cibad");
    let big = "x".repeat(2001);
    let cases = [
        r#"{"mood":6,"energy":3}"#.to_string(),
        r#"{"mood":3,"energy":0}"#.to_string(),
        format!(r#"{{"mood":3,"energy":3,"text":"{big}"}}"#),
        r#"{"mood":3,"energy":3,"tags":["1","2","3","4","5","6","7","8","9"]}"#.to_string(),
        format!(r#"{{"mood":3,"energy":3,"tags":["{}"]}}"#, "t".repeat(41)),
        r#"{"mood":3,"energy":3,"extra":1}"#.to_string(),
        r#"{"mood":"high","energy":3}"#.to_string(),
    ];
    for (i, p) in cases.iter().enumerate() {
        drop_file(&d, &format!("{i}.json"), &env("checkin", p));
    }
    drop_file(&d, "garbage.json", "not json at all");
    drop_file(&d, "kind.json", &env("transfer", "{}"));
    drop_file(
        &d,
        "ver.json",
        &env("checkin", "{}").replace(r#""v":1"#, r#""v":2"#),
    );
    drop_file(
        &d,
        "src.json",
        &env("checkin", "{}").replace("telegram", "email"),
    );
    drop_file(
        &d,
        "at.json",
        &env("checkin", "{}").replace("2026-11-25T09:59:00+09:00", "yesterday"),
    );
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.rejected, cases.len() + 5);
    assert!(r.applied.is_empty());
    assert!(checkins(&d).is_empty());
    assert!(names(&inbox_dir(&d).join("done")).is_empty());
    assert!(why(&d, "garbage.json").contains("not valid JSON"));
    assert!(why(&d, "0.json").contains("1 to 5"));
    assert_eq!(
        names(&inbox_dir(&d).join("rejected")).len(),
        (cases.len() + 5) * 2
    );
}

fn goal(id: &str, title: &str, paused: bool) -> Goal {
    serde_json::from_value(serde_json::json!({
        "id": id, "title": title, "category": "habit", "target": 5.0, "current": 0.0,
        "unit": "", "deadline": null, "paused": paused
    }))
    .unwrap()
}

#[test]
fn habit_resolution_is_exact_id_then_title_then_one_best_word_match() {
    let gs = vec![
        goal("h1", "Morning stretch", false),
        goal("h2", "Evening walk", false),
        goal("h3", "Morning journal", false),
        goal("h4", "Cold shower", true),
    ];
    assert_eq!(resolve_habit(&gs, "h2").unwrap(), "h2");
    assert_eq!(resolve_habit(&gs, "EVENING WALK").unwrap(), "h2");
    assert_eq!(resolve_habit(&gs, "I did my stretch").unwrap(), "h1");
    assert_eq!(resolve_habit(&gs, "morning stretch today").unwrap(), "h1");
    assert!(resolve_habit(&gs, "morning")
        .unwrap_err()
        .contains("more than one"));
    assert!(resolve_habit(&gs, "yoga")
        .unwrap_err()
        .contains("no active habit"));
    assert!(resolve_habit(&gs, "  ").is_err());
    assert!(resolve_habit(&gs, "h4").unwrap_err().contains("paused"));
    assert!(resolve_habit(&gs, "cold shower")
        .unwrap_err()
        .contains("no active"));
    // A goal that is not a habit never matches.
    let mut other = goal("g1", "Save money", false);
    other.category = "finance".into();
    assert!(resolve_habit(&[other], "save money").is_err());
}

#[test]
fn habit_reply_marks_with_source_telegram_and_keeps_the_back_dating_rule() {
    let d = temp_dir("in-hab");
    goals(&d);
    drop_file(
        &d,
        "1.json",
        &env("habit", r#"{"habit":"stretch","done":true,"day":""}"#),
    );
    drop_file(
        &d,
        "2.json",
        &env("habit", r#"{"habit":"h2","done":true,"day":"2026-11-23"}"#),
    );
    drop_file(
        &d,
        "3.json",
        &env("habit", r#"{"habit":"h2","done":true,"day":"2026-11-10"}"#),
    );
    drop_file(
        &d,
        "4.json",
        &env("habit", r#"{"habit":"h2","done":true,"day":"2026-11-26"}"#),
    );
    drop_file(
        &d,
        "5.json",
        &env("habit", r#"{"habit":"morning","done":true,"day":""}"#),
    );
    drop_file(
        &d,
        "6.json",
        &env("habit", r#"{"habit":"h4","done":true,"day":""}"#),
    );
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.applied, vec!["habit", "habit"]);
    assert_eq!(
        r.rejected, 4,
        "10 days back, a future day, ambiguous, paused"
    );
    let rows = super::super::habits::rows_in(&super::super::habits::dir_in(&d)).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.source == "telegram" && r.done));
    assert!(rows
        .iter()
        .any(|r| r.habit_id == "h1" && r.day == "2026-11-25"));
    assert!(rows
        .iter()
        .any(|r| r.habit_id == "h2" && r.day == "2026-11-23"));
    assert!(why(&d, "5.json").contains("more than one"));
    assert!(why(&d, "3.json").contains("too far back"));
    // Already marked: applied, with a note.
    let e: Envelope = serde_json::from_str(&env("habit", r#"{"habit":"h1","done":true}"#)).unwrap();
    assert_eq!(apply_in(&d, e, now()).unwrap().note, Some("already_marked"));
}

fn entries(d: &Path) -> Vec<super::super::money_entries::MoneyEntry> {
    let (v, _) = super::super::money_entries::load(d).unwrap();
    v
}

#[test]
fn money_reply_becomes_integer_minor_units_with_source_telegram() {
    let d = temp_dir("in-money");
    drop_file(
        &d,
        "1.json",
        &env(
            "money",
            r#"{"kind":"expense","amount":1200.4,"currency":"JPY","note":"lunch","category":"Food","date":""}"#,
        ),
    );
    drop_file(
        &d,
        "2.json",
        &env(
            "money",
            r#"{"kind":"balance","amount":12.345,"currency":"USD"}"#,
        ),
    );
    drop_file(
        &d,
        "3.json",
        &env(
            "money",
            r#"{"kind":"bill","amount":5000,"currency":"JPY","date":"2026-12-01"}"#,
        ),
    );
    drop_file(
        &d,
        "4.json",
        &env(
            "money",
            r#"{"kind":"income","amount":300000,"currency":"JPY","date":"2026-11-20","note":"salary"}"#,
        ),
    );
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.applied.len(), 4, "{r:?}");
    let e = entries(&d);
    let by = |k: &str| e.iter().find(|x| x.kind == k).unwrap();
    assert_eq!(by("expense").amount_minor, 1200);
    assert_eq!(by("expense").category.as_deref(), Some("food"));
    assert_eq!(by("expense").date, "2026-11-25");
    assert_eq!(by("balance").amount_minor, 1235);
    assert_eq!(by("balance").currency, "USD");
    assert_eq!(by("bill").due.as_deref(), Some("2026-12-01"));
    assert_eq!(by("income").date, "2026-11-20");
    assert!(e.iter().all(|x| x.source == "telegram"));
}

#[test]
fn bad_money_is_rejected() {
    let d = temp_dir("in-moneybad");
    let note = "n".repeat(301);
    let cases = [
        r#"{"kind":"expense","amount":0,"currency":"JPY"}"#.to_string(),
        r#"{"kind":"expense","amount":-5,"currency":"JPY"}"#.to_string(),
        r#"{"kind":"expense","amount":2000000000,"currency":"JPY"}"#.to_string(),
        r#"{"kind":"expense","amount":0.001,"currency":"USD"}"#.to_string(),
        r#"{"kind":"expense","amount":5,"currency":"EUR"}"#.to_string(),
        r#"{"kind":"commitment","amount":5,"currency":"JPY"}"#.to_string(),
        r#"{"kind":"transfer","amount":5,"currency":"JPY"}"#.to_string(),
        r#"{"kind":"expense","amount":5,"currency":"JPY","category":"yachts"}"#.to_string(),
        r#"{"kind":"expense","amount":5,"currency":"JPY","date":"25/11/2026"}"#.to_string(),
        r#"{"kind":"expense","amount":5,"currency":"JPY","date":"2026-12-25"}"#.to_string(),
        format!(r#"{{"kind":"expense","amount":5,"currency":"JPY","note":"{note}"}}"#),
        r#"{"kind":"expense","amount":"lots","currency":"JPY"}"#.to_string(),
        r#"{"kind":"expense","amount":5,"currency":"JPY","account_id":"a"}"#.to_string(),
    ];
    for (i, p) in cases.iter().enumerate() {
        drop_file(&d, &format!("{i:02}.json"), &env("money", p));
    }
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.rejected, cases.len(), "{r:?}");
    assert!(entries(&d).is_empty());
}

#[test]
fn symlinks_dotfiles_other_files_and_big_files_are_handled_safely() {
    let d = temp_dir("in-files");
    let outside = d.join("outside.json");
    fs::write(&outside, env("checkin", r#"{"mood":3,"energy":3}"#)).unwrap();
    let inbox = inbox_dir(&d);
    crate::app_paths::create_private_dir_all(&inbox).unwrap();
    std::os::unix::fs::symlink(&outside, inbox.join("link.json")).unwrap();
    drop_file(
        &d,
        ".hidden.json",
        &env("checkin", r#"{"mood":3,"energy":3}"#),
    );
    drop_file(&d, ".1.json.tmp", "partial");
    drop_file(&d, "note.txt", "hello");
    fs::create_dir(inbox.join("sub.json")).unwrap();
    drop_file(&d, "big.json", &" ".repeat(9000));
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.skipped, 2, "the symlink and the directory");
    assert_eq!(r.rejected, 1);
    assert!(checkins(&d).is_empty(), "the symlink target was never read");
    assert!(
        fs::symlink_metadata(inbox.join("link.json")).is_ok(),
        "left in place"
    );
    for kept in [".hidden.json", ".1.json.tmp", "note.txt"] {
        assert!(inbox.join(kept).exists(), "{kept}");
    }
    assert!(why(&d, "big.json").contains("8 KB"));
}

#[test]
fn a_folder_that_others_can_write_is_not_used() {
    use std::os::unix::fs::PermissionsExt;
    let d = temp_dir("in-perm");
    drop_file(&d, "1.json", &env("checkin", r#"{"mood":3,"energy":3}"#));
    fs::set_permissions(inbox_dir(&d), fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(ingest_pass_in(&d, now()), PassReport::default());
    assert!(checkins(&d).is_empty());
    assert!(inbox_dir(&d).join("1.json").exists());
}

#[test]
fn a_pass_takes_at_most_50_files() {
    let d = temp_dir("in-cap");
    for i in 0..60 {
        drop_file(&d, &format!("{i:03}.json"), "x");
    }
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.rejected, 50);
    let left: Vec<_> = names(&inbox_dir(&d))
        .into_iter()
        .filter(|n| n.ends_with(".json"))
        .collect();
    assert_eq!(left.len(), 10);
    assert_eq!(left[0], "050.json", "oldest names first");
    assert_eq!(ingest_pass_in(&d, now()).rejected, 10);
}

#[test]
fn a_file_left_in_processing_is_rejected_at_start_and_never_applied() {
    let d = temp_dir("in-crash");
    let inbox = inbox_dir(&d);
    crate::app_paths::create_private_dir_all(&inbox.join("processing")).unwrap();
    fs::write(
        inbox.join("processing").join("9.json"),
        env("checkin", r#"{"mood":5,"energy":5}"#),
    )
    .unwrap();
    assert_eq!(recover_processing_in(&d, now()), 1);
    assert!(names(&inbox.join("processing")).is_empty());
    assert_eq!(
        names(&inbox.join("rejected")),
        vec!["9.json", "9.json.why.txt"]
    );
    assert!(why(&d, "9.json").contains("interrupted"));
    ingest_pass_in(&d, now());
    assert!(checkins(&d).is_empty(), "not applied");
    assert_eq!(recover_processing_in(&d, now()), 0);
}

#[test]
fn applying_is_idempotent_across_passes_and_name_clashes_never_overwrite() {
    let d = temp_dir("in-idem");
    drop_file(&d, "1.json", &env("checkin", r#"{"mood":4,"energy":4}"#));
    ingest_pass_in(&d, now());
    ingest_pass_in(&d, now());
    assert_eq!(checkins(&d).len(), 1);
    // The same file name arriving again is applied as a new reply and filed beside the first.
    drop_file(&d, "1.json", &env("checkin", r#"{"mood":2,"energy":2}"#));
    ingest_pass_in(&d, now());
    assert_eq!(checkins(&d).len(), 2);
    assert_eq!(
        names(&inbox_dir(&d).join("done")),
        vec!["1-1.json", "1.json"]
    );
}

#[test]
fn an_applied_reply_closes_its_open_ask_only() {
    let d = temp_dir("in-asks");
    super::super::proactive::write_ask_in(&d, "checkin", "How?", now()).unwrap();
    super::super::proactive::write_ask_in(&d, "money", "Money?", now()).unwrap();
    drop_file(&d, "1.json", &env("checkin", r#"{"mood":4,"energy":4}"#));
    drop_file(&d, "2.json", &env("checkin", r#"{"mood":9,"energy":4}"#));
    ingest_pass_in(&d, now());
    let left: Vec<_> = super::super::proactive::read_asks_in(&d)
        .into_iter()
        .map(|a| a.kind)
        .collect();
    assert_eq!(left, vec!["money"]);
    // A rejected reply leaves the ask open.
    super::super::proactive::write_ask_in(&d, "checkin", "How?", now()).unwrap();
    drop_file(&d, "3.json", &env("checkin", r#"{"mood":9,"energy":4}"#));
    ingest_pass_in(&d, now());
    assert_eq!(super::super::proactive::read_asks_in(&d).len(), 2);
}

#[test]
fn status_counts_files_and_remembers_the_last_apply_without_content() {
    let d = temp_dir("in-status");
    assert_eq!(status_in(&d, now()).done, 0);
    drop_file(
        &d,
        "1.json",
        &env("checkin", r#"{"mood":4,"energy":4,"text":"private words"}"#),
    );
    drop_file(&d, "2.json", "junk");
    ingest_pass_in(&d, now());
    let s = status_in(&d, now());
    assert_eq!((s.done, s.rejected), (1, 1));
    assert_eq!(s.last_applied.as_ref().unwrap().kind, "checkin");
    assert_eq!(
        s.last_rejected_reason.as_deref(),
        Some("not valid JSON for this format")
    );
    let raw = fs::read_to_string(state_path(&d)).unwrap();
    assert!(!raw.contains("private words"));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(state_path(&d)).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn replies_are_capped_per_kind_per_day_and_the_cap_resets_the_next_day() {
    let d = temp_dir("inbox-cap");
    for i in 0..5 {
        drop_file(
            &d,
            &format!("c{i}.json"),
            &env("checkin", r#"{"mood":3,"energy":3,"text":"ok","tags":[]}"#),
        );
    }
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.applied.len(), 3, "3 check-ins a day");
    assert_eq!(r.rejected, 2);
    assert!(names(&inbox_dir(&d).join("rejected"))
        .iter()
        .any(|n| n.ends_with(".why.txt")));
    assert_eq!(checkins(&d).len(), 3);
    // the next day starts again
    drop_file(
        &d,
        "c9.json",
        &env(
            "checkin",
            r#"{"mood":4,"energy":4,"text":"next day","tags":[]}"#,
        ),
    );
    let tomorrow = now() + chrono::Duration::days(1);
    assert_eq!(ingest_pass_in(&d, tomorrow).applied, ["checkin"]);
    let _ = fs::remove_dir_all(d);
}

#[test]
fn a_money_date_more_than_a_year_away_is_rejected() {
    let d = temp_dir("inbox-date");
    drop_file(
        &d,
        "far.json",
        &env(
            "money",
            r#"{"kind":"bill","amount":1000,"currency":"JPY","date":"9999-12-31"}"#,
        ),
    );
    drop_file(
        &d,
        "old.json",
        &env(
            "money",
            r#"{"kind":"expense","amount":1000,"currency":"JPY","date":"0001-01-01"}"#,
        ),
    );
    drop_file(
        &d,
        "ok.json",
        &env(
            "money",
            r#"{"kind":"bill","amount":1000,"currency":"JPY","date":"2026-12-20"}"#,
        ),
    );
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.applied, ["money"]);
    assert_eq!(r.rejected, 2);
    assert!(why(&d, "far.json").contains("more than a year"));
    let _ = fs::remove_dir_all(d);
}

#[test]
fn ingest_pauses_when_the_rejected_folder_is_full() {
    let d = temp_dir("inbox-full");
    let rej_dir = inbox_dir(&d).join("rejected");
    crate::app_paths::create_private_dir_all(&rej_dir).unwrap();
    for i in 0..REJECTED_PAUSE {
        fs::write(rej_dir.join(format!("r{i}.json")), "{}").unwrap();
    }
    drop_file(
        &d,
        "c.json",
        &env("checkin", r#"{"mood":3,"energy":3,"text":"ok","tags":[]}"#),
    );
    let r = ingest_pass_in(&d, now());
    assert!(r.applied.is_empty(), "paused");
    assert!(
        inbox_dir(&d).join("c.json").exists(),
        "left in place, never deleted"
    );
    let _ = fs::remove_dir_all(d);
}

#[test]
fn a_fifo_in_the_inbox_is_skipped_and_never_blocks_the_pass() {
    let d = temp_dir("inbox-fifo");
    let dir = inbox_dir(&d);
    crate::app_paths::create_private_dir_all(&dir).unwrap();
    let fifo = dir.join("pipe.json");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: a valid NUL-terminated path; mkfifo has no other preconditions.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    drop_file(
        &d,
        "c.json",
        &env("checkin", r#"{"mood":3,"energy":3,"text":"ok","tags":[]}"#),
    );
    let r = ingest_pass_in(&d, now());
    assert_eq!(r.applied, ["checkin"]);
    let _ = fs::remove_dir_all(d);
}
