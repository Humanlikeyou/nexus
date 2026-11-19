//! Life dates (design-1111v, HANDOFF-1111v §7 and §9.4.2): personal and Japan-residence dates and deadlines in
//! `<data dir>/life/dates.json`. Reminders at 90, 30 and 7 days are computed from the date when the list is
//! read. Nothing here schedules a timer or fires a notification; Needs you today and the Dates page just
//! read the computed windows. Enterprise dates are not stored here (they live in Enterprise › Ventures).

use super::local_store::{
    clean_slug, clean_text, day_of, new_id, now_jst, read_json, write_json_atomic,
};
use super::shared::task::{blocking, JoinErr};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const LADDER: [i64; 3] = [90, 30, 7];
const MAX_DATES: usize = 500;
const MAX_BYTES: u64 = 512 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LifeDateRecord {
    pub id: String,
    pub title: String,
    /// `YYYY-MM-DD`, always a real calendar day.
    pub date: String,
    /// "personal" or "residence".
    pub lane: String,
    #[serde(default)]
    pub sub: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifeDateDraft {
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    pub date: String,
    pub lane: String,
    #[serde(default)]
    pub sub: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub days: i64,
    pub on: String,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LifeDateView {
    #[serde(flatten)]
    pub record: LifeDateRecord,
    /// Days from today (JST); negative once the date has passed.
    pub in_days: i64,
    pub reminders: Vec<Reminder>,
    /// The reminder rung (90, 30 or 7) whose window today falls in, if any.
    pub active_window: Option<i64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    dates: Vec<LifeDateRecord>,
}

pub(crate) fn dir_in(data_dir: &Path) -> PathBuf {
    data_dir.join("life")
}

fn file(dir: &Path) -> PathBuf {
    dir.join("dates.json")
}

fn parse_day(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| "invalid input".to_string())
}

/// The 90/30/7 reminders for `date`, as of `today`.
pub(crate) fn reminders(date: NaiveDate, today: NaiveDate) -> Vec<Reminder> {
    LADDER
        .iter()
        .map(|&days| {
            let on = date - Duration::days(days);
            Reminder {
                days,
                on: on.format("%Y-%m-%d").to_string(),
                passed: on <= today,
            }
        })
        .collect()
}

/// Which window `days_until` falls in. The 7-day rung is open from 7 days out to the day itself; the 90 and
/// 30 rungs are open on the reminder day and the two days after, so a far-off date appears briefly, not all
/// quarter. `None` outside every window or once the date has passed.
pub(crate) fn active_window(days_until: i64) -> Option<i64> {
    if days_until < 0 {
        return None;
    }
    if days_until <= 7 {
        return Some(7);
    }
    [30, 90]
        .into_iter()
        .find(|&r| (r - 2..=r).contains(&days_until))
}

fn view(record: &LifeDateRecord, today: NaiveDate) -> Option<LifeDateView> {
    let date = parse_day(&record.date).ok()?;
    let in_days = (date - today).num_days();
    Some(LifeDateView {
        record: record.clone(),
        in_days,
        reminders: reminders(date, today),
        active_window: active_window(in_days),
    })
}

fn validate(d: &LifeDateDraft) -> Result<LifeDateRecord, String> {
    if d.lane != "personal" && d.lane != "residence" {
        return Err("invalid input".into());
    }
    let date = parse_day(&d.date)?;
    if !(2000..=2100).contains(&date.year()) {
        return Err("invalid input".into());
    }
    Ok(LifeDateRecord {
        id: match d.id.as_deref() {
            Some(id) => clean_slug(id, 40)?,
            None => String::new(),
        },
        title: clean_text(&d.title, 1, 120)?,
        date: date.format("%Y-%m-%d").to_string(),
        lane: d.lane.clone(),
        sub: match d.sub.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => clean_text(s, 1, 160)?,
            None => String::new(),
        },
    })
}

fn sorted(mut v: Vec<LifeDateRecord>) -> Vec<LifeDateRecord> {
    v.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));
    v
}

