//! Habit days (decision 0045 item 3): `<data dir>/habits/days.json`, one row per habit and JST day saying
//! whether it was done. A habit itself is a `goals.json` row with category "habit" (adds.rs, goalStore.ts);
//! this store only holds the marks. Marking goes through `adds::logged_write`, so every change is in History
//! with the 8 s Undo like the other writes (`add_undo("habit", id)`).
//!
//! Rules: a mark is for today or up to 7 days back, never a future day; marking the same state again changes
//! nothing and logs nothing; a day with no row is "not marked", which the streak and the missed flag treat as
//! not done (only for days on or after the habit was created).

use super::adds::{self, Dirs, UndoRef};
use super::goals::Goal;
use super::local_store::{clean_slug, day_of, iso, now_jst, read_json, write_json_atomic};
use super::shared::task::{blocking, JoinErr};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAX_BACK_DAYS: i64 = 7;
const MAX_ROWS: usize = 15_000;
const MAX_BYTES: u64 = 6 * 1024 * 1024;
/// The longest window `habit_days` will return (a year plus a little).
const MAX_SPAN_DAYS: i64 = 400;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HabitDayRow {
    pub habit_id: String,
    /// JST `YYYY-MM-DD`.
    pub day: String,
    pub done: bool,
    /// "manual" | "chat" | "telegram".
    pub source: String,
    /// When the mark was made (JST, ISO).
    pub at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HabitMarkResult {
    /// False when the day already had that state: nothing was written and nothing is in History.
    pub changed: bool,
    /// The History id Undo takes (`add_undo("habit", id)`); `None` when nothing changed.
    pub id: Option<String>,
    pub habit_id: String,
    pub day: String,
    pub done: bool,
    pub toast: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    days: Vec<HabitDayRow>,
}

pub(crate) fn dir_in(data_dir: &Path) -> PathBuf {
    data_dir.join("habits")
}

fn file(dir: &Path) -> PathBuf {
    dir.join("days.json")
}

fn parse_day(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| "habit: invalid day".to_string())
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn clean_source(s: &str) -> Result<&'static str, String> {
    match s {
        "manual" => Ok("manual"),
        "chat" => Ok("chat"),
        "telegram" => Ok("telegram"),
        _ => Err("habit: invalid source".into()),
    }
}

/// Today or up to `MAX_BACK_DAYS` days back; never a future day.
pub(crate) fn check_markable(day: NaiveDate, today: NaiveDate) -> Result<(), String> {
    if day > today {
        return Err("habit: that day hasn't happened yet".into());
    }
    if (today - day).num_days() > MAX_BACK_DAYS {
        return Err("habit: too far back to mark".into());
    }
    Ok(())
}

/// Monday of the week `d` falls in (the weekly review runs Mon to Sun).
pub(crate) fn week_start(d: NaiveDate) -> NaiveDate {
    d - Duration::days(d.weekday().num_days_from_monday() as i64)
}

pub(crate) fn rows_in(dir: &Path) -> Result<Vec<HabitDayRow>, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store: Store = read_json(&file(dir), MAX_BYTES)?;
    Ok(store.days)
}

/// Rows with `from <= day <= to`, oldest first.
pub(crate) fn range_in(dir: &Path, from: &str, to: &str) -> Result<Vec<HabitDayRow>, String> {
    let (a, b) = (parse_day(from)?, parse_day(to)?);
    if a > b || (b - a).num_days() > MAX_SPAN_DAYS {
        return Err("habit: invalid range".into());
    }
    let mut rows: Vec<HabitDayRow> = rows_in(dir)?
        .into_iter()
        .filter(|r| parse_day(&r.day).is_ok_and(|d| d >= a && d <= b))
        .collect();
    rows.sort_by(|x, y| x.day.cmp(&y.day).then(x.habit_id.cmp(&y.habit_id)));
    Ok(rows)
}

