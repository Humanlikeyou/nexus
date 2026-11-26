use super::*;
use crate::commands::amara_store::{NewEvidence, NewRecommendation};
use crate::commands::local_store::tests::temp_dir;
use std::sync::atomic::{AtomicUsize, Ordering};

fn at(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).unwrap()
}

const NOON: &str = "2026-11-25T12:00:00+09:00";

fn cfg() -> ProactiveNudges {
    ProactiveNudges::default()
}

fn rec<'a>(
    kind: &'a str,
    target: Option<&'a str>,
    expires: Option<&'a str>,
) -> NewRecommendation<'a> {
    NewRecommendation {
        observation: Some("Done 9 of 10 mornings"),
        rationale: None,
        suggested_action: Some("Meditate before breakfast"),
        uncertainty: None,
        impact: None,
        effort: None,
        evaluation_json: None,
        period_start: None,
        period_end: None,
        fingerprint: None,
        id: None,
        scope: Scope::Personal,
        domain: "life",
        kind,
        title: "Move meditation to mornings",
        body: "Mornings went better.",
        expires_at: expires,
        produced_by: "rules:habit_timing",
        model_tier: None,
        confidence: None,
        confidence_basis: None,
        target_ref: target,
        supersedes_id: None,
    }
}

fn ev(quality: &str) -> NewEvidence<'_> {
    NewEvidence {
        record_ref: "goals:g-1",
        window_start: Some("2026-11-01T00:00:00Z"),
        window_end: Some("2026-11-20T00:00:00Z"),
        role: "observation",
        quality,
        note: None,
    }
}

fn seed(d: &Path) {
    if !d.join("goals.json").exists() {
        std::fs::write(
            d.join("goals.json"),
            r#"[{"id":"g-1","title":"Meditate","category":"life","target":10,"current":3,"unit":"x","deadline":null}]"#,
        )
        .unwrap();
    }
}

fn make(d: &Path, kind: &str, target: Option<&str>, q: &[&str]) -> String {
    seed(d);
    let evs: Vec<NewEvidence> = q.iter().map(|x| ev(x)).collect();
    store_for(d, Scope::Personal)
        .create_recommendation(&rec(kind, target, None), &evs)
        .unwrap()
}

fn status(d: &Path, id: &str) -> String {
    store_for(d, Scope::Personal)
        .get_recommendation(id)
        .unwrap()
        .unwrap()
        .recommendation
        .status
}

#[test]
fn quiet_hours_wrap_past_midnight_and_equal_ends_mean_none() {
    assert!(in_quiet(at("2026-11-25T23:00:00+09:00"), "22:00", "07:00"));
    assert!(in_quiet(at("2026-11-25T06:59:00+09:00"), "22:00", "07:00"));
    assert!(!in_quiet(at("2026-11-25T07:00:00+09:00"), "22:00", "07:00"));
    assert!(!in_quiet(at(NOON), "22:00", "07:00"));
    assert!(in_quiet(at(NOON), "12:00", "13:00"));
    assert!(!in_quiet(at(NOON), "12:00", "12:00"));
}

#[test]
fn only_good_evidence_is_brought_up_and_takes_never() {
    let d = temp_dir("nudge-quality");
    make(&d, "habit_timing", Some("goals:a"), &["estimate"]);
    make(
        &d,
        "habit_timing",
        Some("goals:b"),
        &["measured", "partial"],
    );
    make(&d, "take", Some("goals:d"), &["measured"]);
    let good = make(
        &d,
        "habit_timing",
        Some("goals:e"),
        &["measured", "self_report"],
    );
    let ids: Vec<String> = candidates(&d, at(NOON))
        .unwrap()
        .into_iter()
        .map(|c| c.rec.recommendation_id)
        .collect();
    assert_eq!(ids, [good]);
}

#[test]
fn bringing_one_up_records_shown_once_and_it_stays_without_a_second_record() {
    let d = temp_dir("nudge-once");
    let id = make(&d, "habit_timing", Some("goals:a"), &["measured"]);
    let first = insight_in(&d, &cfg(), at(NOON)).unwrap();
    assert!(first.nudged);
    assert_eq!(first.recommendation.unwrap().recommendation_id, id);
    assert_eq!(status(&d, &id), "shown");
    let again = insight_in(&d, &cfg(), at("2026-11-25T13:00:00+09:00")).unwrap();
    assert!(!again.nudged, "already shown: no second shown record");
    assert_eq!(again.recommendation.unwrap().recommendation_id, id);
    let outcomes = store_for(&d, Scope::Personal)
        .get_recommendation(&id)
        .unwrap()
        .unwrap()
        .outcomes;
    assert_eq!(outcomes.iter().filter(|o| o.outcome == "shown").count(), 1);
    // The status machine refuses a second shown outright.
    assert!(respond_in(&d, "personal", &id, "shown", None).is_err());
}

#[test]
fn a_shown_suggestion_is_left_alone_after_a_week() {
    let d = temp_dir("nudge-week");
    make(&d, "habit_timing", Some("goals:a"), &["measured"]);
    // shown_at comes from the store's clock (now), so look from 8 days later.
    insight_in(&d, &cfg(), at(NOON)).unwrap();
    let later = super::super::shared::time::now_jst() + Duration::days(8);
    let r = insight_in(&d, &cfg(), later).unwrap();
    assert!(r.recommendation.is_none());
    assert_eq!(r.held.as_deref(), Some("none"));
}