pub(crate) fn list_in(dir: &Path, today: NaiveDate) -> Result<Vec<LifeDateView>, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let store: Store = read_json(&file(dir), MAX_BYTES)?;
    Ok(sorted(store.dates)
        .iter()
        .filter_map(|r| view(r, today))
        .collect())
}

/// Inserts or (with an existing `id`) replaces. Returns the stored record.
pub(crate) fn upsert_in(dir: &Path, draft: &LifeDateDraft) -> Result<LifeDateRecord, String> {
    let mut rec = validate(draft)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store: Store = read_json(&file(dir), MAX_BYTES)?;
    match store
        .dates
        .iter_mut()
        .find(|r| r.id == rec.id && !rec.id.is_empty())
    {
        Some(slot) => *slot = rec.clone(),
        None => {
            if store.dates.len() >= MAX_DATES {
                return Err("The dates list is full.".into());
            }
            if rec.id.is_empty() {
                rec.id = new_id("ld");
            }
            store.dates.push(rec.clone());
        }
    }
    write_json_atomic(&file(dir), &store, MAX_BYTES)?;
    Ok(rec)
}

pub(crate) fn delete_in(dir: &Path, id: &str) -> Result<bool, String> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut store: Store = read_json(&file(dir), MAX_BYTES)?;
    let before = store.dates.len();
    store.dates.retain(|r| r.id != id);
    if store.dates.len() == before {
        return Ok(false);
    }
    write_json_atomic(&file(dir), &store, MAX_BYTES)?;
    Ok(true)
}

// ---- Reading a typed or spoken line into a date (the preview) ----

/// What reading the text gave. Amara never guesses a legal date: a missing day or year is a question.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reading {
    Ready(LifeDateDraft),
    Ask {
        question: String,
        options: Vec<String>,
        note: String,
    },
    Elsewhere {
        message: String,
    },
}

const FULL: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

fn month_of(word: &str) -> Option<u32> {
    let w: String = word
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_lowercase();
    if w.chars().count() < 3 || !w.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if w == "sept" {
        return Some(9);
    }
    FULL.iter()
        .position(|f| f.starts_with(&w))
        .map(|i| i as u32 + 1)
}

fn day_in_word(word: &str) -> Option<u32> {
    let w = word
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    let digits = w
        .strip_suffix("st")
        .or_else(|| w.strip_suffix("nd"))
        .or_else(|| w.strip_suffix("rd"))
        .or_else(|| w.strip_suffix("th"))
        .unwrap_or(&w);
    let n: u32 = digits.parse().ok()?;
    (1..=31).contains(&n).then_some(n)
}

fn year_in_word(word: &str) -> Option<i32> {
    let w = word.trim_matches(|c: char| !c.is_alphanumeric());
    let n: i32 = w.parse().ok()?;
    (2000..=2100).contains(&n).then_some(n)
}

