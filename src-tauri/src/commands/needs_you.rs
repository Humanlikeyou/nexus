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
use super::local_store::{day_of, now_jst};
use super::{checkin, crew, decisions_log, followups, life_dates};
use chrono::NaiveDate;
use serde::Serialize;
use std::collections::HashSet;

pub const CHECKIN_REMINDER: &str = "22:00 JST";

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

/// What each source returned. `Ok(None)` is "not set up" (unwired); `Err` is "didn't load".
pub(crate) struct Inputs {
    pub today: NaiveDate,
    pub todos: Result<Vec<FollowupItem>, String>,
    pub dates: Result<Vec<LifeDateView>, String>,
    pub checkin_done: Result<bool, String>,
    pub ledger: Result<Option<BusinessLedger>, String>,
    pub decided: Result<HashSet<String>, String>,
    pub crew: Result<Option<Vec<CrewIn>>, String>,
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
            if done {
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

    vec![todos, dates, checkin, decisions, crew]
}

async fn gather() -> Inputs {
    let today = day_of(&now_jst());
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
        )
    })
    .await;
    let (dates, checkin_done, ledger, decided) = local.unwrap_or_else(|_| {
        let e = || "failed".to_string();
        (Err(e()), Err(e()), Err(e()), Err(e()))
    });
    let crew = match crew::crew_health().await.as_deref() {
        Ok("not_configured") => Ok(None),
        Ok("operational") => crew::crew_list_approvals("pending".into())
            .await
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
        todos,
        dates,
        checkin_done,
        ledger,
        decided,
        crew,
    }
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

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 11).unwrap()
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
}
