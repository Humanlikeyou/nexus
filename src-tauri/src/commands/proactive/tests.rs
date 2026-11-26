use super::*;
use crate::commands::local_store::tests::temp_dir;
use std::sync::atomic::{AtomicUsize, Ordering};

fn at(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).unwrap()
}

// 2026-11-25 is a Wednesday, 2026-11-29 a Sunday.
const WED_EVE: &str = "2026-11-25T22:30:00+09:00";

fn base<'a>(now: DateTime<FixedOffset>, st: &'a KindState) -> DecideIn<'a> {
    DecideIn {
        now,
        enabled: true,
        at: Some("22:00"),
        weekday: None,
        needed: true,
        state: st,
        disabled: false,
    }
}

#[test]
fn decide_waits_for_the_time_and_for_something_to_ask() {
    let st = KindState::default();
    assert_eq!(
        decide(&base(at("2026-11-25T21:59:00+09:00"), &st)),
        Decision::Wait
    );
    assert_eq!(
        decide(&base(at("2026-11-25T22:00:00+09:00"), &st)),
        Decision::Send
    );
    let mut d = base(at(WED_EVE), &st);
    d.needed = false;
    assert_eq!(decide(&d), Decision::Wait);
    let mut d = base(at(WED_EVE), &st);
    d.enabled = false;
    assert_eq!(decide(&d), Decision::Wait);
}

#[test]
fn the_kill_switch_suppresses_once_and_never_spams() {
    let mut st = KindState::default();
    let mut d = base(at("2026-11-25T22:00:00+09:00"), &st);
    d.disabled = true;
    assert_eq!(decide(&d), Decision::Suppress);
    record_outcome(
        &mut st,
        at("2026-11-25T22:00:00+09:00"),
        Outcome::Suppressed,
    );
    // Every later minute of the same day: nothing.
    for m in 1..(2 * 60) {
        let now = at("2026-11-25T22:00:00+09:00") + Duration::minutes(m);
        if day_of(&now) != day_of(&at("2026-11-25T22:00:00+09:00")) {
            break;
        }
        let mut d = base(now, &st);
        d.disabled = true;
        assert_eq!(decide(&d), Decision::Wait, "minute {m}");
    }
    // The next day it is due again.
    let mut d = base(at("2026-11-26T22:00:00+09:00"), &st);
    d.disabled = true;
    assert_eq!(decide(&d), Decision::Suppress);
}

#[test]
fn a_sent_question_is_not_sent_again_that_day() {
    let mut st = KindState::default();
    record_outcome(&mut st, at(WED_EVE), Outcome::Sent);
    assert_eq!(
        decide(&base(at("2026-11-25T23:50:00+09:00"), &st)),
        Decision::Wait
    );
    assert_eq!(
        decide(&base(at("2026-11-26T22:05:00+09:00"), &st)),
        Decision::Send
    );
}

#[test]
fn a_failed_send_retries_once_after_30_minutes() {
    let mut st = KindState::default();
    let t0 = at("2026-11-25T22:00:00+09:00");
    record_outcome(&mut st, t0, Outcome::Failed);
    assert_eq!(
        decide(&base(t0 + Duration::minutes(29), &st)),
        Decision::Wait
    );
    assert_eq!(
        decide(&base(t0 + Duration::minutes(30), &st)),
        Decision::Send
    );
    record_outcome(&mut st, t0 + Duration::minutes(30), Outcome::Failed);
    assert_eq!(st.attempts, 2);
    assert_eq!(
        decide(&base(t0 + Duration::minutes(90), &st)),
        Decision::Wait
    );
}

#[test]
fn money_goes_out_on_its_weekday_once_a_week() {
    let st = KindState::default();
    let mk = |now: &str, st: &'_ KindState| {
        let mut d = base(at(now), st);
        d.at = Some("19:00");
        d.weekday = Some(Weekday::Sun);
        decide(&d)
    };
    assert_eq!(
        mk("2026-11-28T19:30:00+09:00", &st),
        Decision::Wait,
        "Saturday"
    );
    assert_eq!(
        mk("2026-11-29T18:59:00+09:00", &st),
        Decision::Wait,
        "before time"
    );
    assert_eq!(mk("2026-11-29T19:00:00+09:00", &st), Decision::Send);
    let mut sent = KindState::default();
    record_outcome(&mut sent, at("2026-11-29T19:00:00+09:00"), Outcome::Sent);
    assert_eq!(mk("2026-11-29T20:00:00+09:00", &sent), Decision::Wait);
    assert_eq!(
        mk("2026-12-06T19:00:00+09:00", &sent),
        Decision::Send,
        "next week"
    );
}