#[test]
fn the_daily_limit_and_quiet_hours_hold_new_ones_back() {
    let d = temp_dir("nudge-limit");
    let mut c = cfg();
    c.max_per_day = 1;
    let a = make(&d, "habit_timing", Some("goals:a"), &["measured"]);
    let b = make(&d, "habit_timing", Some("goals:b"), &["measured"]);
    let first = insight_in(&d, &c, at(NOON)).unwrap();
    assert!(first.nudged);
    let first = first.recommendation.unwrap().recommendation_id;
    let other = if first == a { b.clone() } else { a.clone() };
    // Answer the first so the slot frees; the limit still holds the other back today.
    respond_in(&d, "personal", &first, "accepted", None).unwrap();
    let r = insight_in(&d, &c, at("2026-11-25T15:00:00+09:00")).unwrap();
    assert_eq!(r.held.as_deref(), Some("daily_limit"));
    assert_eq!(status(&d, &other), "proposed");
    let r = insight_in(&d, &c, at("2026-11-26T23:30:00+09:00")).unwrap();
    assert_eq!(r.held.as_deref(), Some("quiet_hours"));
    assert!(
        insight_in(&d, &c, at("2026-11-26T09:00:00+09:00"))
            .unwrap()
            .nudged
    );
    let mut off = cfg();
    off.enabled = false;
    let d2 = temp_dir("nudge-off");
    make(&d2, "habit_timing", Some("goals:a"), &["measured"]);
    assert_eq!(
        insight_in(&d2, &off, at(NOON)).unwrap().held.as_deref(),
        Some("off")
    );
}

#[test]
fn a_dismissal_blocks_the_same_kind_and_target_for_30_days() {
    let d = temp_dir("nudge-dismiss");
    let a = make(&d, "habit_timing", Some("goals:a"), &["measured"]);
    respond_in(&d, "personal", &a, "dismissed", None).unwrap();
    let same = make(&d, "habit_timing", Some("goals:a"), &["measured"]);
    let other = make(&d, "habit_timing", Some("goals:b"), &["measured"]);
    let ids: Vec<String> = candidates(&d, super::super::shared::time::now_jst())
        .unwrap()
        .into_iter()
        .map(|c| c.rec.recommendation_id)
        .collect();
    assert_eq!(ids, std::slice::from_ref(&other));
    assert!(!ids.contains(&a) && !ids.contains(&same));
    // 31 days on, the block has passed for the new one (the dismissed one itself never returns).
    let later = super::super::shared::time::now_jst() + Duration::days(31);
    let ids: Vec<String> = candidates(&d, later)
        .unwrap()
        .into_iter()
        .map(|c| c.rec.recommendation_id)
        .collect();
    assert!(ids.contains(&same) && !ids.contains(&a));
}

#[test]
fn an_expired_one_is_recorded_expired_and_never_shown() {
    let d = temp_dir("nudge-expired");
    seed(&d);
    let id = store_for(&d, Scope::Personal)
        .create_recommendation(
            &rec(
                "habit_timing",
                Some("goals:a"),
                Some("2026-11-20T00:00:00Z"),
            ),
            &[ev("measured")],
        )
        .unwrap();
    let r = insight_in(&d, &cfg(), at(NOON)).unwrap();
    assert!(r.recommendation.is_none());
    assert_eq!(status(&d, &id), "expired");
}

#[test]
fn telegram_respects_the_kill_switch_setting_limit_and_failure_backoff() {
    let d = temp_dir("nudge-tg");
    let id = make(&d, "habit_timing", Some("goals:a"), &["measured"]);
    let sends = AtomicUsize::new(0);
    let send = |o: TgOutcome| {
        let s = &sends;
        move |text: String| {
            assert!(text.starts_with("Amara suggestion: Move meditation to mornings."));
            s.fetch_add(1, Ordering::SeqCst);
            async move { o }
        }
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    // Kill switch: nothing sent, nothing recorded.
    assert_eq!(
        rt.block_on(telegram_in(
            &d,
            &cfg(),
            at(NOON),
            true,
            send(TgOutcome::Sent)
        )),
        None
    );
    let mut no_tg = cfg();
    no_tg.telegram = false;
    assert_eq!(
        rt.block_on(telegram_in(
            &d,
            &no_tg,
            at(NOON),
            false,
            send(TgOutcome::Sent)
        )),
        None
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert_eq!(status(&d, &id), "proposed");
    // A failed send records nothing shown and waits 30 minutes.
    assert_eq!(
        rt.block_on(telegram_in(
            &d,
            &cfg(),
            at(NOON),
            false,
            send(TgOutcome::Failed)
        )),
        Some(TgOutcome::Failed)
    );
    assert_eq!(status(&d, &id), "proposed");
    assert_eq!(
        rt.block_on(telegram_in(
            &d,
            &cfg(),
            at("2026-11-25T12:10:00+09:00"),
            false,
            send(TgOutcome::Sent)
        )),
        None
    );
    // Then it sends once, records shown, and never sends the same one again.
    assert_eq!(
        rt.block_on(telegram_in(
            &d,
            &cfg(),
            at("2026-11-25T12:31:00+09:00"),
            false,
            send(TgOutcome::Sent)
        )),
        Some(TgOutcome::Sent)
    );
    assert_eq!(status(&d, &id), "shown");
    assert_eq!(
        rt.block_on(telegram_in(
            &d,
            &cfg(),
            at("2026-11-25T13:31:00+09:00"),
            false,
            send(TgOutcome::Sent)
        )),
        None
    );
    assert_eq!(sends.load(Ordering::SeqCst), 2);
}