const BUSINESS: [&str; 6] = [
    "taiyo",
    "launch",
    "venture",
    "enterprise",
    "tax filing",
    "invoice",
];
const RESIDENCE: [&str; 4] = ["visa", "residence", "immigration", "zairyu"];
const CONNECTORS: [&str; 8] = ["on", "by", "before", "until", "due", "is", "at", "for"];

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub(crate) fn read_text(text: &str, today: NaiveDate) -> Reading {
    let lower = text.to_lowercase();
    if BUSINESS.iter().any(|w| lower.contains(w)) {
        return Reading::Elsewhere {
            message:
                "Business dates live in Enterprise › Ventures. Change it there and it updates here."
                    .into(),
        };
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut used = vec![false; words.len()];
    let mut md: Option<(u32, u32)> = None;
    let mut year: Option<i32> = None;
    let mut i = 0;
    while i < words.len() {
        if md.is_none() && i + 1 < words.len() {
            if let (Some(m), Some(d)) = (month_of(words[i]), day_in_word(words[i + 1])) {
                md = Some((m, d));
                used[i] = true;
                used[i + 1] = true;
                i += 2;
                continue;
            }
            if let (Some(d), Some(m)) = (day_in_word(words[i]), month_of(words[i + 1])) {
                md = Some((m, d));
                used[i] = true;
                used[i + 1] = true;
                i += 2;
                continue;
            }
        }
        if year.is_none() {
            if let Some(y) = year_in_word(words[i]) {
                year = Some(y);
                used[i] = true;
            } else if words[i].eq_ignore_ascii_case("next")
                && words.get(i + 1).is_some_and(|w| {
                    w.trim_matches(|c: char| !c.is_alphabetic())
                        .eq_ignore_ascii_case("year")
                })
            {
                year = Some(today.year() + 1);
                used[i] = true;
                used[i + 1] = true;
                i += 1;
            }
        }
        i += 1;
    }
    let Some((month, day)) = md else {
        return Reading::Ask {
            question: "Which day is it?".into(),
            options: vec![],
            note: "Type the date, e.g. Sept 14. Amara doesn't guess legal dates.".into(),
        };
    };
    let Some(year) = year else {
        return Reading::Ask {
            question: "Which year is it?".into(),
            options: vec![
                (today.year() + 1).to_string(),
                (today.year() + 2).to_string(),
            ],
            note: "Amara doesn't guess legal dates.".into(),
        };
    };
    let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
        return Reading::Ask {
            question: "That day doesn't exist. Which day is it?".into(),
            options: vec![],
            note: "Check the month and the day.".into(),
        };
    };
    let mut kept: Vec<&str> = words
        .iter()
        .zip(&used)
        .filter(|(_, u)| !**u)
        .map(|(w, _)| *w)
        .collect();
    while kept.last().is_some_and(|w| {
        let w = w
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        w.is_empty() || CONNECTORS.contains(&w.as_str())
    }) {
        kept.pop();
    }
    let title = kept.join(" ");
    let title = title.trim_matches(|c: char| c == ',' || c == '(' || c == ')' || c.is_whitespace());
    let title = if title.is_empty() {
        "Date".to_string()
    } else {
        capital(title)
    };
    let title: String = title.chars().take(120).collect();
    let lane = if RESIDENCE.iter().any(|w| lower.contains(w)) {
        "residence"
    } else {
        "personal"
    };
    Reading::Ready(LifeDateDraft {
        id: None,
        title,
        date: date.format("%Y-%m-%d").to_string(),
        lane: lane.into(),
        sub: None,
    })
}

fn data_dir() -> PathBuf {
    dir_in(&crate::app_paths::data_dir())
}

#[tauri::command]
#[specta::specta]
pub async fn life_date_list() -> Result<Vec<LifeDateView>, String> {
    blocking(JoinErr::Plain, || list_in(&data_dir(), day_of(&now_jst()))).await
}

/// Direct save (an edit, or a caller that already holds a checked date). The add flow goes through
/// `add_preview` → `add_save`, which ends here too.
#[tauri::command]
#[specta::specta]
pub async fn life_date_upsert(draft: LifeDateDraft) -> Result<LifeDateRecord, String> {
    blocking(JoinErr::Plain, move || upsert_in(&data_dir(), &draft)).await
}

#[tauri::command]
#[specta::specta]
pub async fn life_date_delete(id: String) -> Result<bool, String> {
    blocking(JoinErr::Plain, move || delete_in(&data_dir(), &id)).await
}

/// The preview before save: the same reading `add_preview("date", …)` gives.
#[tauri::command]
pub async fn life_date_preview(text: String) -> Result<super::adds::ParseResult, String> {
    super::adds::add_preview("date".into(), text).await
}