#[test]
fn habit_message_lists_due_habits_and_stays_short() {
    let h = |id: &str, title: &str, at: Option<&str>, paused: bool, marked: bool| HabitIn {
        id: id.into(),
        title: title.into(),
        remind_at: at.map(String::from),
        paused,
        marked_today: marked,
    };
    let hs = vec![
        h("a", "Stretch", None, false, false),
        h("b", "Walk", Some("21:00"), false, false),
        h("c", "Late", Some("23:00"), false, false),
        h("d", "Paused", None, true, false),
        h("e", "Marked", None, false, true),
    ];
    let due = due_habit_titles(&hs, at("2026-11-25T21:30:00+09:00"));
    assert_eq!(due, vec!["Stretch", "Walk"]);
    assert_eq!(
        habit_text(&due),
        "Did you do: Stretch, Walk? Reply and I will mark them."
    );
    assert!(due_habit_titles(&hs, at("2026-11-25T19:59:00+09:00")).is_empty());
    let many: Vec<String> = (0..7).map(|i| format!("h{i}")).collect();
    assert!(habit_text(&many).contains("and 2 more"));
    let long = habit_text(&["x".repeat(100)]);
    assert!(long.chars().count() < 100);
}

#[test]
fn settings_validate_times_and_weekday() {
    let ok = ProactiveSettings::default();
    assert!(validate(&ok).is_ok());
    let mut s = ok.clone();
    s.checkin.at = "25:00".into();
    assert!(validate(&s).is_err());
    let mut s = ok.clone();
    s.money.at = "7:00".into();
    assert!(validate(&s).is_err());
    let mut s = ok.clone();
    s.money.weekday = "Sunday".into();
    assert!(validate(&s).is_err());
    let mut s = ok.clone();
    s.money.weekday = "Fri".into();
    s.checkin.at = " 08:15 ".into();
    assert_eq!(validate(&s).unwrap().checkin.at, "08:15");
    assert_eq!(ProactiveSettings::default().checkin.at, "22:00");
    assert_eq!(ProactiveSettings::default().money.weekday, "Sun");
}

#[test]
fn settings_round_trip_default_when_missing_or_broken() {
    let d = temp_dir("pro-set");
    assert_eq!(load_settings_in(&d), ProactiveSettings::default());
    let mut s = ProactiveSettings::default();
    s.checkin.at = "21:15".into();
    s.habits.enabled = false;
    save_settings_in(&d, &s).unwrap();
    assert_eq!(load_settings_in(&d), s);
    let mut bad = s.clone();
    bad.money.weekday = "Xyz".into();
    assert!(save_settings_in(&d, &bad).is_err());
    assert_eq!(load_settings_in(&d), s, "a refused save changes nothing");
    std::fs::write(settings_file(&d), "{nope").unwrap();
    assert_eq!(load_settings_in(&d), ProactiveSettings::default());
    // Unknown keys are refused, so a typo can't silently pass.
    assert!(
        serde_json::from_str::<ProactiveSettings>(r#"{"checkin":{"enabled":true,"x":1}}"#).is_err()
    );
    // A partial file fills the rest with defaults.
    let p: ProactiveSettings = serde_json::from_str(r#"{"habits":{"enabled":false}}"#).unwrap();
    assert!(!p.habits.enabled && p.checkin.enabled);
}

#[test]
fn asks_replace_per_kind_expire_after_36_hours_and_are_private() {
    let d = temp_dir("pro-asks");
    let t0 = at("2026-11-25T10:00:00+09:00");
    write_ask_in(&d, "checkin", "q1", t0).unwrap();
    write_ask_in(&d, "money", "q2", t0 + Duration::hours(1)).unwrap();
    write_ask_in(&d, "checkin", "q3", t0 + Duration::hours(2)).unwrap();
    let asks = read_asks_in(&d);
    assert_eq!(asks.iter().filter(|a| a.kind == "checkin").count(), 1);
    assert_eq!(
        asks.iter().find(|a| a.kind == "checkin").unwrap().text,
        "q3"
    );
    // 37 h after the money ask: it is gone on the next write.
    write_ask_in(&d, "habit", "q4", t0 + Duration::hours(38)).unwrap();
    let kinds: Vec<_> = read_asks_in(&d).into_iter().map(|a| a.kind).collect();
    assert_eq!(kinds, vec!["habit"], "checkin (36 h old) and money expired");
    remove_asks_in(&d, "habit", t0 + Duration::hours(39)).unwrap();
    assert!(read_asks_in(&d).is_empty());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(asks_file(&d))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    // Hermes reads {kind, text, at}.
    write_ask_in(&d, "money", "q", t0).unwrap();
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(asks_file(&d)).unwrap()).unwrap();
    assert!(raw[0]["kind"] == "money" && raw[0]["text"] == "q" && raw[0]["at"].is_string());
}

#[tokio::test]
async fn run_sends_the_checkin_once_writes_the_ask_and_never_repeats() {
    let d = temp_dir("pro-run");
    let calls = AtomicUsize::new(0);
    let texts = Mutex::new(Vec::<String>::new());
    let send = |t: String| {
        calls.fetch_add(1, Ordering::SeqCst);
        texts.lock().unwrap().push(t);
        async { Outcome::Sent }
    };
    let now = at(WED_EVE);
    let r = run_in(&d, now, false, &send).await;
    assert_eq!(r, vec![(Kind::Checkin, Outcome::Sent)]);
    assert_eq!(texts.lock().unwrap()[0], CHECKIN_TEXT);
    let asks = read_asks_in(&d);
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].kind, "checkin");
    for m in 1..30 {
        run_in(&d, now + Duration::minutes(m), false, &send).await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let sent = sent_today_in(&d, day_of(&now));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].result, "sent");
}

