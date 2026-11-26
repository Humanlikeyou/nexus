use super::*;
use crate::commands::local_store::tests::temp_dir;

const T0: &str = "2026-11-26T10:00:00+09:00";
fn at(secs: i64) -> String {
    (DateTime::parse_from_rfc3339(T0).unwrap() + chrono::Duration::seconds(secs)).to_rfc3339()
}
fn lib() -> Vec<Exercise> {
    built_in().1.clone()
}
fn item(id: &str, sets: u32, reps: Option<u32>, hold: Option<u32>, rest: u32) -> TemplateItem {
    TemplateItem {
        exercise_id: id.into(),
        sets,
        reps,
        hold_s: hold,
        load_kg: None,
        rest_s: rest,
    }
}
fn template(items: Vec<TemplateItem>) -> Template {
    Template {
        id: "wt-1".into(),
        name: "Pull day".into(),
        source: "user".into(),
        source_ref: None,
        items,
        rest_between_s: 120,
        archived: false,
        created_at: String::new(),
        updated_at: String::new(),
    }
}
fn log(reps: u32) -> Action {
    Action::LogSet {
        reps: Some(reps),
        hold_s: None,
        load_kg: None,
    }
}

#[test]
fn the_built_in_library_is_valid_calisthenics() {
    let (version, ex) = built_in();
    assert_eq!(version, "calisthenics-v1-0062");
    assert!(ex.len() >= 30);
    let mut ids = std::collections::BTreeSet::new();
    for e in ex {
        validate_exercise(e).unwrap_or_else(|m| panic!("{}: {m}", e.id));
        assert!(ids.insert(e.id.clone()), "duplicate {}", e.id);
        assert!(
            !e.form.is_empty() && !e.safety.is_empty(),
            "{} needs form and safety guidance",
            e.id
        );
        assert_eq!(
            e.demo_asset.as_deref(),
            Some(format!("demo:{}", e.id).as_str())
        );
        assert!(matches!(
            e.family.as_str(),
            "push" | "pull" | "dip" | "legs" | "core" | "skill"
        ));
        assert!(
            e.equipment
                .iter()
                .all(|q| !q.contains("barbell") && !q.contains("machine")),
            "{} is calisthenics",
            e.id
        );
    }
    // Each family is a progression, easiest first.
    for fam in ["push", "pull", "dip", "legs", "core"] {
        assert!(
            ex.iter().filter(|e| e.family == fam).any(|e| e.level == 1),
            "{fam} starts at level 1"
        );
    }
}

#[test]
fn the_estimate_is_work_plus_rest_with_no_rest_after_the_last_set() {
    let l = lib();
    // pull-up 3 s/rep: 3 sets x 6 reps = 54 s + 2 x 90 s rest = 234; plank 3 x 30 s hold = 90 + 2 x 60 = 210; between 120.
    let items = vec![
        item("pull-up", 3, Some(6), None, 90),
        item("plank", 3, None, Some(30), 60),
    ];
    assert_eq!(estimate_s(&items, 120, &l), 234 + 210 + 120);
    assert_eq!(estimate_s(&items[..1], 120, &l), 234);
    // Unknown exercises fall back to 3 s per rep.
    assert_eq!(estimate_s(&[item("nope", 1, Some(10), None, 0)], 0, &l), 30);
}

#[test]
fn templates_are_validated_against_the_library_and_each_exercise_measure() {
    let l = lib();
    assert!(validate_template(&template(vec![item("pull-up", 3, Some(6), None, 90)]), &l).is_ok());
    for bad in [
        template(vec![]),
        template(vec![item("pull-up", 3, None, Some(20), 90)]),
        template(vec![item("plank", 3, Some(10), None, 60)]),
        template(vec![item("not-an-exercise", 3, Some(6), None, 90)]),
        template(vec![item("pull-up", 0, Some(6), None, 90)]),
        template(vec![TemplateItem {
            load_kg: Some(-5.0),
            ..item("pull-up", 3, Some(6), None, 90)
        }]),
        Template {
            source: "cloud".into(),
            ..template(vec![item("pull-up", 3, Some(6), None, 90)])
        },
    ] {
        assert!(validate_template(&bad, &l).is_err(), "{bad:?}");
    }
}