/// Sets one habit/day. Returns the row it replaced (`None` if the day had no row). Writes nothing when the
/// day already holds `done`.
pub(crate) fn set_in(
    dir: &Path,
    habit_id: &str,
    day: NaiveDate,
    done: bool,
    source: &str,
    now: DateTime<FixedOffset>,
) -> Result<(bool, Option<HabitDayRow>), String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store: Store = read_json(&file(dir), MAX_BYTES)?;
    let d = ymd(day);
    let pos = store
        .days
        .iter()
        .position(|r| r.habit_id == habit_id && r.day == d);
    if pos.is_some_and(|i| store.days[i].done == done) {
        return Ok((false, None));
    }
    let row = HabitDayRow {
        habit_id: habit_id.to_string(),
        day: d,
        done,
        source: source.to_string(),
        at: iso(&now),
    };
    let prev = match pos {
        Some(i) => Some(std::mem::replace(&mut store.days[i], row)),
        None => {
            if store.days.len() >= MAX_ROWS {
                return Err("The habit log is full.".into());
            }
            store.days.push(row);
            None
        }
    };
    write_json_atomic(&file(dir), &store, MAX_BYTES)?;
    Ok((true, prev))
}

/// Undo: puts the day back to `prev` (`None` removes the row).
pub(crate) fn restore_in(
    dir: &Path,
    habit_id: &str,
    day: &str,
    prev: Option<HabitDayRow>,
) -> Result<(), String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store: Store = read_json(&file(dir), MAX_BYTES)?;
    store
        .days
        .retain(|r| !(r.habit_id == habit_id && r.day == day));
    if let Some(p) = prev {
        store.days.push(p);
    }
    write_json_atomic(&file(dir), &store, MAX_BYTES)
}

fn habit_in(dirs: &Dirs, habit_id: &str) -> Result<Goal, String> {
    let goals: Vec<Goal> = read_json(&dirs.goals(), adds::MAX_MD_BYTES)?;
    goals
        .into_iter()
        .find(|g| g.id == habit_id && g.category == "habit")
        .ok_or_else(|| "habit: not found".to_string())
}

/// The one write path: History + 8 s Undo through `adds::logged_write`.
pub(crate) fn mark_in(
    dirs: &Dirs,
    habit_id: &str,
    day: &str,
    done: bool,
    source: &str,
    now: DateTime<FixedOffset>,
) -> Result<HabitMarkResult, String> {
    let habit_id = clean_slug(habit_id, 80).or_else(|_| {
        // Console-made ids carry a mixed-case base36 suffix; allow plain alphanumerics too.
        if !habit_id.is_empty()
            && habit_id.len() <= 80
            && habit_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        {
            Ok(habit_id.to_string())
        } else {
            Err("habit: invalid input".to_string())
        }
    })?;
    let source = clean_source(source)?;
    let day = parse_day(day)?;
    check_markable(day, day_of(&now))?;
    let goal = habit_in(dirs, &habit_id)?;
    let dir = dir_in(&dirs.data);
    let result = |changed: bool, id: Option<String>| HabitMarkResult {
        changed,
        id,
        habit_id: habit_id.clone(),
        day: ymd(day),
        done,
        toast: if changed {
            "Saved. Undo is open for a few seconds.".into()
        } else {
            "Already marked.".into()
        },
    };
    // Same state again: no write and no History entry.
    if rows_in(&dir)?
        .iter()
        .any(|r| r.habit_id == habit_id && r.day == ymd(day) && r.done == done)
    {
        return Ok(result(false, None));
    }
    let via = match source {
        "manual" => "typed",
        _ => "chat",
    };
    let title: String = goal.title.chars().take(80).collect();
    let text = format!(
        "Marked {title} {} for {}",
        if done { "done" } else { "not done" },
        day.format("%b %-d")
    );
    let (id, _) = adds::logged_write("habit", text, Some(via), dirs, now, || {
        let (_, prev) = set_in(&dir, &habit_id, day, done, source, now)?;
        Ok((
            (),
            UndoRef::Habit {
                habit_id: habit_id.clone(),
                day: ymd(day),
                prev,
            },
        ))
    })?;
    Ok(result(true, Some(id)))
}