#[tokio::test]
async fn run_does_not_ask_when_today_is_already_checked_in() {
    let d = temp_dir("pro-done");
    let now = at(WED_EVE);
    super::super::checkin::save_in(
        &super::super::checkin::dir_in(&d),
        &super::super::checkin::CheckInDraft {
            mood: 3,
            energy: 3,
            tags: vec![],
            text: String::new(),
            transcribed: false,
        },
        now,
    )
    .unwrap();
    let calls = AtomicUsize::new(0);
    let send = |_t: String| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Outcome::Sent }
    };
    run_in(&d, now, false, &send).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn run_under_the_kill_switch_records_suppressed_and_sends_nothing() {
    let d = temp_dir("pro-kill");
    let calls = AtomicUsize::new(0);
    let send = |_t: String| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Outcome::Sent }
    };
    let now = at(WED_EVE);
    let r = run_in(&d, now, true, &send).await;
    assert_eq!(r, vec![(Kind::Checkin, Outcome::Suppressed)]);
    for m in 1..120 {
        let t = now + Duration::minutes(m);
        if day_of(&t) != day_of(&now) {
            break;
        }
        assert!(run_in(&d, t, true, &send).await.is_empty());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        read_asks_in(&d).is_empty(),
        "nothing was asked, so nothing is open"
    );
    assert_eq!(sent_today_in(&d, day_of(&now))[0].result, "suppressed");
}

#[tokio::test]
async fn run_retries_a_failed_send_once_then_stops() {
    let d = temp_dir("pro-fail");
    let calls = AtomicUsize::new(0);
    let send = |_t: String| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Outcome::Failed }
    };
    let now = at("2026-11-25T22:00:00+09:00");
    for m in 0..200 {
        let t = now + Duration::minutes(m);
        if day_of(&t) != day_of(&now) {
            break;
        }
        run_in(&d, t, false, &send).await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(read_asks_in(&d).is_empty());
}

#[tokio::test]
async fn run_asks_about_due_habits_in_one_message() {
    let d = temp_dir("pro-habits");
    std::fs::write(
        d.join("goals.json"),
        r#"[{"id":"h1","title":"Stretch","category":"habit","target":5,"current":0,"unit":"","deadline":null},
            {"id":"h2","title":"Walk","category":"habit","target":5,"current":0,"unit":"","deadline":null,"paused":true}]"#,
    )
    .unwrap();
    let texts = Mutex::new(Vec::<String>::new());
    let send = |t: String| {
        texts.lock().unwrap().push(t);
        async { Outcome::Sent }
    };
    // 20:30: the default 20:00 reminder has passed, the 22:00 check-in has not.
    let r = run_in(&d, at("2026-11-25T20:30:00+09:00"), false, &send).await;
    assert_eq!(r, vec![(Kind::Habits, Outcome::Sent)]);
    assert_eq!(
        texts.lock().unwrap()[0],
        "Did you do: Stretch? Reply and I will mark them."
    );
    assert_eq!(read_asks_in(&d)[0].kind, "habit");
}

#[tokio::test]
async fn an_unwritable_state_file_never_makes_the_question_go_out_every_minute() {
    let d = temp_dir("pro-nostate");
    // the state path is a directory, so recording the send fails
    std::fs::create_dir_all(state_file(&d)).unwrap();
    let calls = AtomicUsize::new(0);
    let send = |_t: String| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Outcome::Sent }
    };
    let now = at(WED_EVE);
    for m in 0..10 {
        run_in(&d, now + Duration::minutes(m), false, &send).await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1, "sent once, then held");
    let _ = std::fs::remove_dir_all(d);
}