#[test]
fn a_full_session_runs_the_state_machine_and_completes() {
    let l = lib();
    let t = template(vec![
        item("pull-up", 2, Some(6), None, 90),
        item("plank", 1, None, Some(30), 60),
    ]);
    let mut s = plan_from(&t, "ws-1".into(), T0);
    assert_eq!(s.state, "planned");
    assert!(
        apply(&mut s, &log(6), &l, &at(1)).is_err(),
        "can't log before starting"
    );
    apply(&mut s, &Action::Start, &l, &at(0)).unwrap();
    apply(&mut s, &log(6), &l, &at(20)).unwrap();
    apply(&mut s, &Action::Pause, &l, &at(30)).unwrap();
    assert!(
        apply(&mut s, &log(6), &l, &at(31)).is_err(),
        "can't log while paused"
    );
    apply(&mut s, &Action::Resume, &l, &at(90)).unwrap();
    apply(
        &mut s,
        &Action::LogSet {
            reps: Some(5),
            hold_s: None,
            load_kg: Some(10.0),
        },
        &l,
        &at(150),
    )
    .unwrap();
    assert_eq!(
        s.sets[1].rest_before_s,
        Some(70),
        "130 s since the last set, minus 60 s paused"
    );
    assert!(
        apply(&mut s, &log(5), &l, &at(200)).is_err(),
        "a hold item needs seconds"
    );
    apply(
        &mut s,
        &Action::LogSet {
            reps: None,
            hold_s: Some(30),
            load_kg: None,
        },
        &l,
        &at(300),
    )
    .unwrap();
    assert_eq!(s.state, "completed", "the last set ends the workout");
    assert_eq!(s.ended_at.as_deref(), Some(at(300).as_str()));
    let sum = summary(&s, &l, &at(999));
    assert_eq!(
        (sum.planned_sets, sum.done_sets, sum.completion_pct),
        (3, 3, 100.0)
    );
    assert_eq!((sum.total_reps, sum.total_hold_s), (11, 30));
    assert_eq!(sum.load_volume_kg, 50.0, "5 reps x 10 kg");
    assert_eq!(sum.paused_s, 60);
    assert_eq!(sum.active_s, Some(240));
    assert_eq!(sum.sets_by_muscle["lats"], 2);
    assert!(
        apply(&mut s, &Action::Start, &l, &at(400)).is_err(),
        "an ended workout never changes"
    );
}

#[test]
fn skipping_and_ending_early_give_partial_or_abandoned() {
    let l = lib();
    let t = template(vec![
        item("pull-up", 3, Some(6), None, 90),
        item("dip", 2, Some(8), None, 90),
    ]);
    let mut s = plan_from(&t, "ws-2".into(), T0);
    apply(&mut s, &Action::Start, &l, &at(0)).unwrap();
    apply(&mut s, &log(6), &l, &at(30)).unwrap();
    apply(
        &mut s,
        &Action::Skip {
            scope: "exercise".into(),
        },
        &l,
        &at(40),
    )
    .unwrap();
    assert_eq!((s.next_item, s.next_set), (1, 0));
    apply(
        &mut s,
        &Action::Skip {
            scope: "set".into(),
        },
        &l,
        &at(50),
    )
    .unwrap();
    assert!(apply(
        &mut s,
        &Action::Skip {
            scope: "workout".into()
        },
        &l,
        &at(55)
    )
    .is_err());
    apply(&mut s, &Action::End { abandon: false }, &l, &at(60)).unwrap();
    assert_eq!(s.state, "partially_completed");
    let sum = summary(&s, &l, &at(60));
    assert_eq!(
        (sum.done_sets, sum.skipped_sets, sum.planned_sets),
        (1, 3, 5)
    );
    assert_eq!(sum.completion_pct, 20.0);

    let mut never = plan_from(&t, "ws-3".into(), T0);
    apply(&mut never, &Action::End { abandon: true }, &l, &at(5)).unwrap();
    assert_eq!(never.state, "abandoned");
    let mut skipped_all = plan_from(&t, "ws-4".into(), T0);
    apply(&mut skipped_all, &Action::Start, &l, &at(0)).unwrap();
    apply(
        &mut skipped_all,
        &Action::Skip {
            scope: "exercise".into(),
        },
        &l,
        &at(1),
    )
    .unwrap();
    apply(
        &mut skipped_all,
        &Action::Skip {
            scope: "exercise".into(),
        },
        &l,
        &at(2),
    )
    .unwrap();
    assert_eq!(
        skipped_all.state, "abandoned",
        "skipping everything is not completing"
    );
}

#[test]
fn ending_while_paused_closes_the_pause() {
    let l = lib();
    let mut s = plan_from(
        &template(vec![item("pull-up", 2, Some(6), None, 90)]),
        "ws-5".into(),
        T0,
    );
    apply(&mut s, &Action::Start, &l, &at(0)).unwrap();
    apply(&mut s, &log(6), &l, &at(20)).unwrap();
    apply(&mut s, &Action::Pause, &l, &at(30)).unwrap();
    apply(&mut s, &Action::End { abandon: false }, &l, &at(100)).unwrap();
    assert_eq!(s.pauses[0].to.as_deref(), Some(at(100).as_str()));
    let sum = summary(&s, &l, &at(500));
    assert_eq!((sum.paused_s, sum.active_s), (70, Some(30)));
}

