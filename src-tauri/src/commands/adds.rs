//! The one write path for adds (design-1111v §5, HANDOFF-1111v §7.3): typed, chat and voice all call
//! `add_preview` → `add_save` (→ `add_undo` within 8 s) and read `add_history`.
//!
//! No parallel stores: goals and habits land in the existing `goals.json`, Enterprise tasks in OpenClaw's
//! `TASKS.md`, content ideas in `CONTENT-PIPELINE.md` (the files `read_tasks` / `read_content_pipeline`
//! already read) and dates in the life dates store. A task or idea carries its venture as a `#venture:<id>`
//! tag. Only `add_save` writes, and only from a preview that came back from `add_preview`; `payload` is
//! validated again here, the display fields are never trusted.

use super::business_ledger::read_ledger_in;
use super::goals::Goal;
use super::life_dates::{self, LifeDateDraft, Reading};
use super::local_store::{
    clean_slug, clean_text, day_of, iso, new_id, now_jst, read_json, write_json_atomic,
    write_text_atomic,
};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const UNDO_MS: i64 = 8_000;
const MAX_MD_BYTES: u64 = 1024 * 1024;
const MAX_GOALS: usize = 500;
const MAX_HISTORY: usize = 200;
const MAX_TEXT: usize = 500;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Route {
    pub id: String,
    pub view: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddPreview {
    pub title: String,
    pub fields: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// What `add_save` writes. Opaque to the UI, which hands the preview back unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ParseResult {
    Preview {
        preview: AddPreview,
    },
    Ask {
        question: String,
        options: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    Elsewhere {
        message: String,
        link: Route,
        #[serde(rename = "linkLabel")]
        link_label: String,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Saved {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub also: Option<Also>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Also {
    pub label: String,
    pub link: Route,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HistoryItem {
    pub id: String,
    pub when: String,
    pub text: String,
    pub via: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Payload {
    Goal {
        title: String,
        per_week: Option<u32>,
        unit: Option<String>,
        minutes: Option<u32>,
    },
    Date {
        title: String,
        date: String,
        lane: String,
    },
    Task {
        title: String,
        venture: Option<String>,
        due: Option<String>,
    },
    Idea {
        title: String,
        content_type: String,
        venture: Option<String>,
    },
}

/// How to take a saved add back out.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum UndoRef {
    Goal { id: String },
    Date { id: String },
    Task { line: String },
    Idea { line: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistEntry {
    id: String,
    kind: String,
    text: String,
    via: String,
    at: String,
    undo: UndoRef,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HistFile {
    #[serde(default)]
    entries: Vec<HistEntry>,
}

/// Where each store lives. Tests pass temp dirs; the commands pass the app data dir and OpenClaw's workspace.
#[derive(Debug, Clone)]
pub(crate) struct Dirs {
    pub data: PathBuf,
    pub workspace: PathBuf,
    /// Legacy ~/.openclaw/workspace: read once to seed the app-data copy, never written.
    pub legacy: PathBuf,
}

impl Dirs {
    fn real() -> Dirs {
        Dirs {
            data: crate::app_paths::data_dir(),
            // The app-data copy openclaw.rs reads first (read_tasks / read_content_pipeline).
            workspace: super::openclaw::workspace_dir_in(&crate::app_paths::data_dir()),
            legacy: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".openclaw")
                .join("workspace"),
        }
    }
    /// Current text of a workspace file. The first add seeds from the legacy file (a copy: the legacy file is
    /// only read) when the app-data copy doesn't exist yet.
    fn md_source(&self, name: &str) -> Result<String, String> {
        let own = self.workspace.join(name);
        if own.exists() {
            return read_md(&own);
        }
        read_md(&self.legacy.join(name))
    }
    fn goals(&self) -> PathBuf {
        self.data.join("goals.json")
    }
    fn history(&self) -> PathBuf {
        self.data.join("adds").join("history.json")
    }
    fn life(&self) -> PathBuf {
        life_dates::dir_in(&self.data)
    }
}

fn kind_ok(kind: &str) -> Result<(), String> {
    if matches!(kind, "goal" | "date" | "task" | "idea") {
        Ok(())
    } else {
        Err("invalid input".into())
    }
}

// ---- reading text ----

fn first_clause(text: &str) -> String {
    let lower = text.to_lowercase();
    let cut = [",", " remind me", " for ", " by "]
        .iter()
        .filter_map(|m| lower.find(m))
        .min()
        .unwrap_or(text.len());
    // `lower` can differ in byte length from `text` for exotic characters; fall back to the whole text.
    let head = text.get(..cut).unwrap_or(text);
    let t = head
        .trim()
        .trim_end_matches(|c: char| c == ':' || c == '.' || c.is_whitespace());
    let mut c = t.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn ventures(data: &Path) -> Vec<(String, String)> {
    read_ledger_in(data)
        .ok()
        .flatten()
        .map(|l| l.ventures.into_iter().map(|v| (v.id, v.name)).collect())
        .unwrap_or_default()
}

/// A venture is named in the text only if the ledger has a venture of that name. Never guessed.
fn venture_in(text: &str, known: &[(String, String)]) -> Option<(String, String)> {
    let lower = text.to_lowercase();
    known
        .iter()
        .find(|(_, name)| !name.is_empty() && lower.contains(&name.to_lowercase()))
        .cloned()
}

fn weekday_of(word: &str) -> Option<Weekday> {
    match word
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_lowercase()
        .as_str()
    {
        "mon" | "monday" => Some(Weekday::Mon),
        "tue" | "tues" | "tuesday" => Some(Weekday::Tue),
        "wed" | "wednesday" => Some(Weekday::Wed),
        "thu" | "thur" | "thurs" | "thursday" => Some(Weekday::Thu),
        "fri" | "friday" => Some(Weekday::Fri),
        "sat" | "saturday" => Some(Weekday::Sat),
        "sun" | "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

/// "by Friday" / "by tomorrow" → the next such day after today (never today for a weekday).
fn due_in(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let at = words.iter().position(|w| w.eq_ignore_ascii_case("by"))?;
    let next = words.get(at + 1)?;
    let lower = next
        .trim_matches(|c: char| !c.is_alphabetic())
        .to_lowercase();
    if lower == "today" {
        return Some(today);
    }
    if lower == "tomorrow" {
        return Some(today + Duration::days(1));
    }
    let wd = weekday_of(next)?;
    (1..=7)
        .map(|n| today + Duration::days(n))
        .find(|d| d.weekday() == wd)
}

fn label_day(d: NaiveDate) -> String {
    d.format("%a, %b %-d").to_string()
}

fn clean_title(t: &str) -> Result<String, String> {
    // '#' would start a tag in the markdown stores, a leading '[' would read as a checkbox.
    let t: String = t.chars().filter(|c| *c != '#').collect();
    let t = clean_text(&t, 1, 160)?;
    Ok(t.trim_start_matches(['[', '-', ' ']).to_string())
}

fn goal_reading(text: &str) -> Result<AddPreview, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut past_comma = false;
    let (mut minutes, mut per_week, mut unit) = (None, None, None);
    let mut keep: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let n: Option<u32> = words[i].parse().ok();
        let next = words
            .get(i + 1)
            .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase());
        match (n, next.as_deref()) {
            (Some(n), Some("min" | "mins" | "minutes")) => {
                minutes = Some(n);
                i += 2;
            }
            (Some(n), Some(u @ ("nights" | "days" | "times"))) => {
                per_week = Some(n);
                unit = Some(format!("{u} a week"));
                i += 2;
                if words
                    .get(i)
                    .is_some_and(|w| w.eq_ignore_ascii_case("a") || w.eq_ignore_ascii_case("per"))
                    && words.get(i + 1).is_some_and(|w| {
                        w.trim_matches(|c: char| !c.is_alphabetic())
                            .eq_ignore_ascii_case("week")
                    })
                {
                    i += 2;
                }
            }
            _ => {
                if !past_comma {
                    keep.push(words[i]);
                }
                past_comma |= words[i].ends_with(',');
                i += 1;
            }
        }
    }
    let title = clean_title(&first_clause(&keep.join(" ")))?;
    let target = [
        minutes.map(|m| format!("{m} min")),
        Some(match (per_week, &unit) {
            (Some(n), Some(u)) => format!("{n} {u}"),
            _ => "daily".to_string(),
        }),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let payload = Payload::Goal {
        title: title.clone(),
        per_week,
        unit,
        minutes,
    };
    Ok(AddPreview {
        title,
        fields: vec![
            ("Target".into(), target),
            (
                "Counts from".into(),
                "You, one tap a day (no data source on this Mac)".into(),
            ),
            ("Shows in".into(), "Goals · Weekly review".into()),
        ],
        note: None,
        payload: Some(serde_json::to_value(payload).map_err(|e| e.to_string())?),
    })
}

fn task_reading(text: &str, data: &Path, today: NaiveDate) -> Result<AddPreview, String> {
    let title = clean_title(&first_clause(text))?;
    let venture = venture_in(text, &ventures(data));
    let due = due_in(text, today);
    let payload = Payload::Task {
        title: title.clone(),
        venture: venture.as_ref().map(|v| v.0.clone()),
        due: due.map(|d| d.format("%Y-%m-%d").to_string()),
    };
    Ok(AddPreview {
        title,
        fields: vec![
            (
                "Venture".into(),
                venture
                    .map(|v| v.1)
                    .unwrap_or_else(|| "No venture yet".into()),
            ),
            (
                "Due".into(),
                due.map(label_day).unwrap_or_else(|| "No date".into()),
            ),
            ("Status".into(), "Backlog".into()),
        ],
        note: None,
        payload: Some(serde_json::to_value(payload).map_err(|e| e.to_string())?),
    })
}

fn idea_reading(text: &str, data: &Path) -> Result<AddPreview, String> {
    let lower = text.to_lowercase();
    let (ct, label) = if lower.contains("reel") || lower.contains("video") {
        ("video", "Short video")
    } else if lower.contains("thread") {
        ("thread", "Thread")
    } else if lower.contains("blog") {
        ("blog", "Blog post")
    } else {
        ("post", "Post")
    };
    let stripped = ["reel:", "thread:", "post:", "blog:"]
        .iter()
        .find_map(|p| lower.strip_prefix(p).map(|_| text[p.len()..].trim()))
        .unwrap_or(text);
    let title = clean_title(&first_clause(stripped))?;
    let venture = venture_in(text, &ventures(data));
    let payload = Payload::Idea {
        title: title.clone(),
        content_type: ct.into(),
        venture: venture.as_ref().map(|v| v.0.clone()),
    };
    Ok(AddPreview {
        title,
        fields: vec![
            ("Type".into(), label.into()),
            (
                "Venture".into(),
                venture
                    .map(|v| v.1)
                    .unwrap_or_else(|| "No venture yet".into()),
            ),
            ("Lands in".into(), "Idea".into()),
        ],
        note: Some("Amara drafts; she never posts for you.".into()),
        payload: Some(serde_json::to_value(payload).map_err(|e| e.to_string())?),
    })
}

fn date_reading(text: &str, today: NaiveDate) -> Result<ParseResult, String> {
    Ok(match life_dates::read_text(text, today) {
        Reading::Ask {
            question,
            options,
            note,
        } => ParseResult::Ask {
            question,
            options,
            note: Some(note),
        },
        Reading::Elsewhere { message } => ParseResult::Elsewhere {
            message,
            link: Route {
                id: "enterprise".into(),
                view: "ventures".into(),
            },
            link_label: "Open Ventures ›".into(),
        },
        Reading::Ready(d) => {
            let date = NaiveDate::parse_from_str(&d.date, "%Y-%m-%d").map_err(|e| e.to_string())?;
            let lane = if d.lane == "residence" {
                "Japan · residence"
            } else {
                "Personal"
            };
            let payload = Payload::Date {
                title: d.title.clone(),
                date: d.date.clone(),
                lane: d.lane.clone(),
            };
            ParseResult::Preview {
                preview: AddPreview {
                    title: d.title,
                    fields: vec![
                        ("Date".into(), date.format("%a, %b %-d, %Y").to_string()),
                        ("Lane".into(), lane.into()),
                        ("Reminders".into(), "90, 30 and 7 days before".into()),
                        ("From".into(), "You typed it".into()),
                    ],
                    note: Some(
                        "Reminders show in Needs you today. Nothing is sent to anyone.".into(),
                    ),
                    payload: Some(serde_json::to_value(payload).map_err(|e| e.to_string())?),
                },
            }
        }
    })
}

/// Reads the line into a preview, a question or "this lives elsewhere". Writes nothing.
pub(crate) fn preview_in(
    kind: &str,
    text: &str,
    dirs: &Dirs,
    today: NaiveDate,
) -> Result<ParseResult, String> {
    kind_ok(kind)?;
    let text = clean_text(text, 1, MAX_TEXT)?;
    let preview = match kind {
        "date" => return date_reading(&text, today),
        "goal" => goal_reading(&text)?,
        "task" => task_reading(&text, &dirs.data, today)?,
        _ => idea_reading(&text, &dirs.data)?,
    };
    Ok(ParseResult::Preview { preview })
}

// ---- writing ----

/// Inserts `line` after the last line of the first section whose header contains one of `keys`; adds the
/// section at the end when there is none.
fn insert_in_section(content: &str, keys: &[&str], new_header: &str, line: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let is_header = |l: &str| l.trim_start().starts_with('#');
    let start = lines
        .iter()
        .position(|l| is_header(l) && keys.iter().any(|k| l.to_lowercase().contains(k)));
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    match start {
        Some(s) => {
            let end = lines[s + 1..]
                .iter()
                .position(|l| is_header(l))
                .map(|p| s + 1 + p)
                .unwrap_or(lines.len());
            let mut at = end;
            while at > s + 1 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            out.insert(at, line.to_string());
        }
        None => {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(format!("## {new_header}"));
            out.push(line.to_string());
        }
    }
    out.join("\n") + "\n"
}

fn remove_line(content: &str, line: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let at = lines.iter().rposition(|l| *l == line)?;
    let mut out: Vec<&str> = lines.clone();
    out.remove(at);
    Some(out.join("\n") + "\n")
}

fn read_md(path: &Path) -> Result<String, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => Err("store: can't read the file".into()),
        Ok(m) if !m.file_type().is_file() => {
            Err("store: the file must be a regular file (no symlink)".into())
        }
        Ok(m) if m.len() > MAX_MD_BYTES => Err("store: the file is over its size limit".into()),
        Ok(_) => std::fs::read_to_string(path).map_err(|_| "store: can't read the file".into()),
    }
}

fn tag_line(prefix: &str, title: &str, tags: &[(&str, Option<String>)]) -> String {
    let mut line = format!("{prefix}{title}");
    for (k, v) in tags {
        if let Some(v) = v {
            line.push_str(&format!(" #{k}:{v}"));
        }
    }
    line
}

fn also(label: &str, id: &str, view: &str) -> Option<Also> {
    Some(Also {
        label: label.into(),
        link: Route {
            id: id.into(),
            view: view.into(),
        },
    })
}

fn apply(
    p: &Payload,
    dirs: &Dirs,
    now: DateTime<FixedOffset>,
) -> Result<(Saved, UndoRef, String), String> {
    match p {
        Payload::Goal {
            title,
            per_week,
            unit,
            minutes,
        } => {
            let title = clean_title(title)?;
            let unit_text = [unit.clone(), minutes.map(|m| format!("{m} min"))]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            let path = dirs.goals();
            let mut goals: Vec<Goal> = read_json(&path, MAX_MD_BYTES)?;
            if goals.len() >= MAX_GOALS {
                return Err("The goals list is full.".into());
            }
            let id = new_id("g");
            goals.push(Goal {
                id: id.clone(),
                title: title.clone(),
                category: "habit".into(),
                target: per_week.unwrap_or(0) as f64,
                current: 0.0,
                unit: if unit_text.is_empty() {
                    "daily".into()
                } else {
                    unit_text
                },
                deadline: None,
                created_at: iso(&now),
                updated_at: iso(&now),
            });
            write_json_atomic(&path, &goals, MAX_MD_BYTES)?;
            Ok((
                Saved {
                    id: id.clone(),
                    title: title.clone(),
                    also: also("Weekly review ›", "life", "weekly-review"),
                },
                UndoRef::Goal { id },
                format!("Added {title}"),
            ))
        }
        Payload::Date { title, date, lane } => {
            let rec = life_dates::upsert_in(
                &dirs.life(),
                &LifeDateDraft {
                    id: None,
                    title: title.clone(),
                    date: date.clone(),
                    lane: lane.clone(),
                    sub: None,
                },
            )?;
            Ok((
                Saved {
                    id: rec.id.clone(),
                    title: rec.title.clone(),
                    also: also("Needs you today ›", "terrance", "needs-you"),
                },
                UndoRef::Date { id: rec.id },
                format!("Added {} · {}", rec.title, rec.date),
            ))
        }
        Payload::Task {
            title,
            venture,
            due,
        } => {
            let title = clean_title(title)?;
            let venture = venture.as_deref().map(|v| clean_slug(v, 64)).transpose()?;
            let due = due
                .as_deref()
                .map(|d| {
                    NaiveDate::parse_from_str(d, "%Y-%m-%d")
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .map_err(|_| "invalid input".to_string())
                })
                .transpose()?;
            let line = tag_line("- [ ] ", &title, &[("venture", venture), ("due", due)]);
            let path = dirs.workspace.join("TASKS.md");
            let next = insert_in_section(
                &dirs.md_source("TASKS.md")?,
                &["backlog", "todo"],
                "Backlog",
                &line,
            );
            write_text_atomic(&path, &next, MAX_MD_BYTES)?;
            Ok((
                Saved {
                    id: new_id("task"),
                    title: title.clone(),
                    also: also("Needs you today ›", "terrance", "needs-you"),
                },
                UndoRef::Task { line },
                format!("Added {title}"),
            ))
        }
        Payload::Idea {
            title,
            content_type,
            venture,
        } => {
            let title = clean_title(title)?;
            let ct = clean_slug(content_type, 20)?;
            let venture = venture.as_deref().map(|v| clean_slug(v, 64)).transpose()?;
            let line = tag_line("- ", &title, &[("venture", venture), ("type", Some(ct))]);
            let path = dirs.workspace.join("CONTENT-PIPELINE.md");
            let next = insert_in_section(
                &dirs.md_source("CONTENT-PIPELINE.md")?,
                &["idea"],
                "Ideas",
                &line,
            );
            write_text_atomic(&path, &next, MAX_MD_BYTES)?;
            Ok((
                Saved {
                    id: new_id("idea"),
                    title: title.clone(),
                    also: None,
                },
                UndoRef::Idea { line },
                format!("Added {title}"),
            ))
        }
    }
}

fn payload_kind(p: &Payload) -> &'static str {
    match p {
        Payload::Goal { .. } => "goal",
        Payload::Date { .. } => "date",
        Payload::Task { .. } => "task",
        Payload::Idea { .. } => "idea",
    }
}

fn revert(u: &UndoRef, dirs: &Dirs) -> Result<(), String> {
    match u {
        UndoRef::Goal { id } => {
            let path = dirs.goals();
            let mut goals: Vec<Goal> = read_json(&path, MAX_MD_BYTES)?;
            goals.retain(|g| &g.id != id);
            write_json_atomic(&path, &goals, MAX_MD_BYTES)
        }
        UndoRef::Date { id } => life_dates::delete_in(&dirs.life(), id).map(|_| ()),
        UndoRef::Task { line } | UndoRef::Idea { line } => {
            let name = if matches!(u, UndoRef::Task { .. }) {
                "TASKS.md"
            } else {
                "CONTENT-PIPELINE.md"
            };
            let path = dirs.workspace.join(name);
            match remove_line(&read_md(&path)?, line) {
                Some(next) => write_text_atomic(&path, &next, MAX_MD_BYTES),
                None => Ok(()),
            }
        }
    }
}

pub(crate) fn save_in(
    kind: &str,
    preview: &AddPreview,
    via: Option<&str>,
    dirs: &Dirs,
    now: DateTime<FixedOffset>,
) -> Result<Saved, String> {
    kind_ok(kind)?;
    let via = match via.unwrap_or("typed") {
        v @ ("typed" | "voice" | "chat") => v,
        _ => return Err("invalid input".into()),
    };
    let payload: Payload = serde_json::from_value(preview.payload.clone().ok_or("invalid input")?)
        .map_err(|_| "invalid input".to_string())?;
    if payload_kind(&payload) != kind {
        return Err("invalid input".into());
    }
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (saved, undo, text) = apply(&payload, dirs, now)?;
    let mut hist: HistFile = read_json(&dirs.history(), MAX_MD_BYTES).unwrap_or_default();
    hist.entries.insert(
        0,
        HistEntry {
            id: saved.id.clone(),
            kind: kind.into(),
            text,
            via: via.into(),
            at: iso(&now),
            undo: undo.clone(),
        },
    );
    hist.entries.truncate(MAX_HISTORY);
    if let Err(e) = write_json_atomic(&dirs.history(), &hist, MAX_MD_BYTES) {
        let _ = revert(&undo, dirs);
        return Err(e);
    }
    Ok(saved)
}

/// Takes the add back out, only within 8 s of saving. Returns whether it did.
pub(crate) fn undo_in(
    kind: &str,
    id: &str,
    dirs: &Dirs,
    now: DateTime<FixedOffset>,
) -> Result<bool, String> {
    kind_ok(kind)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut hist: HistFile = read_json(&dirs.history(), MAX_MD_BYTES)?;
    let Some(pos) = hist
        .entries
        .iter()
        .position(|e| e.id == id && e.kind == kind)
    else {
        return Ok(false);
    };
    let age = DateTime::parse_from_rfc3339(&hist.entries[pos].at)
        .map(|t| now.signed_duration_since(t).num_milliseconds())
        .unwrap_or(i64::MAX);
    if !(0..=UNDO_MS).contains(&age) {
        return Ok(false);
    }
    revert(&hist.entries[pos].undo, dirs)?;
    hist.entries.remove(pos);
    write_json_atomic(&dirs.history(), &hist, MAX_MD_BYTES)?;
    Ok(true)
}

pub(crate) fn history_in(kind: &str, dirs: &Dirs) -> Result<Vec<HistoryItem>, String> {
    kind_ok(kind)?;
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let hist: HistFile = read_json(&dirs.history(), MAX_MD_BYTES)?;
    Ok(hist
        .entries
        .iter()
        .filter(|e| e.kind == kind)
        .take(50)
        .map(|e| HistoryItem {
            id: e.id.clone(),
            when: DateTime::parse_from_rfc3339(&e.at)
                .map(|t| t.format("%b %-d").to_string())
                .unwrap_or_default(),
            text: e.text.clone(),
            via: e.via.clone(),
        })
        .collect())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn add_preview(kind: String, text: String) -> Result<ParseResult, String> {
    blocking(move || preview_in(&kind, &text, &Dirs::real(), day_of(&now_jst()))).await
}

#[tauri::command]
pub async fn add_save(
    kind: String,
    preview: AddPreview,
    via: Option<String>,
) -> Result<Saved, String> {
    blocking(move || save_in(&kind, &preview, via.as_deref(), &Dirs::real(), now_jst())).await
}

#[tauri::command]
pub async fn add_undo(kind: String, id: String) -> Result<bool, String> {
    blocking(move || undo_in(&kind, &id, &Dirs::real(), now_jst())).await
}

#[tauri::command]
pub async fn add_history(kind: String) -> Result<Vec<HistoryItem>, String> {
    blocking(move || history_in(&kind, &Dirs::real())).await
}

#[cfg(test)]
mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::*;
    use chrono::Duration;

    fn dirs(tag: &str) -> Dirs {
        let root = temp_dir(tag);
        Dirs {
            data: root.join("data"),
            workspace: root.join("ws"),
            legacy: root.join("home").join(".openclaw").join("workspace"),
        }
    }
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 11).unwrap()
    }
    fn preview(kind: &str, text: &str, d: &Dirs) -> AddPreview {
        match preview_in(kind, text, d, today()).unwrap() {
            ParseResult::Preview { preview } => preview,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn preview_writes_nothing() {
        let d = dirs("addp");
        for (k, t) in [
            ("goal", "Read 20 minutes before bed, 5 nights a week"),
            ("task", "Order jars by Friday"),
            ("idea", "Reel: label approval"),
            ("date", "Residence card Sept 14 2027"),
        ] {
            preview_in(k, t, &d, today()).unwrap();
        }
        assert!(!d.data.exists() && !d.workspace.exists());
    }

    #[test]
    fn goal_reading_and_save_reuse_goals_json_and_undo_inside_8s() {
        let d = dirs("addg");
        let p = preview("goal", "Read 20 minutes before bed, 5 nights a week", &d);
        assert_eq!(p.title, "Read before bed");
        assert_eq!(p.fields[0].1, "20 min · 5 nights a week");
        let t = now_jst();
        let s = save_in("goal", &p, None, &d, t).unwrap();
        let goals: Vec<Goal> =
            serde_json::from_str(&std::fs::read_to_string(d.goals()).unwrap()).unwrap();
        assert_eq!(
            (goals.len(), goals[0].category.as_str(), goals[0].target),
            (1, "habit", 5.0)
        );
        let h = history_in("goal", &d).unwrap();
        assert_eq!((h.len(), h[0].via.as_str()), (1, "typed"));
        assert!(!undo_in("goal", &s.id, &d, t + Duration::seconds(9)).unwrap());
        assert!(undo_in("goal", &s.id, &d, t + Duration::seconds(3)).unwrap());
        let goals: Vec<Goal> =
            serde_json::from_str(&std::fs::read_to_string(d.goals()).unwrap()).unwrap();
        assert!(goals.is_empty());
        assert!(history_in("goal", &d).unwrap().is_empty());
    }

    #[test]
    fn tasks_and_ideas_land_in_the_files_the_readers_use_with_a_venture_tag() {
        let d = dirs("addt");
        std::fs::create_dir_all(d.data.join("business")).unwrap();
        std::fs::write(
            d.data.join("business").join("ledger.json"),
            r#"{"version":1,"ventures":[{"id":"taiyo-skin","name":"Taiyo Skin","health":"on_track","progress":null,"nextStep":null,"targetJpy":null,"detail":null}]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(&d.workspace).unwrap();
        std::fs::write(
            d.workspace.join("TASKS.md"),
            "# Tasks\n## Active\n- [ ] A\n\n## Backlog\n- [ ] B\n\n## Done\n- [x] C\n",
        )
        .unwrap();
        let p = preview("task", "Order 50 sample jars for Taiyo Skin by Friday", &d);
        assert_eq!(p.title, "Order 50 sample jars");
        assert_eq!(p.fields[0].1, "Taiyo Skin");
        assert_eq!(p.fields[1].1, "Fri, Nov 13");
        let t = now_jst();
        let s = save_in("task", &p, Some("voice"), &d, t).unwrap();
        let md = std::fs::read_to_string(d.workspace.join("TASKS.md")).unwrap();
        assert_eq!(md, "# Tasks\n## Active\n- [ ] A\n\n## Backlog\n- [ ] B\n- [ ] Order 50 sample jars #venture:taiyo-skin #due:2026-11-13\n\n## Done\n- [x] C\n");
        assert_eq!(history_in("task", &d).unwrap()[0].via, "voice");
        assert!(undo_in("task", &s.id, &d, t + Duration::seconds(1)).unwrap());
        assert!(!std::fs::read_to_string(d.workspace.join("TASKS.md"))
            .unwrap()
            .contains("jars"));

        let p = preview("idea", "Reel: how label approval works, for Taiyo Skin", &d);
        assert_eq!(p.fields[0].1, "Short video");
        save_in("idea", &p, None, &d, t).unwrap();
        let md = std::fs::read_to_string(d.workspace.join("CONTENT-PIPELINE.md")).unwrap();
        assert_eq!(
            md,
            "## Ideas\n- How label approval works #venture:taiyo-skin #type:video\n"
        );
        // The readers the Enterprise pages use see the venture on the item.
        let tasks = super::super::openclaw::parse_tasks_md(
            &std::fs::read_to_string(d.workspace.join("TASKS.md")).unwrap(),
        );
        let jars = tasks.iter().find(|t| t.title == "Order 50 sample jars");
        assert!(jars.is_none(), "undone task is gone");
        let items = super::super::openclaw::parse_content_pipeline(&md);
        assert_eq!(items[0].title, "How label approval works");
        assert_eq!(items[0].venture.as_deref(), Some("taiyo-skin"));
        assert_eq!(items[0].content_type, "video");
        let again = save_in(
            "task",
            &preview("task", "Order 50 sample jars for Taiyo Skin by Friday", &d),
            None,
            &d,
            t,
        )
        .unwrap();
        let tasks = super::super::openclaw::parse_tasks_md(
            &std::fs::read_to_string(d.workspace.join("TASKS.md")).unwrap(),
        );
        let jars = tasks
            .iter()
            .find(|t| t.title == "Order 50 sample jars")
            .unwrap();
        assert_eq!(
            (jars.venture.as_deref(), jars.status.as_str()),
            (Some("taiyo-skin"), "backlog")
        );
        assert!(!again.id.is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(d.workspace.join("TASKS.md"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn a_write_never_touches_the_legacy_openclaw_folder_and_the_first_add_seeds_from_it() {
        let d = dirs("addl");
        // No legacy folder: a save must not create it (nor its parent).
        save_in("task", &preview("task", "First", &d), None, &d, now_jst()).unwrap();
        assert!(!d.legacy.exists() && !d.legacy.parent().unwrap().exists());
        assert!(d.workspace.join("TASKS.md").exists());

        // A legacy file: copied on the first add, left byte-identical (same mtime) afterwards.
        let d = dirs("addl2");
        std::fs::create_dir_all(&d.legacy).unwrap();
        let legacy_md = "## Backlog\n- [ ] Old task\n";
        std::fs::write(d.legacy.join("TASKS.md"), legacy_md).unwrap();
        let before = std::fs::metadata(d.legacy.join("TASKS.md"))
            .unwrap()
            .modified()
            .unwrap();
        let t = now_jst();
        let s = save_in("task", &preview("task", "New one", &d), None, &d, t).unwrap();
        let app = std::fs::read_to_string(d.workspace.join("TASKS.md")).unwrap();
        assert_eq!(app, "## Backlog\n- [ ] Old task\n- [ ] New one\n");
        undo_in("task", &s.id, &d, t).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.workspace.join("TASKS.md")).unwrap(),
            legacy_md
        );
        assert_eq!(
            std::fs::read_to_string(d.legacy.join("TASKS.md")).unwrap(),
            legacy_md
        );
        assert_eq!(
            std::fs::metadata(d.legacy.join("TASKS.md"))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
    }

    #[test]
    fn a_date_asks_before_it_saves_and_saves_through_the_dates_store() {
        let d = dirs("addd");
        assert!(matches!(
            preview_in("date", "Residence card expires Sept 14", &d, today()).unwrap(),
            ParseResult::Ask { .. }
        ));
        let p = preview("date", "Residence card expires Sept 14 2027", &d);
        let s = save_in("date", &p, Some("chat"), &d, now_jst()).unwrap();
        let list = life_dates::list_in(&d.life(), today()).unwrap();
        assert_eq!(
            (
                list.len(),
                list[0].record.date.as_str(),
                list[0].reminders.len()
            ),
            (1, "2027-09-14", 3)
        );
        assert!(undo_in("date", &s.id, &d, now_jst()).unwrap());
        assert!(life_dates::list_in(&d.life(), today()).unwrap().is_empty());
    }

    #[test]
    fn save_trusts_only_a_valid_payload_of_the_same_kind() {
        let d = dirs("addv");
        let mut p = preview("task", "Do a thing", &d);
        assert!(save_in("goal", &p, None, &d, now_jst()).is_err());
        assert!(save_in("task", &p, Some("email"), &d, now_jst()).is_err());
        p.payload = None;
        assert!(save_in("task", &p, None, &d, now_jst()).is_err());
        p.payload =
            Some(serde_json::json!({"kind":"task","title":"x","venture":"../x","due":null}));
        assert!(save_in("task", &p, None, &d, now_jst()).is_err());
        assert!(preview_in("nope", "x", &d, today()).is_err());
        assert!(!d.workspace.exists());
    }
}