/// Used by `adds::revert`.
pub(crate) fn revert(
    dirs: &Dirs,
    habit_id: &str,
    day: &str,
    prev: &Option<HabitDayRow>,
) -> Result<(), String> {
    restore_in(&dir_in(&dirs.data), habit_id, day, prev.clone())
}

// ---- what the marks add up to ----

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HabitStats {
    /// Days marked done from Monday to today.
    pub done_week: u32,
    /// Days a week it is meant for (1..=7).
    pub target: u32,
    /// Consecutive done days ending today (or yesterday, while today isn't marked yet).
    pub streak: u32,
    /// `None` = not marked today.
    pub today: Option<bool>,
    /// Neither of the last two days is done, both on or after the day the habit was created.
    pub missed_last_2: bool,
}

pub(crate) fn target_of(g: &Goal) -> u32 {
    if g.target.is_finite() && g.target >= 1.0 {
        (g.target.round() as u32).min(7)
    } else {
        7
    }
}

fn created_day(g: &Goal) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(&g.created_at)
        .ok()
        .map(|t| day_of(&t))
}

pub(crate) fn stats(g: &Goal, rows: &[HabitDayRow], today: NaiveDate) -> HabitStats {
    let state = |d: NaiveDate| -> Option<bool> {
        let key = ymd(d);
        rows.iter()
            .find(|r| r.habit_id == g.id && r.day == key)
            .map(|r| r.done)
    };
    let done_week = (0..7)
        .map(|i| week_start(today) + Duration::days(i))
        .filter(|d| *d <= today && state(*d) == Some(true))
        .count() as u32;
    let today_state = state(today);
    let mut streak = 0;
    let mut d = match today_state {
        Some(false) => today + Duration::days(1), // an explicit "not done" today ends it
        Some(true) => today,
        None => today - Duration::days(1),
    };
    while d <= today && state(d) == Some(true) {
        streak += 1;
        d -= Duration::days(1);
        if streak > MAX_SPAN_DAYS as u32 {
            break;
        }
    }
    let (y1, y2) = (today - Duration::days(1), today - Duration::days(2));
    let missed_last_2 = created_day(g).is_some_and(|c| c <= y2)
        && state(y1) != Some(true)
        && state(y2) != Some(true);
    HabitStats {
        done_week,
        target: target_of(g),
        streak,
        today: today_state,
        missed_last_2,
    }
}

// ---- commands ----

/// Marks a habit done (or not done) for `day` (today, or up to 7 days back). History + 8 s Undo.
/// `source` is "manual", "chat" or "telegram".
#[tauri::command]
#[specta::specta]
pub async fn habit_mark(
    habit_id: String,
    day: String,
    done: bool,
    source: String,
) -> Result<HabitMarkResult, String> {
    blocking(JoinErr::Plain, move || {
        mark_in(&Dirs::real(), &habit_id, &day, done, &source, now_jst())
    })
    .await
}