#[test]
fn the_store_round_trips_and_keeps_one_open_session() {
    let d = temp_dir("workout-store");
    let t = save_template_in(
        &d,
        Template {
            id: String::new(),
            ..template(vec![item("push-up", 2, Some(10), None, 60)])
        },
        T0,
    )
    .unwrap();
    assert!(t.id.starts_with("wt-"));
    let v = plan_in(&d, &t.id, None, T0).unwrap();
    assert!(plan_in(&d, &t.id, None, T0)
        .unwrap_err()
        .contains("still open"));
    act_in(&d, &v.session.id, &Action::Start, &at(0)).unwrap();
    act_in(&d, &v.session.id, &log(10), &at(30)).unwrap();
    let (done, just_ended) = act_in(&d, &v.session.id, &log(9), &at(120)).unwrap();
    assert!(just_ended);
    assert_eq!(done.session.state, "completed");
    let h = history_in(&d, 10, &at(200)).unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].summary.total_reps, 19);
    // A later template edit never changes the recorded plan.
    save_template_in(
        &d,
        Template {
            items: vec![item("push-up", 5, Some(20), None, 60)],
            ..t.clone()
        },
        &at(300),
    )
    .unwrap();
    assert_eq!(
        history_in(&d, 10, &at(400)).unwrap()[0].session.plan[0].sets,
        2
    );
    assert!(
        plan_in(&d, &t.id, None, &at(500)).is_ok(),
        "a new session once the last one ended"
    );
}

#[test]
fn custom_exercises_get_a_custom_id_and_cannot_shadow_built_ins() {
    let d = temp_dir("workout-custom");
    let mut e = built_in().1[0].clone();
    e.id = "ring-row".into();
    e.name = "Ring row".into();
    let saved = save_exercise_in(&d, e.clone()).unwrap();
    assert_eq!(saved.id, "custom-ring-row");
    assert!(saved.custom);
    assert!(library_in(&d)
        .unwrap()
        .iter()
        .any(|x| x.id == "custom-ring-row"));
    let mut bad = e;
    bad.instructions.clear();
    assert!(save_exercise_in(&d, bad).is_err());
}

#[test]
fn unknown_action_fields_are_refused() {
    let raw = serde_json::json!({ "kind": "logSet", "reps": 5, "endedAt": "2020-01-01T00:00:00Z" });
    assert!(
        serde_json::from_value::<Action>(raw).is_err(),
        "the screen never supplies a time"
    );
}

// ---------------------------------------------------------------- 9b

#[test]
fn every_mapped_take_exists_in_the_catalog_and_is_valid_calisthenics() {
    let catalog: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../src-console/amaraTake/catalog.json"
    ))
    .unwrap();
    let ids: Vec<String> = catalog["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_string())
        .collect();
    let l = lib();
    assert!(take_templates().len() >= 10);
    for m in take_templates() {
        assert!(
            ids.contains(&m.take_id),
            "{} is not in the take catalog",
            m.take_id
        );
        let t = Template {
            id: String::new(),
            name: m.name.clone(),
            source: "amara".into(),
            source_ref: Some(format!("take:{}", m.take_id)),
            items: m.items.clone(),
            rest_between_s: m.rest_between_s,
            archived: false,
            created_at: String::new(),
            updated_at: String::new(),
        };
        validate_template(&t, &l).unwrap_or_else(|e| panic!("{}: {e}", m.take_id));
    }
    // Timed formats, cardio and unstated counts are never guessed into sets.
    for unmapped in [
        "calisthenics_emom_10",
        "amrap_12_minutes",
        "bodyweight_tabata",
        "brisk_walk",
        "upper_push_pull",
    ] {
        assert!(
            !mapped_take_ids().contains(&unmapped.to_string()),
            "{unmapped}"
        );
    }
}

#[test]
fn an_amara_take_runs_through_the_same_engine_with_provenance() {
    let d = temp_dir("workout-take");
    let t = template_from_take_in(&d, "pushup_ladder_2", T0).unwrap();
    assert_eq!(
        (t.source.as_str(), t.source_ref.as_deref()),
        ("amara", Some("take:pushup_ladder_2"))
    );
    assert_eq!(
        template_from_take_in(&d, "pushup_ladder_2", &at(10))
            .unwrap()
            .id,
        t.id,
        "one template per take"
    );
    assert!(template_from_take_in(&d, "brisk_walk", T0).is_err());
    assert!(plan_in(&d, &t.id, Some("bad id"), T0).is_err());
    let v = plan_in(&d, &t.id, Some("sug-123"), T0).unwrap();
    assert_eq!(v.session.source, "amara");
    assert_eq!(v.session.offer_sid.as_deref(), Some("sug-123"));
    act_in(&d, &v.session.id, &Action::Start, &at(0)).unwrap();
    for (i, r) in [8, 8, 7].iter().enumerate() {
        act_in(&d, &v.session.id, &log(*r), &at(60 * (i as i64 + 1))).unwrap();
    }
    let h = history_in(&d, 5, &at(999)).unwrap();
    assert_eq!(
        (h[0].session.state.as_str(), h[0].summary.total_reps),
        ("completed", 23)
    );
}

#[test]
fn a_user_template_never_carries_an_offer_id() {
    let d = temp_dir("workout-user-offer");
    let t = save_template_in(
        &d,
        Template {
            id: String::new(),
            ..template(vec![item("push-up", 1, Some(5), None, 0)])
        },
        T0,
    )
    .unwrap();
    assert_eq!(
        plan_in(&d, &t.id, Some("sug-9"), T0)
            .unwrap()
            .session
            .offer_sid,
        None
    );
}
