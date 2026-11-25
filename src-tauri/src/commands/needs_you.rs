//! Needs you today (design-1111v §3, HANDOFF-1111v §7.2): a read-only list of pointers over the sources.
//! Each row is `source, id, title, due, reason, deepLink` for an item that lives in exactly one place;
//! there are no inbox-only items and no mutation here. This module only reads: to-dos, life dates,
//! check-ins, the business ledger (waiting votes and tax flags) with the decisions log, and the pending
//! crew approvals. It never calls the crew decide command or any writer. Amara's own action approvals are
//! pointed to from the UI snapshot and are not read here.

use super::adds::Route;
use super::business_ledger::{read_ledger_in, BusinessLedger, RequestStatus};
use super::followups::FollowupItem;
use super::life_dates::LifeDateView;
use super::local_store::day_of;
use super::{checkin, crew, decisions_log, followups, life_dates};
use crate::commands::shared::time::now_jst;
use chrono::{DateTime, FixedOffset, NaiveDate, Timelike};
use serde::Serialize;
use std::collections::HashSet;

pub const CHECKIN_REMINDER: &str = "22:00 JST";
/// The check-in pointer appears from this hour JST (decision 0025 §1: "from afternoon"), never before.
pub const CHECKIN_SHOW_FROM_HOUR: u32 = 12;

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct Badge {
    pub tone: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Pointer {
    pub source: String,
    pub id: String,
    pub title: String,
    pub due: Option<String>,
    pub reason: String,
    pub deep_link: Route,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge: Option<Badge>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub quiet: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct SourceReport {
    pub source: String,
    /// "ok", "failed" (didn't load) or "unwired" (nothing to read yet).
    pub state: String,
    pub rows: Vec<Pointer>,
}

pub(crate) struct CrewIn {
    pub id: String,
    pub crew: String,
    pub title: String,
}

/// A habit that may need a "mark today" pointer: only its own facts, no marks.
pub(crate) struct HabitIn {
    pub id: String,
    pub title: String,
    /// `HH:MM` JST; `None` = `HABIT_DEFAULT_REMIND`.
    pub remind_at: Option<String>,
    pub paused: bool,
    pub marked_today: bool,
}

/// A habit is asked about from this time JST when it has no `remind_at` of its own.
pub const HABIT_DEFAULT_REMIND: &str = "20:00";

/// What each source returned. `Ok(None)` is "not set up" (unwired); `Err` is "didn't load".
pub(crate) struct Inputs {
    pub today: NaiveDate,
    /// The clock, in JST. Injected so tests can pick the hour.
    pub now: DateTime<FixedOffset>,
    pub todos: Result<Vec<FollowupItem>, String>,
    pub dates: Result<Vec<LifeDateView>, String>,
    pub checkin_done: Result<bool, String>,
    pub ledger: Result<Option<BusinessLedger>, String>,
    pub decided: Result<HashSet<String>, String>,
    pub crew: Result<Option<Vec<CrewIn>>, String>,
    /// Open Treasury flags, stale first. `Ok(None)` when Treasury has no database yet (unwired).
    pub treasury: Result<Option<Vec<super::treasury_monitor::Flag>>, String>,
    /// The last night's heart-rate-only check (counts only). `Ok(None)` when NOOP has no store or
    /// lacks the sample tables (unwired); `Err` when the store could not be read.
    pub strap: Result<Option<super::strap_night::StrapNight>, String>,
    /// 0038: true when the watchlist's "Want new picks?" question is due (14 days, feeds allowed).
    pub watchlist: Result<bool, String>,
    /// 0045: habits and whether today is marked. `Err` when goals or the habit log could not be read.
    pub habits: Result<Vec<HabitIn>, String>,
}

fn link(id: &str, view: &str) -> Route {
    Route {
        id: id.into(),
        view: view.into(),
    }
}

fn day_label(d: NaiveDate) -> String {
    d.format("%b %-d").to_string()
}

fn pointer(source: &str, id: String, title: String, reason: String, deep_link: Route) -> Pointer {
    Pointer {
        source: source.into(),
        id,
        title,
        due: None,
        reason,
        deep_link,
        badge: None,
        quiet: false,
    }
}

fn report(source: &str, rows: Result<Vec<Pointer>, String>) -> SourceReport {
    match rows {
        Ok(rows) => SourceReport {
            source: source.into(),
            state: "ok".into(),
            rows,
        },
        Err(_) => SourceReport {
            source: source.into(),
            state: "failed".into(),
            rows: vec![],
        },
    }
}

fn unwired(source: &str) -> SourceReport {
    SourceReport {
        source: source.into(),
        state: "unwired".into(),
        rows: vec![],
    }
}

pub(crate) fn build(i: Inputs) -> Vec<SourceReport> {
    let todos = report(
        "todos",
        i.todos.map(|items| {
            items
                .iter()
                .filter_map(|it| {
                    let due = NaiveDate::parse_from_str(it.due.as_deref()?, "%Y-%m-%d").ok()?;
                    if due > i.today {
                        return None;
                    }
                    let overdue = due < i.today;
                    let mut p = pointer(
                        "todos",
                        format!("todo-{}", it.id),
                        it.text.clone(),
                        if overdue {
                            format!("Overdue since {}", day_label(due))
                        } else {
                            "Due today".into()
                        },
                        link("terrance", "lists"),
                    );
                    p.due = it.due.clone();
                    p.badge = Some(Badge {
                        tone: if overdue { "warning" } else { "accent" }.into(),
                        label: if overdue { "Overdue" } else { "Due today" }.into(),
                    });
                    Some(p)
                })
                .collect()
        }),
    );

    let dates = report(
        "dates",
        i.dates.map(|ds| {
            ds.iter()
                .filter_map(|d| {
                    let rung = d.active_window?;
                    let when = NaiveDate::parse_from_str(&d.record.date, "%Y-%m-%d").ok()?;
                    let mut p = pointer(
                        "dates",
                        format!("date-{}", d.record.id),
                        d.record.title.clone(),
                        format!(
                            "{} · in {} days · {rung}-day reminder",
                            day_label(when),
                            d.in_days
                        ),
                        link("life", "dates"),
                    );
                    p.due = Some(d.record.date.clone());
                    Some(p)
                })
                .collect()
        }),
    );

    let checkin = report(
        "checkin",
        i.checkin_done.map(|done| {
            if done || i.now.hour() < CHECKIN_SHOW_FROM_HOUR {
                return vec![];
            }
            let mut p = pointer(
                "checkin",
                "checkin-today".into(),
                "Evening check-in".into(),
                format!("Not done yet today · reminder at {CHECKIN_REMINDER}"),
                link("terrance", "checkin"),
            );
            p.quiet = true;
            vec![p]
        }),
    );

    let decisions = match (&i.ledger, &i.decided) {
        (Err(_), _) | (_, Err(_)) => report("decisions", Err("failed".into())),
        (Ok(None), _) => unwired("decisions"),
        (Ok(Some(l)), Ok(decided)) => {
            let votes = l.funding_requests.iter().filter(|v| {
                v.status == RequestStatus::Open && !decided.contains(&v.id.to_lowercase())
            });
            let flags = l
                .tax_flags
                .iter()
                .filter(|t| !decided.contains(&t.id.to_lowercase()));
            let mut rows: Vec<Pointer> = votes
                .map(|v| {
                    let purpose = v
                        .purpose
                        .split(" (")
                        .next()
                        .unwrap_or(&v.purpose)
                        .to_lowercase();
                    pointer(
                        "decisions",
                        format!("vote-{}", v.id),
                        format!("{} {purpose} · funding vote", v.requester),
                        format!("Proposed by the {} crew", v.requester),
                        link("enterprise", "decisions"),
                    )
                })
                .collect();
            rows.extend(flags.map(|t| {
                let mut label = t.label.chars();
                let label = label
                    .next()
                    .map(|c| c.to_lowercase().collect::<String>() + label.as_str())
                    .unwrap_or_default();
                pointer(
                    "decisions",
                    format!("flag-{}", t.id),
                    format!("Tax flag · {label}"),
                    format!("{} · {} · from the business ledger", t.area, t.date),
                    link("enterprise", "decisions"),
                )
            }));
            report("decisions", Ok(rows))
        }
    };

    let crew = match i.crew {
        Err(_) => report("crew", Err("failed".into())),
        Ok(None) => unwired("crew"),
        Ok(Some(rows)) => report(
            "crew",
            Ok(rows
                .into_iter()
                .map(|r| pointer("crew", format!("crew-{}", r.id), r.title, format!("From the {} crew · waiting for your OK · nothing runs until you approve", r.crew), link("chat", "crews")))
                .collect()),
        ),
    };

    let treasury = match i.treasury {
        Err(_) => report("treasury", Err("failed".into())),
        Ok(None) => unwired("treasury"),
        Ok(Some(flags)) => report(
            "treasury",
            Ok(flags
                .into_iter()
                .map(|f| {
                    let stale = f.kind == "stale_data";
                    let mut p = pointer(
                        "treasury",
                        format!("treasury-{}", f.id),
                        treasury_title(&f.kind).into(),
                        f.detail,
                        link("enterprise", "treasury"),
                    );
                    p.due = day_part_of(&f.since);
                    if stale {
                        p.badge = Some(Badge {
                            tone: "warning".into(),
                            label: "Stale".into(),
                        });
                    }
                    p
                })
                .collect()),
        ),
    };

    // One quiet row per active habit that is still unmarked after its reminder time. Pointers only.
    let habits = report(
        "habits",
        i.habits.map(|hs| {
            let minutes = i.now.hour() * 60 + i.now.minute();
            hs.iter()
                .filter(|h| !h.paused && !h.marked_today)
                .filter(|h| {
                    let at = h.remind_at.as_deref().unwrap_or(HABIT_DEFAULT_REMIND);
                    super::goals::valid_hhmm(at)
                        .or_else(|| super::goals::valid_hhmm(HABIT_DEFAULT_REMIND))
                        .and_then(|t| {
                            let (hh, mm) = t.split_once(':')?;
                            Some(hh.parse::<u32>().ok()? * 60 + mm.parse::<u32>().ok()?)
                        })
                        .is_some_and(|m| minutes >= m)
                })
                .map(|h| {
                    let mut p = pointer(
                        "habits",
                        format!("habit-{}", h.id),
                        h.title.clone(),
                        "Not marked today".into(),
                        link("life", "goals"),
                    );
                    p.quiet = true;
                    p
                })
                .collect()
        }),
    );

    let strap = match i.strap {
        Err(_) => report("strap", Err("failed".into())),
        Ok(None) => unwired("strap"),
        Ok(Some(n)) => report(
            "strap",
            Ok(if n.hr_only {
                // Fixed text only: no counts and no health values are formatted in.
                vec![pointer(
                    "strap",
                    "strap-hr-only".into(),
                    super::strap_night::TITLE.into(),
                    if n.recovery_missing {
                        super::strap_night::DETAIL_NO_RECOVERY
                    } else {
                        super::strap_night::DETAIL
                    }
                    .into(),
                    link("mindbody", "fitness"),
                )]
            } else {
                vec![]
            }),
        ),
    };

    // One fixed-text pointer and nothing else: no tickers, prices or headlines.
    let watchlist = report(
        "watchlist",
        i.watchlist.map(|due| {
            if due {
                vec![pointer(
                    "watchlist",
                    super::watchlist::POINTER_ID.into(),
                    super::watchlist::POINTER_TITLE.into(),
                    super::watchlist::POINTER_DETAIL.into(),
                    link("enterprise", "ventures"),
                )]
            } else {
                vec![]
            }
        }),
    );

    vec![
        todos, dates, checkin, decisions, crew, treasury, strap, watchlist, habits,
    ]
}

fn treasury_title(kind: &str) -> &'static str {
    match kind {
        "stale_data" => "Treasury data is out of date",
        "failed_import" => "A Treasury import failed",
        "income_down" => "Treasury income is down",
        "no_income" => "No Treasury income lately",
        "results_spike" => "Email bounces or unsubscribes are up",
        "no_conversions" => "An email sequence has no conversions",
        _ => "Treasury needs a look",
    }
}

fn day_part_of(iso: &str) -> Option<String> {
    iso.get(..10).map(str::to_string)
}

async fn gather() -> Inputs {
    let now = now_jst();
    let today = day_of(&now);
    let data = crate::app_paths::data_dir();
    let todos = followups::followups_list().await.map(|l| l.open);
    let d = data.clone();
    let local = tauri::async_runtime::spawn_blocking(move || {
        (
            life_dates::list_in(&life_dates::dir_in(&d), today),
            checkin::done_on(&checkin::dir_in(&d), today),
            read_ledger_in(&d),
            decisions_log::list_in(&decisions_log::dir_in(&d)).map(|r| {
                r.into_iter()
                    .map(|x| x.item_id.to_lowercase())
                    .collect::<HashSet<_>>()
            }),
            super::treasury_monitor::needs_you_rows_in(&d),
        )
    })
    .await;
    let (dates, checkin_done, ledger, decided, treasury) = local.unwrap_or_else(|_| {
        let e = || "failed".to_string();
        (Err(e()), Err(e()), Err(e()), Err(e()), Err(e()))
    });
    let d3 = data.clone();
    let habits = tauri::async_runtime::spawn_blocking(move || habits_now(&d3, today))
        .await
        .unwrap_or_else(|_| Err("failed".to_string()));
    let strap = tauri::async_runtime::spawn_blocking(move || super::strap_night::read(now))
        .await
        .unwrap_or_else(|_| Err("failed".to_string()));
    let blocked = super::telegram::outbound_disabled();
    let d2 = data.clone();
    let watchlist = tauri::async_runtime::spawn_blocking(move || {
        super::watchlist::review_due_in(&d2, blocked, now)
    })
    .await
    .unwrap_or_else(|_| Err("failed".to_string()));
    let crew = match crew::crew_health().await.as_deref() {
        Ok("not_configured") => Ok(None),
        // A crew service built before approvals existed answers 404: nothing can be waiting, so no rows and
        // no "didn't load" line (Taisun 2026-11-19). Any other failure still shows as failed.
        Ok("operational") => crew::crew_list_approvals("pending".into())
            .await
            .or_else(|e| {
                if crew::approvals_not_supported(&e) {
                    Ok(Vec::new())
                } else {
                    Err(e)
                }
            })
            .map(|rows| {
                Some(
                    rows.into_iter()
                        .map(|r| CrewIn {
                            id: r.id,
                            crew: r.crew,
                            title: r.title,
                        })
                        .collect(),
                )
            }),
        _ => Err("offline".to_string()),
    };
    Inputs {
        today,
        now,
        todos,
        dates,
        checkin_done,
        ledger,
        decided,
        crew,
        treasury,
        strap,
        watchlist,
        habits,
    }
}

/// Habit rows from goals.json with whether today has a done mark. Read-only.
fn habits_now(data: &std::path::Path, today: NaiveDate) -> Result<Vec<HabitIn>, String> {
    let goals: Vec<super::goals::Goal> =
        super::local_store::read_json(&data.join("goals.json"), super::adds::MAX_MD_BYTES)?;
    let rows = super::habits::rows_in(&super::habits::dir_in(data))?;
    let key = today.format("%Y-%m-%d").to_string();
    Ok(goals
        .into_iter()
        .filter(|g| g.category == "habit")
        .map(|g| HabitIn {
            marked_today: rows
                .iter()
                .any(|r| r.habit_id == g.id && r.day == key && r.done),
            id: g.id,
            title: g.title,
            remind_at: g.remind_at,
            paused: g.paused,
        })
        .collect())
}

/// One report per source. A failed source never hides the others.
#[tauri::command]
#[specta::specta]
pub async fn needs_you_list() -> Result<Vec<SourceReport>, String> {
    Ok(build(gather().await))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::life_dates::{LifeDateRecord, Reminder};
    use crate::commands::shared::time::jst;
    use crate::commands::strap_night::StrapNight;
    use chrono::TimeZone;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 11).unwrap()
    }
    /// A JST instant on 2026-11-`day`.
    fn at(day: u32, h: u32, m: u32) -> DateTime<FixedOffset> {
        jst().with_ymd_and_hms(2026, 11, day, h, m, 0).unwrap()
    }
    fn todo(id: i64, due: Option<&str>) -> FollowupItem {
        FollowupItem {
            id,
            text: format!("t{id}"),
            added: None,
            due: due.map(Into::into),
            next: None,
            notes: vec![],
            last_nudged: None,
            done: None,
        }
    }
    fn date(id: &str, in_days: i64, win: Option<i64>) -> LifeDateView {
        LifeDateView {
            record: LifeDateRecord {
                id: id.into(),
                title: format!("D {id}"),
                date: "2026-11-14".into(),
                lane: "personal".into(),
                sub: String::new(),
                lead_days: vec![],
                done: false,
                goal_id: None,
            },
            in_days,
            reminders: Vec::<Reminder>::new(),
            active_window: win,
        }
    }
    fn ledger() -> BusinessLedger {
        serde_json::from_str(r#"{"version":1,"fx":null,"voteRules":null,"revenueTargetJpy":null,
          "fundingRequests":[{"id":"f1","requester":"Taiyo Skin","purpose":"Second supplier quote (deposit)","amountJpy":null,"status":"open"},{"id":"f2","requester":"X","purpose":"p","amountJpy":null,"status":"approved"}],
          "taxFlags":[{"id":"t1","label":"Home office split","area":"Tax","date":"Nov 30"}]}"#).unwrap()
    }
    fn inputs() -> Inputs {
        Inputs {
            today: today(),
            now: at(11, 15, 0),
            todos: Ok(vec![
                todo(1, Some("2026-11-09")),
                todo(2, Some("2026-11-11")),
                todo(3, Some("2026-11-12")),
                todo(4, None),
            ]),
            dates: Ok(vec![date("a", 3, Some(7)), date("b", 40, None)]),
            checkin_done: Ok(false),
            ledger: Ok(Some(ledger())),
            decided: Ok(HashSet::new()),
            crew: Ok(Some(vec![CrewIn {
                id: "r1".into(),
                crew: "Taiyo Skin".into(),
                title: "Launch post draft".into(),
            }])),
            treasury: Ok(Some(vec![
                tflag("stale_data:s1", "stale_data"),
                tflag("no_income:s1", "no_income"),
            ])),
            strap: Ok(Some(StrapNight {
                hr_only: true,
                recovery_missing: true,
            })),
            watchlist: Ok(false),
            habits: Ok(vec![]),
        }
    }
    fn tflag(id: &str, kind: &str) -> super::super::treasury_monitor::Flag {
        super::super::treasury_monitor::Flag {
            id: id.into(),
            kind: kind.into(),
            stream_id: Some("s1".into()),
            sequence_id: None,
            since: "2026-11-10T04:00:00.000+09:00".into(),
            detail: "No import for 8 days (limit 7).".into(),
            acked: false,
        }
    }
    fn by<'a>(r: &'a [SourceReport], s: &str) -> &'a SourceReport {
        r.iter().find(|x| x.source == s).unwrap()
    }

    #[test]
    fn lists_pointers_from_every_source() {
        let r = build(inputs());
        assert_eq!(
            by(&r, "todos")
                .rows
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["todo-1", "todo-2"]
        );
        assert_eq!(by(&r, "todos").rows[0].reason, "Overdue since Nov 9");
        assert_eq!(by(&r, "dates").rows.len(), 1);
        assert!(by(&r, "checkin").rows[0].quiet);
        assert_eq!(
            by(&r, "decisions")
                .rows
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["vote-f1", "flag-t1"]
        );
        assert_eq!(
            by(&r, "decisions").rows[0].title,
            "Taiyo Skin second supplier quote · funding vote"
        );
        assert_eq!(
            by(&r, "crew").rows[0].deep_link,
            Route {
                id: "chat".into(),
                view: "crews".into()
            }
        );
    }

    #[test]
    fn decided_items_and_a_done_checkin_leave() {
        let mut i = inputs();
        i.decided = Ok(["f1".to_string(), "t1".to_string()].into());
        i.checkin_done = Ok(true);
        let r = build(i);
        assert!(by(&r, "decisions").rows.is_empty());
        assert!(by(&r, "checkin").rows.is_empty());
        assert_eq!(by(&r, "checkin").state, "ok");
    }

    #[test]
    fn checkin_reminder_is_hidden_before_noon_jst() {
        let mut i = inputs();
        i.now = at(11, 11, 59);
        assert!(by(&build(i), "checkin").rows.is_empty());
        let mut i = inputs();
        i.now = at(11, 12, 0);
        let r = build(i);
        assert_eq!(by(&r, "checkin").rows.len(), 1);
        assert_eq!(by(&r, "checkin").rows[0].id, "checkin-today");
    }

    #[test]
    fn checkin_reminder_is_gone_at_three_pm_once_checked_in_today() {
        let mut i = inputs();
        i.now = at(11, 15, 0);
        i.checkin_done = Ok(true);
        let r = build(i);
        assert!(by(&r, "checkin").rows.is_empty());
        assert_eq!(by(&r, "checkin").state, "ok");
    }

    #[test]
    fn checkin_reminder_is_per_day_next_morning_hidden_then_back_at_noon() {
        let next = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let mut i = inputs();
        i.today = next;
        i.now = at(12, 9, 0);
        assert!(by(&build(i), "checkin").rows.is_empty());
        let mut i = inputs();
        i.today = next;
        i.now = at(12, 12, 0);
        assert_eq!(by(&build(i), "checkin").rows.len(), 1);
    }

    #[test]
    fn one_failed_or_unwired_source_leaves_the_rest_current() {
        let mut i = inputs();
        i.todos = Err("boom".into());
        i.ledger = Ok(None);
        i.crew = Ok(None);
        let r = build(i);
        assert_eq!(
            (by(&r, "todos").state.as_str(), by(&r, "todos").rows.len()),
            ("failed", 0)
        );
        assert_eq!(by(&r, "decisions").state, "unwired");
        assert_eq!(by(&r, "crew").state, "unwired");
        assert_eq!(by(&r, "dates").state, "ok");
        assert_eq!(by(&r, "dates").rows.len(), 1);
    }

    #[test]
    fn treasury_flags_point_stale_first_and_a_failed_treasury_hides_nothing_else() {
        let r = build(inputs());
        let t = by(&r, "treasury");
        assert_eq!(t.state, "ok");
        assert_eq!(t.rows[0].id, "treasury-stale_data:s1");
        assert_eq!(t.rows[0].badge.as_ref().unwrap().label, "Stale");
        assert_eq!(t.rows[0].due.as_deref(), Some("2026-11-10"));
        assert_eq!(t.rows[1].title, "No Treasury income lately");
        let mut i = inputs();
        i.treasury = Err("boom".into());
        let r = build(i);
        assert_eq!(by(&r, "treasury").state, "failed");
        assert_eq!(by(&r, "todos").rows.len(), 2);
        let mut i = inputs();
        i.treasury = Ok(None);
        assert_eq!(by(&build(i), "treasury").state, "unwired");
    }

    #[test]
    fn rows_only_link_and_the_module_never_writes_or_decides() {
        let r = build(inputs());
        let json = serde_json::to_value(&r).unwrap();
        for src in json.as_array().unwrap() {
            for row in src["rows"].as_array().unwrap() {
                let mut keys: Vec<_> = row.as_object().unwrap().keys().cloned().collect();
                keys.sort();
                let allowed = [
                    "badge", "deepLink", "due", "id", "quiet", "reason", "source", "title",
                ];
                assert!(
                    keys.iter().all(|k| allowed.contains(&k.as_str())),
                    "{keys:?}"
                );
            }
        }
        let src = include_str!("needs_you.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        for word in [
            "crew_decide_approval",
            "followups_mark_done",
            "followups_add",
            "followups_update",
            "followups_delete",
            "write_json_atomic",
            "upsert_in",
            "append_in",
            "decisions_append",
            "save_in(",
            "fs::write",
        ] {
            assert!(!body.contains(word), "needs_you must not use {word}");
        }
    }

    #[test]
    fn strap_pointer_is_fixed_text_links_mindbody_and_clears_on_a_normal_night() {
        let r = build(inputs());
        let s = by(&r, "strap");
        assert_eq!(s.state, "ok");
        assert_eq!(s.rows.len(), 1);
        assert_eq!(
            s.rows[0].title,
            "Last night came through as heart rate only"
        );
        assert_eq!(
            s.rows[0].reason,
            crate::commands::strap_night::DETAIL_NO_RECOVERY
        );
        assert_eq!(
            s.rows[0].deep_link,
            Route {
                id: "mindbody".into(),
                view: "fitness".into()
            }
        );
        let mut i = inputs();
        i.strap = Ok(Some(StrapNight {
            hr_only: true,
            recovery_missing: false,
        }));
        assert_eq!(
            by(&build(i), "strap").rows[0].reason,
            crate::commands::strap_night::DETAIL
        );
        let mut i = inputs();
        i.strap = Ok(Some(StrapNight {
            hr_only: false,
            recovery_missing: false,
        }));
        let r = build(i);
        assert_eq!(
            (by(&r, "strap").state.as_str(), by(&r, "strap").rows.len()),
            ("ok", 0)
        );
    }

    #[test]
    fn watchlist_pointer_is_fixed_text_and_only_when_due() {
        let mut i = inputs();
        i.watchlist = Ok(true);
        let r = build(i);
        let w = by(&r, "watchlist");
        assert_eq!(w.state, "ok");
        assert_eq!(w.rows.len(), 1);
        assert_eq!(w.rows[0].title, "Want new picks?");
        assert_eq!(w.rows[0].reason, crate::commands::watchlist::POINTER_DETAIL);
        assert_eq!(w.rows[0].deep_link.id, "enterprise");
        assert!(by(&build(inputs()), "watchlist").rows.is_empty());
        let mut i = inputs();
        i.watchlist = Err("x".into());
        assert_eq!(by(&build(i), "watchlist").state, "failed");
    }

    #[test]
    fn strap_unwired_and_failed_never_hide_other_sources() {
        let mut i = inputs();
        i.strap = Ok(None);
        assert_eq!(by(&build(i), "strap").state, "unwired");
        let mut i = inputs();
        i.strap = Err("boom".into());
        let r = build(i);
        assert_eq!(by(&r, "strap").state, "failed");
        assert!(by(&r, "strap").rows.is_empty());
        assert_eq!(by(&r, "todos").rows.len(), 2);
        assert_eq!(by(&r, "treasury").rows.len(), 2);
    }

    #[test]
    fn strap_source_text_formats_no_health_value() {
        let src = include_str!("needs_you.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        let block = body
            .split("let strap = match i.strap")
            .nth(1)
            .unwrap()
            .split("vec![todos")
            .next()
            .unwrap();
        assert!(!block.contains("format!"), "strap text must be fixed");
        assert!(!block.contains("hr:") && !block.contains("motion"));
    }

    fn hab(id: &str, remind: Option<&str>, paused: bool, marked: bool) -> HabitIn {
        HabitIn {
            id: id.into(),
            title: format!("Habit {id}"),
            remind_at: remind.map(Into::into),
            paused,
            marked_today: marked,
        }
    }

    #[test]
    fn habit_rows_wait_for_the_reminder_time_and_skip_paused_and_marked() {
        let hs = || {
            Ok(vec![
                hab("a", None, false, false),
                hab("b", Some("21:30"), false, false),
                hab("c", None, true, false),
                hab("d", None, false, true),
            ])
        };
        let mut i = inputs();
        i.habits = hs();
        i.now = at(11, 19, 59);
        assert!(by(&build(i), "habits").rows.is_empty());
        let mut i = inputs();
        i.habits = hs();
        i.now = at(11, 20, 0);
        let r = build(i);
        let rows = &by(&r, "habits").rows;
        assert_eq!(
            rows.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["habit-a"]
        );
        assert_eq!(rows[0].reason, "Not marked today");
        assert_eq!(
            rows[0].deep_link,
            Route {
                id: "life".into(),
                view: "goals".into()
            }
        );
        let mut i = inputs();
        i.habits = hs();
        i.now = at(11, 21, 30);
        let r = build(i);
        assert_eq!(
            by(&r, "habits")
                .rows
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["habit-a", "habit-b"]
        );
        let mut i = inputs();
        i.habits = Err("x".into());
        assert_eq!(by(&build(i), "habits").state, "failed");
    }

    #[test]
    fn habits_now_reads_goals_and_marks() {
        use crate::commands::local_store::tests::temp_dir;
        let d = temp_dir("nyh");
        assert!(habits_now(&d, today()).unwrap().is_empty());
        let g = r#"[{"id":"h1","title":"Walk","category":"habit","target":5,"current":0,"unit":"","deadline":null,"remind_at":"21:00","paused":true},{"id":"g1","title":"G","category":"health","target":5,"current":0,"unit":"","deadline":null}]"#;
        std::fs::write(d.join("goals.json"), g).unwrap();
        let hs = habits_now(&d, today()).unwrap();
        assert_eq!(hs.len(), 1);
        assert_eq!(
            (hs[0].paused, hs[0].remind_at.as_deref(), hs[0].marked_today),
            (true, Some("21:00"), false)
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