#[cfg(test)]
mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn reminders_are_90_30_7_days_before() {
        let r = reminders(d(2027, 9, 14), d(2026, 11, 11));
        assert_eq!(r.iter().map(|x| x.days).collect::<Vec<_>>(), [90, 30, 7]);
        assert_eq!(r[0].on, "2027-06-16");
        assert_eq!(r[1].on, "2027-08-15");
        assert_eq!(r[2].on, "2027-09-07");
        assert!(r.iter().all(|x| !x.passed));
        let r = reminders(d(2026, 12, 18), d(2026, 11, 20));
        assert_eq!(
            r.iter().map(|x| x.passed).collect::<Vec<_>>(),
            [true, true, false]
        );
    }

    #[test]
    fn windows_open_on_the_reminder_day_and_close_after_the_date() {
        assert_eq!(active_window(3), Some(7));
        assert_eq!(active_window(7), Some(7));
        assert_eq!(active_window(0), Some(7));
        assert_eq!(active_window(8), None);
        assert_eq!(active_window(30), Some(30));
        assert_eq!(active_window(28), Some(30));
        assert_eq!(active_window(27), None);
        assert_eq!(active_window(90), Some(90));
        assert_eq!(active_window(91), None);
        assert_eq!(active_window(-1), None);
    }

    #[test]
    fn upsert_list_delete_round_trip_and_validation() {
        let data = temp_dir("ld");
        let dir = dir_in(&data);
        let mk = |title: &str, date: &str| LifeDateDraft {
            id: None,
            title: title.into(),
            date: date.into(),
            lane: "residence".into(),
            sub: Some("renew before".into()),
        };
        let a = upsert_in(&dir, &mk("Residence card expires", "2027-09-14")).unwrap();
        let b = upsert_in(&dir, &mk("Lease renewal", "2027-04-01")).unwrap();
        let list = list_in(&dir, d(2027, 3, 25)).unwrap();
        assert_eq!(
            list.iter().map(|v| v.record.id.clone()).collect::<Vec<_>>(),
            [b.id.clone(), a.id.clone()]
        );
        assert_eq!(list[0].in_days, 7);
        assert_eq!(list[0].active_window, Some(7));
        // an edit keeps the id
        let mut e = mk("Lease renewal (moved)", "2027-04-08");
        e.id = Some(b.id.clone());
        upsert_in(&dir, &e).unwrap();
        assert_eq!(list_in(&dir, d(2027, 3, 25)).unwrap().len(), 2);
        assert!(upsert_in(&dir, &mk("x", "2027-02-30")).is_err());
        let mut bad = mk("x", "2027-02-01");
        bad.lane = "enterprise".into();
        assert!(upsert_in(&dir, &bad).is_err());
        assert!(delete_in(&dir, &a.id).unwrap());
        assert!(!delete_in(&dir, &a.id).unwrap());
        assert_eq!(list_in(&dir, d(2027, 3, 25)).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn reading_asks_instead_of_guessing() {
        let today = d(2026, 11, 11);
        match read_text("Residence card expires Sept 14", today) {
            Reading::Ask {
                question, options, ..
            } => {
                assert_eq!(question, "Which year is it?");
                assert_eq!(options, ["2027", "2028"]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            read_text("Renew passport", today),
            Reading::Ask { .. }
        ));
        assert!(matches!(
            read_text("Thing Feb 30 2027", today),
            Reading::Ask { .. }
        ));
        assert!(matches!(
            read_text("Taiyo Skin launch Nov 21 2026", today),
            Reading::Elsewhere { .. }
        ));
    }

    #[test]
    fn reading_builds_a_date_with_a_clean_title_and_lane() {
        let today = d(2026, 11, 11);
        match read_text("Residence card expires Sept 14 2027", today) {
            Reading::Ready(r) => {
                assert_eq!(r.date, "2027-09-14");
                assert_eq!(r.lane, "residence");
                assert_eq!(r.title, "Residence card expires");
            }
            other => panic!("{other:?}"),
        }
        match read_text("14 March next year lease renewal", today) {
            Reading::Ready(r) => {
                assert_eq!(r.date, "2027-03-14");
                assert_eq!(r.lane, "personal");
                assert_eq!(r.title, "Lease renewal");
            }
            other => panic!("{other:?}"),
        }
    }
}