/// The marks with `from <= day <= to` (JST `YYYY-MM-DD`, at most 400 days), oldest first.
#[tauri::command]
#[specta::specta]
pub async fn habit_days(from: String, to: String) -> Result<Vec<HabitDayRow>, String> {
    blocking(JoinErr::Plain, move || {
        range_in(&dir_in(&crate::app_paths::data_dir()), &from, &to)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32, h: u32) -> DateTime<FixedOffset> {
        super::super::shared::time::jst()
            .with_ymd_and_hms(2026, 11, day, h, 0, 0)
            .unwrap()
    }
    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap()
    }
    fn dirs(tag: &str) -> Dirs {
        let data = temp_dir(tag);
        Dirs {
            workspace: data.join("ws"),
            legacy: data.join("legacy"),
            data,
        }
    }
    fn habit(id: &str, created_day: u32, target: f64) -> Goal {
        Goal {
            id: id.into(),
            title: format!("Habit {id}"),
            category: "habit".into(),
            target,
            current: 0.0,
            unit: "daily".into(),
            deadline: None,
            created_at: at(created_day, 9).to_rfc3339(),
            updated_at: String::new(),
            venture_id: None,
            why: None,
            remind_at: None,
            paused: false,
        }
    }
    fn seed(dirs: &Dirs, goals: &[Goal]) {
        write_json_atomic(&dirs.goals(), &goals.to_vec(), adds::MAX_MD_BYTES).unwrap();
    }
    fn row(id: &str, day: u32, done: bool) -> HabitDayRow {
        HabitDayRow {
            habit_id: id.into(),
            day: ymd(d(day)),
            done,
            source: "manual".into(),
            at: at(day, 21).to_rfc3339(),
        }
    }

    #[test]
    fn marking_is_idempotent_and_logs_only_a_change() {
        let dr = dirs("hb1");
        seed(&dr, &[habit("h1", 1, 5.0)]);
        let a = mark_in(&dr, "h1", "2026-11-18", true, "manual", at(18, 21)).unwrap();
        assert!(a.changed && a.id.is_some());
        let b = mark_in(&dr, "h1", "2026-11-18", true, "chat", at(18, 22)).unwrap();
        assert!(!b.changed && b.id.is_none());
        assert_eq!(rows_in(&dir_in(&dr.data)).unwrap().len(), 1);
        assert_eq!(rows_in(&dir_in(&dr.data)).unwrap()[0].source, "manual");
        assert_eq!(adds::history_in("habit", &dr).unwrap().len(), 1);
        // flipping to not-done replaces the row, still one row per habit/day
        let c = mark_in(&dr, "h1", "2026-11-18", false, "manual", at(18, 23)).unwrap();
        assert!(c.changed);
        let rows = rows_in(&dir_in(&dr.data)).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].done);
        let _ = std::fs::remove_dir_all(dr.data);
    }

    #[test]
    fn undo_restores_the_previous_state_within_8_seconds() {
        let dr = dirs("hb2");
        seed(&dr, &[habit("h1", 1, 5.0)]);
        let t = at(18, 21);
        let a = mark_in(&dr, "h1", "2026-11-18", true, "manual", t).unwrap();
        assert!(adds::undo_in(
            "habit",
            a.id.as_deref().unwrap(),
            &dr,
            t + Duration::seconds(3)
        )
        .unwrap());
        assert!(rows_in(&dir_in(&dr.data)).unwrap().is_empty());
        assert!(adds::history_in("habit", &dr).unwrap().is_empty());
        // undo of a flip puts the earlier mark back
        mark_in(&dr, "h1", "2026-11-18", true, "manual", t).unwrap();
        let f = mark_in(&dr, "h1", "2026-11-18", false, "chat", t).unwrap();
        assert!(adds::undo_in("habit", f.id.as_deref().unwrap(), &dr, t).unwrap());
        let rows = rows_in(&dir_in(&dr.data)).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].done);
        // after the window it is refused
        let g = mark_in(&dr, "h1", "2026-11-17", true, "manual", t).unwrap();
        assert!(!adds::undo_in(
            "habit",
            g.id.as_deref().unwrap(),
            &dr,
            t + Duration::seconds(9)
        )
        .unwrap());
        let _ = std::fs::remove_dir_all(dr.data);
    }

    #[test]
    fn back_dating_is_limited_and_the_future_is_refused() {
        let dr = dirs("hb3");
        seed(&dr, &[habit("h1", 1, 7.0)]);
        let now = at(18, 12);
        assert!(mark_in(&dr, "h1", "2026-11-18", true, "manual", now).is_ok());
        assert!(mark_in(&dr, "h1", "2026-11-11", true, "manual", now).is_ok());
        assert_eq!(
            mark_in(&dr, "h1", "2026-11-10", true, "manual", now).unwrap_err(),
            "habit: too far back to mark"
        );
        assert_eq!(
            mark_in(&dr, "h1", "2026-11-19", true, "manual", now).unwrap_err(),
            "habit: that day hasn't happened yet"
        );
        assert!(mark_in(&dr, "h1", "2026-02-30", true, "manual", now).is_err());
        assert!(mark_in(&dr, "h1", "2026-11-18", true, "voice", now).is_err());
        assert!(mark_in(&dr, "nope", "2026-11-18", true, "manual", now).is_err());
        assert!(mark_in(&dr, "../x", "2026-11-18", true, "manual", now).is_err());
        let _ = std::fs::remove_dir_all(dr.data);
    }

    #[test]
    fn only_habits_can_be_marked() {
        let dr = dirs("hb4");
        let mut g = habit("g1", 1, 5.0);
        g.category = "health".into();
        seed(&dr, &[g]);
        assert_eq!(
            mark_in(&dr, "g1", "2026-11-18", true, "manual", at(18, 9)).unwrap_err(),
            "habit: not found"
        );
        let _ = std::fs::remove_dir_all(dr.data);
    }

    #[test]
    fn range_filters_sorts_and_bounds() {
        let dr = dirs("hb5");
        let dir = dir_in(&dr.data);
        for (id, day) in [("b", 12), ("a", 12), ("a", 10), ("a", 20)] {
            set_in(&dir, id, d(day), true, "manual", at(day, 9)).unwrap();
        }
        let r = range_in(&dir, "2026-11-10", "2026-11-12").unwrap();
        assert_eq!(
            r.iter()
                .map(|x| format!("{}{}", x.habit_id, &x.day[8..]))
                .collect::<Vec<_>>(),
            ["a10", "a12", "b12"]
        );
        assert!(range_in(&dir, "2026-11-12", "2026-11-10").is_err());
        assert!(range_in(&dir, "2024-01-01", "2026-11-10").is_err());
        assert!(range_in(&dir, "x", "2026-11-10").is_err());
        let _ = std::fs::remove_dir_all(dr.data);
    }

    #[test]
    fn a_corrupt_file_is_an_error_and_is_never_overwritten() {
        let dr = dirs("hb6");
        seed(&dr, &[habit("h1", 1, 5.0)]);
        let dir = dir_in(&dr.data);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(file(&dir), "{ not json").unwrap();
        assert!(mark_in(&dr, "h1", "2026-11-18", true, "manual", at(18, 9)).is_err());
        assert!(range_in(&dir, "2026-11-01", "2026-11-18").is_err());
        assert_eq!(std::fs::read_to_string(file(&dir)).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(dr.data);
    }

    #[test]
    fn the_store_is_capped_and_0600() {
        let dr = dirs("hb7");
        let dir = dir_in(&dr.data);
        set_in(&dir, "h1", d(18), true, "manual", at(18, 9)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(file(&dir)).unwrap().permissions().mode();
            assert_eq!(m & 0o777, 0o600);
        }
        let big = Store {
            days: (0..MAX_ROWS)
                .map(|i| HabitDayRow {
                    habit_id: format!("h{i}"),
                    day: "2026-11-18".into(),
                    done: true,
                    source: "manual".into(),
                    at: String::new(),
                })
                .collect(),
        };
        write_json_atomic(&file(&dir), &big, MAX_BYTES * 4).unwrap();
        assert_eq!(
            set_in(&dir, "new", d(18), true, "manual", at(18, 9)).unwrap_err(),
            "The habit log is full."
        );
        let _ = std::fs::remove_dir_all(dr.data);
    }

    #[test]
    fn old_goals_files_load_unchanged_and_the_new_fields_round_trip() {
        let old = r#"[{"id":"g1","title":"Walk","category":"habit","target":5.0,"current":0.0,"unit":"daily","deadline":null,"created_at":"2026-11-01T09:00:00+09:00","updated_at":"2026-11-01T09:00:00+09:00"}]"#;
        let g: Vec<Goal> = serde_json::from_str(old).unwrap();
        assert_eq!(
            (g[0].why.clone(), g[0].remind_at.clone(), g[0].paused),
            (None, None, false)
        );
        // a row with no timestamps (written by the console) loads too
        let bare = r#"[{"id":"g2","title":"x","category":"habit","target":1,"current":0,"unit":"","deadline":null}]"#;
        assert!(serde_json::from_str::<Vec<Goal>>(bare).is_ok());
        // an unchanged old row serializes without the new keys
        let out = serde_json::to_string(&g).unwrap();
        assert!(!out.contains("why") && !out.contains("remind_at") && !out.contains("paused"));
        let mut g2 = g[0].clone();
        g2.why = Some("Clear head".into());
        g2.remind_at = Some("21:30".into());
        g2.paused = true;
        let back: Vec<Goal> = serde_json::from_str(&serde_json::to_string(&[g2]).unwrap()).unwrap();
        assert_eq!(back[0].remind_at.as_deref(), Some("21:30"));
        assert!(back[0].paused);
    }

    #[test]
    fn saving_goals_stamps_missing_times_and_drops_a_bad_reminder() {
        let dr = dirs("hb8");
        let mut g = habit("h1", 1, 5.0);
        g.created_at = String::new();
        g.remind_at = Some("25:00".into());
        g.why = Some("  keeps me\n steady ".into());
        super::super::goals::save_goals_at(&dr.goals(), vec![g]).unwrap();
        let saved: Vec<Goal> = read_json(&dr.goals(), adds::MAX_MD_BYTES).unwrap();
        assert!(!saved[0].created_at.is_empty());
        assert_eq!(saved[0].remind_at, None);
        assert_eq!(saved[0].why.as_deref(), Some("keeps me steady"));
        let _ = std::fs::remove_dir_all(dr.data);
    }

    #[test]
    fn stats_count_the_week_the_streak_and_the_missed_flag() {
        // 2026-11-19 is a Thursday: the week is Nov 16..=19.
        let g = habit("h", 1, 5.0);
        let rows = vec![
            row("h", 15, true), // Sunday: last week, not in done_week
            row("h", 16, true),
            row("h", 17, true),
            row("h", 18, true),
            row("other", 19, true),
        ];
        let s = stats(&g, &rows, d(19));
        assert_eq!((s.done_week, s.target, s.streak, s.today), (3, 5, 4, None));
        assert!(!s.missed_last_2);
        let mut rows = rows;
        rows.push(row("h", 19, true));
        let s = stats(&g, &rows, d(19));
        assert_eq!((s.done_week, s.streak, s.today), (4, 5, Some(true)));
        // an explicit not-done today ends the streak
        rows.retain(|r| !(r.habit_id == "h" && r.day == "2026-11-19"));
        rows.push(row("h", 19, false));
        assert_eq!(stats(&g, &rows, d(19)).streak, 0);
        // two unmarked days in a row
        let s = stats(&g, &[row("h", 16, true)], d(19));
        assert!(s.missed_last_2);
        assert_eq!(s.streak, 0);
        // one done day clears it
        assert!(!stats(&g, &[row("h", 17, true)], d(19)).missed_last_2);
        // a habit created yesterday has not had two days to miss
        assert!(!stats(&habit("h", 18, 5.0), &[], d(19)).missed_last_2);
        // target is 7 when unset
        assert_eq!(target_of(&habit("h", 1, 0.0)), 7);
        assert_eq!(target_of(&habit("h", 1, 3.0)), 3);
    }

    #[test]
    fn week_starts_on_monday() {
        assert_eq!(week_start(d(19)), d(16));
        assert_eq!(week_start(d(16)), d(16));
        assert_eq!(week_start(d(22)), d(16));
    }
}
