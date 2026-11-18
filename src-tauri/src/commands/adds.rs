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
use super::shared::task::{blocking, JoinErr};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const UNDO_MS: i64 = 8_000;
pub(crate) const MAX_MD_BYTES: u64 = 1024 * 1024;
const MAX_GOALS: usize = 500;
const MAX_HISTORY: usize = 200;
const MAX_TEXT: usize = 500;

static LOCK: Mutex<()> = Mutex::new(());

/// A preview's payload lives here, not in the client: `add_preview` hands back only a random single-use token and
/// `add_save` redeems it, so the saved payload can't be swapped or replayed (2 minutes, one use).
pub const TOKEN_TTL_MS: u64 = 2 * 60_000;
const MAX_TOKENS: usize = 256;

#[derive(Default)]
pub(crate) struct Tokens(Vec<(String, String, Value, u64)>);

impl Tokens {
    pub(crate) fn issue(&mut self, kind: &str, payload: Value, now_ms: u64) -> String {
        self.0
            .retain(|t| now_ms.saturating_sub(t.3) <= TOKEN_TTL_MS);
        if self.0.len() >= MAX_TOKENS {
            self.0.remove(0);
        }
        let token = uuid::Uuid::new_v4().simple().to_string();
        self.0.push((token.clone(), kind.into(), payload, now_ms));
        token
    }
    /// Single use: the token is gone after any redeem attempt that finds it.
    pub(crate) fn redeem(
        &mut self,
        kind: &str,
        preview: &AddPreview,
        now_ms: u64,
    ) -> Result<Value, String> {
        let token = preview
            .payload
            .as_ref()
            .and_then(|p| p.get("token"))
            .and_then(Value::as_str)
            .ok_or("invalid input")?;
        let at = self
            .0
            .iter()
            .position(|t| t.0 == token)
            .ok_or("invalid input")?;
        let (_, k, payload, issued) = self.0.remove(at);
        if k != kind || now_ms.saturating_sub(issued) > TOKEN_TTL_MS {
            return Err("This preview expired. Read it again.".into());
        }
        Ok(payload)
    }
}

static TOKENS: Mutex<Tokens> = Mutex::new(Tokens(Vec::new()));

fn mono_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct Route {
    pub id: String,
    pub view: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct AddPreview {
    pub title: String,
    pub fields: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// What `add_save` writes. Opaque to the UI, which hands the preview back unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
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

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct Saved {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub also: Option<Also>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct Also {
    pub label: String,
    pub link: Route,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
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
pub(crate) enum UndoRef {
    Goal {
        id: String,
    },
    /// 1118a: a route write (step edit / complete / accepted branch). Undo puts back the roadmap exactly as it
    /// was (`prev`, None = there was none) and, for an accepted branch, re-opens that suggestion.
    /// 1118a: a declined branch; Undo re-opens it.
    Branch {
        id: String,
    },
    Route {
        goal_id: String,
        prev: Option<Value>,
        branch: Option<String>,
        /// Hash of the roadmap the write produced (all of it: steps, plan note, moved_at, version...). Undo
        /// refuses if anything, including a legacy roadmap write or the planner, changed it since.
        after_hash: String,
    },
    /// 1118a B3: a Smart home device change. `restore` is `{service, value}` that puts the device back
    /// (None = irreversible, e.g. a scene). Undo re-applies it through the same allowlist.
    Ha {
        entity_id: String,
        restore: Option<Value>,
    },
    /// 1118a B2: a recorded or dismissed watch-only proposal; Undo puts it back to `proposed`.
    Watch {
        id: String,
    },
    /// 0038: a watchlist pick removed (soft). Undo puts it back.
    WatchlistRemove {
        pick_id: String,
    },
    Date {
        id: String,
    },
    Task {
        line: String,
    },
    Idea {
        line: String,
    },
    /// 1118a B4: a content item moved between pipeline stages. Undo takes the line out of `to` and puts it
    /// back in `from`.
    Content {
        line: String,
        from: String,
        to: String,
        /// SHA-256 (hex) of the file text the move wrote. Undo refuses if the file changed since.
        after_hash: String,
    },
    /// 0037: a goal's venture link set or cleared. Undo puts back the previous link and `updated_at`, only if
    /// goals.json still hashes to what the write produced.
    GoalVenture {
        goal_id: String,
        prev: Option<String>,
        prev_updated_at: String,
        after_hash: String,
    },
    /// 0037: a saved Content draft file (`drafts/<file>`). Undo restores the previous text (`prev`, when
    /// `had_prev`) or removes the file, only if the file still hashes to what the save wrote. `prev` is dropped
    /// from History once Undo can no longer use it.
    Draft {
        file: String,
        prev: Option<String>,
        had_prev: bool,
        after_hash: String,
    },
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
    pub(crate) fn real() -> Dirs {
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
    pub(crate) fn md_source(&self, name: &str) -> Result<String, String> {
        let own = self.workspace.join(name);
        if own.exists() {
            return read_md(&own);
        }
        read_md(&self.legacy.join(name))
    }
    pub(crate) fn goals(&self) -> PathBuf {
        self.data.join("goals.json")
    }
    fn history(&self) -> PathBuf {
        self.data.join("adds").join("history.json")
    }
    fn life(&self) -> PathBuf {
        life_dates::dir_in(&self.data)
    }
}

/// Kinds the generic add_preview / add_save path takes. "ha" is not one: device writes have their own command.
fn kind_ok_generic(kind: &str) -> Result<(), String> {
    if matches!(kind, "goal" | "date" | "task" | "idea" | "route" | "watch") {
        Ok(())
    } else {
        Err("invalid input".into())
    }
}

fn kind_ok(kind: &str) -> Result<(), String> {
    if matches!(
        kind,
        "goal" | "date" | "task" | "idea" | "route" | "watch" | "ha" | "content"
    ) {
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
    kind_ok_generic(kind)?;
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
pub(crate) fn insert_in_section(
    content: &str,
    keys: &[&str],
    new_header: &str,
    line: &str,
) -> String {
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

pub(crate) fn remove_line(content: &str, line: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let at = lines.iter().rposition(|l| *l == line)?;
    let mut out: Vec<&str> = lines.clone();
    out.remove(at);
    Some(out.join("\n") + "\n")
}

pub(crate) fn read_md(path: &Path) -> Result<String, String> {
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
            if per_week.is_some_and(|n| !(1..=50).contains(&n))
                || minutes.is_some_and(|m| !(1..=1440).contains(&m))
            {
                return Err("invalid input".into());
            }
            let unit = unit.as_deref().map(|u| clean_text(u, 1, 40)).transpose()?;
            let unit_text = [unit, minutes.map(|m| format!("{m} min"))]
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
                venture_id: None,
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
        UndoRef::Content {
            line,
            from,
            to,
            after_hash,
        } => super::content_pipeline::revert_move(dirs, line, from, to, after_hash),
        UndoRef::GoalVenture {
            goal_id,
            prev,
            prev_updated_at,
            after_hash,
        } => super::goal_venture::revert(dirs, goal_id, prev, prev_updated_at, after_hash),
        UndoRef::Draft {
            file,
            prev,
            had_prev,
            after_hash,
        } => super::content_drafts::revert(dirs, file, prev.as_deref(), *had_prev, after_hash),
        UndoRef::Branch { id } => super::goal_route::reopen_in(&dirs.data, id),
        UndoRef::Watch { id } => super::watch_proposals::reopen_in(&dirs.data, id),
        UndoRef::WatchlistRemove { pick_id } => {
            super::watchlist::restore_pick_in(&dirs.data, pick_id)
        }
        UndoRef::Ha { entity_id, restore } => super::ha_devices::restore_blocking(
            entity_id,
            super::ha_devices::decode_restore(restore).as_ref(),
        ),
        UndoRef::Route {
            goal_id,
            prev,
            branch,
            after_hash,
        } => super::goal_route::restore_in(
            &dirs.data,
            goal_id,
            prev.clone(),
            branch.as_deref(),
            after_hash,
        ),
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

/// A saved draft's old text is only for Undo, which closes after the window. Drops it once expired; true if
/// anything changed.
fn trim_drafts(hist: &mut HistFile, now: DateTime<FixedOffset>) -> bool {
    let mut changed = false;
    for e in hist.entries.iter_mut() {
        if let UndoRef::Draft { prev, .. } = &mut e.undo {
            let age = DateTime::parse_from_rfc3339(&e.at)
                .map(|t| now.signed_duration_since(t).num_milliseconds())
                .unwrap_or(i64::MAX);
            if prev.is_some() && !(0..=UNDO_MS).contains(&age) {
                *prev = None;
                changed = true;
            }
        }
    }
    changed
}

/// Trims expired draft text from History under the lock, writing back only if something changed.
pub(crate) fn trim_expired_drafts_in(dirs: &Dirs, now: DateTime<FixedOffset>) {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let Ok(mut hist) = read_json::<HistFile>(&dirs.history(), MAX_MD_BYTES) else {
        return;
    };
    if trim_drafts(&mut hist, now) {
        let _ = write_json_atomic(&dirs.history(), &hist, MAX_MD_BYTES);
    }
}

/// Once at startup.
pub(crate) fn trim_expired_drafts() {
    trim_expired_drafts_in(&Dirs::real(), now_jst());
}

/// Route entries carry a whole-roadmap snapshot for Undo. Only the newest few within the Undo window keep it,
/// so route writes can't fill the file shared with every other kind. The History line stays.
pub(crate) const MAX_SNAPSHOTS: usize = 10;

fn strip_old_snapshots(hist: &mut HistFile, now: DateTime<FixedOffset>) {
    trim_drafts(hist, now);
    let mut kept = 0;
    for e in hist.entries.iter_mut() {
        if let UndoRef::Route { prev, .. } = &mut e.undo {
            if prev.is_none() {
                continue;
            }
            let age = DateTime::parse_from_rfc3339(&e.at)
                .map(|t| now.signed_duration_since(t).num_milliseconds())
                .unwrap_or(i64::MAX);
            kept += 1;
            if !(0..=UNDO_MS).contains(&age) || kept > MAX_SNAPSHOTS {
                *prev = None;
            }
        }
    }
}

/// The route writes' way onto the one write path: runs `f` (which changes the store and returns the undo
/// handle) under the history lock, then records the History entry. If History cannot be written the change is
/// reverted, exactly like `save_in`. Returns the history id (what `add_undo("route", id)` takes).
pub(crate) fn logged_write<T>(
    kind: &str,
    text: String,
    via: Option<&str>,
    dirs: &Dirs,
    now: DateTime<FixedOffset>,
    f: impl FnOnce() -> Result<(T, UndoRef), String>,
) -> Result<(String, T), String> {
    kind_ok(kind)?;
    let via = match via.unwrap_or("typed") {
        v @ ("typed" | "voice" | "chat") => v,
        _ => return Err("invalid input".into()),
    };
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    // A history file that can't be read is an error, never an empty History that gets written back over it.
    let mut hist: HistFile = read_json(&dirs.history(), MAX_MD_BYTES)?;
    let (out, undo) = f()?;
    let id = new_id(kind);
    hist.entries.insert(
        0,
        HistEntry {
            id: id.clone(),
            kind: kind.into(),
            text,
            via: via.into(),
            at: iso(&now),
            undo: undo.clone(),
        },
    );
    hist.entries.truncate(MAX_HISTORY);
    strip_old_snapshots(&mut hist, now);
    if let Err(e) = write_json_atomic(&dirs.history(), &hist, MAX_MD_BYTES) {
        let _ = revert(&undo, dirs);
        return Err(e);
    }
    Ok((id, out))
}

/// Records a change that was ALREADY applied outside the lock (a Home Assistant call must not hold the History
/// lock). Takes the lock only to write History. If the write fails nothing is reverted: the caller reports
/// "applied but not recorded".
pub(crate) fn logged_record(
    kind: &str,
    text: String,
    via: Option<&str>,
    dirs: &Dirs,
    now: DateTime<FixedOffset>,
    undo: UndoRef,
) -> Result<String, String> {
    kind_ok(kind)?;
    let via = match via.unwrap_or("typed") {
        v @ ("typed" | "voice" | "chat") => v,
        _ => return Err("invalid input".into()),
    };
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut hist: HistFile = read_json(&dirs.history(), MAX_MD_BYTES)?;
    let id = new_id(kind);
    hist.entries.insert(
        0,
        HistEntry {
            id: id.clone(),
            kind: kind.into(),
            text,
            via: via.into(),
            at: iso(&now),
            undo,
        },
    );
    hist.entries.truncate(MAX_HISTORY);
    strip_old_snapshots(&mut hist, now);
    write_json_atomic(&dirs.history(), &hist, MAX_MD_BYTES)?;
    Ok(id)
}

pub(crate) fn save_in(
    kind: &str,
    preview: &AddPreview,
    via: Option<&str>,
    dirs: &Dirs,
    now: DateTime<FixedOffset>,
) -> Result<Saved, String> {
    kind_ok_generic(kind)?;
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
    // Read first: an unreadable History is an error before anything is written, never an empty one.
    let mut hist: HistFile = read_json(&dirs.history(), MAX_MD_BYTES)?;
    let (saved, undo, text) = apply(&payload, dirs, now)?;
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
    strip_old_snapshots(&mut hist, now);
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
    let mut hist: HistFile = read_json(&dirs.history(), MAX_MD_BYTES)?;
    if trim_drafts(&mut hist, now_jst()) {
        let _ = write_json_atomic(&dirs.history(), &hist, MAX_MD_BYTES);
    }
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

/// Reads the line and, for a preview, parks its payload behind a single-use token (the caller sees the token only).
/// Shared by the `add_preview` command and the Rust turn pipeline: one issuing path.
pub(crate) fn issue_preview(kind: &str, text: &str) -> Result<ParseResult, String> {
    let mut r = preview_in(kind, text, &Dirs::real(), day_of(&now_jst()))?;
    if let ParseResult::Preview { preview } = &mut r {
        if let Some(payload) = preview.payload.take() {
            let token =
                TOKENS
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .issue(kind, payload, mono_ms());
            preview.payload = Some(serde_json::json!({ "token": token }));
        }
    }
    Ok(r)
}

/// Redeems the preview's token (single use, 2 minutes, kind-bound) and writes. Shared by `add_save` and the turn
/// pipeline's confirm turn: no other path reaches `save_in` with a client-supplied preview.
pub(crate) fn redeem_and_save(
    kind: &str,
    preview: &AddPreview,
    via: Option<&str>,
) -> Result<Saved, String> {
    let payload =
        TOKENS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .redeem(kind, preview, mono_ms())?;
    let trusted = AddPreview {
        payload: Some(payload),
        ..preview.clone()
    };
    save_in(kind, &trusted, via, &Dirs::real(), now_jst())
}

#[tauri::command]
#[specta::specta]
pub async fn add_preview(kind: String, text: String) -> Result<ParseResult, String> {
    blocking(JoinErr::Plain, move || issue_preview(&kind, &text)).await
}

#[tauri::command]
#[specta::specta]
pub async fn add_save(
    kind: String,
    preview: AddPreview,
    via: Option<String>,
) -> Result<Saved, String> {
    blocking(JoinErr::Plain, move || {
        redeem_and_save(&kind, &preview, via.as_deref())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn add_undo(kind: String, id: String) -> Result<bool, String> {
    blocking(JoinErr::Plain, move || {
        undo_in(&kind, &id, &Dirs::real(), now_jst())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn add_history(kind: String) -> Result<Vec<HistoryItem>, String> {
    blocking(JoinErr::Plain, move || history_in(&kind, &Dirs::real())).await
}

// ---- chat / voice intent (hand port of src-console/adds/intent.ts; there is no regex crate) ----

/// JS `\s` (and `String.prototype.trim`): not Rust's `char::is_whitespace` (U+0085 differs, U+FEFF is missing).
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(js_space)
}

/// One element of an intent pattern. Matching is case-insensitive (ASCII) and backtracks like the TS regexes.
#[derive(Clone, Copy)]
enum St {
    /// `(?:(?:hey |ok |okay )?amara[,:]?\s+)?`
    Lead,
    /// `(?:please\s+)?`
    Please,
    /// `(?:yes[, ]+)?`
    Yes,
    /// One of the literals, required.
    Lit(&'static [&'static str]),
    /// One of the literals, or nothing.
    Opt(&'static [&'static str]),
    /// `[\s,:]+(\S.{2,})$` and capture the group.
    Body,
    /// `$`
    End,
}

fn space_run(s: &str, pos: usize) -> Option<usize> {
    let n: usize = s[pos..]
        .chars()
        .take_while(|c| js_space(*c))
        .map(char::len_utf8)
        .sum();
    (n > 0).then_some(pos + n)
}

/// Returns the capture range (Body) or (0, 0); None when the steps cannot all match.
fn run(steps: &[St], orig: &str, low: &str, pos: usize) -> Option<(usize, usize)> {
    let Some((step, rest)) = steps.split_first() else {
        return Some((0, 0));
    };
    let tail = &low[pos..];
    let mut next: Vec<usize> = Vec::new();
    match step {
        St::Lead => {
            let mut starts = Vec::new();
            for p in ["hey ", "ok ", "okay "] {
                if tail.starts_with(p) {
                    starts.push(pos + p.len());
                }
            }
            starts.push(pos);
            for a in starts {
                if low[a..].starts_with("amara") {
                    let q = a + 5;
                    let mut ends = Vec::new();
                    if low[q..].starts_with([',', ':']) {
                        ends.push(q + 1);
                    }
                    ends.push(q);
                    for e in ends {
                        if let Some(r) = space_run(low, e) {
                            next.push(r);
                        }
                    }
                }
            }
            next.push(pos);
        }
        St::Please => {
            if tail.starts_with("please") {
                if let Some(r) = space_run(low, pos + 6) {
                    next.push(r);
                }
            }
            next.push(pos);
        }
        St::Yes => {
            if tail.starts_with("yes") {
                let n: usize = low[pos + 3..]
                    .chars()
                    .take_while(|c| *c == ',' || *c == ' ')
                    .count();
                if n > 0 {
                    // ASCII only, one byte each
                    next.push(pos + 3 + n);
                }
            }
            next.push(pos);
        }
        St::Lit(alts) => {
            for a in alts.iter() {
                if tail.starts_with(a) {
                    next.push(pos + a.len());
                }
            }
        }
        St::Opt(alts) => {
            for a in alts.iter() {
                if tail.starts_with(a) {
                    next.push(pos + a.len());
                }
            }
            next.push(pos);
        }
        St::End => return (pos == low.len()).then_some((0, 0)),
        St::Body => {
            // Greedy separator run, backing off one char at a time like the regex does.
            let mut cuts = Vec::new();
            let mut at = pos;
            for c in orig[pos..].chars() {
                if !(js_space(c) || c == ',' || c == ':') {
                    break;
                }
                at += c.len_utf8();
                cuts.push(at);
            }
            for &p in cuts.iter().rev() {
                let mut it = orig[p..].chars();
                let Some(first) = it.next() else { continue };
                if js_space(first) || it.count() < 2 {
                    continue;
                }
                return Some((p, orig.len()));
            }
            return None;
        }
    }
    for p in next {
        if let Some(r) = run(rest, orig, low, p) {
            return Some(r);
        }
    }
    None
}

fn matches_steps(steps: &[St], s: &str) -> Option<(usize, usize)> {
    let low = s.to_ascii_lowercase(); // same byte offsets as `s`
    run(steps, s, &low, 0)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AddIntent {
    pub kind: &'static str,
    pub text: String,
}

const RULES: &[(&str, &[St])] = &[
    (
        "goal",
        &[
            St::Lead,
            St::Please,
            St::Lit(&["add "]),
            St::Opt(&["a ", "the "]),
            St::Opt(&["new "]),
            St::Lit(&["goal", "habit"]),
            St::Opt(&[" to", " that", " of", " for"]),
            St::Body,
        ],
    ),
    (
        "date",
        &[
            St::Lead,
            St::Please,
            St::Lit(&["remind me on", "remind me by"]),
            St::Body,
        ],
    ),
    (
        "date",
        &[
            St::Lead,
            St::Please,
            St::Lit(&["add "]),
            St::Opt(&["a ", "the "]),
            St::Lit(&["date", "deadline"]),
            St::Opt(&[" for"]),
            St::Body,
        ],
    ),
    (
        "task",
        &[
            St::Lead,
            St::Please,
            St::Lit(&["add "]),
            St::Opt(&["a ", "the "]),
            St::Opt(&["enterprise "]),
            St::Lit(&["task"]),
            St::Opt(&[" to", " for"]),
            St::Body,
        ],
    ),
    (
        "idea",
        &[
            St::Lead,
            St::Please,
            St::Lit(&["add "]),
            St::Opt(&["a ", "the "]),
            St::Lit(&["content idea"]),
            St::Opt(&[" for"]),
            St::Body,
        ],
    ),
    (
        "idea",
        &[
            St::Lead,
            St::Please,
            St::Lit(&["content idea"]),
            St::Opt(&[" for"]),
            St::Body,
        ],
    ),
];

/// Explicit phrasings only (same as `detectAddIntent`): anything else is None and nothing happens.
pub(crate) fn detect_add_intent(input: &str) -> Option<AddIntent> {
    let t = js_trim(input)
        .split(js_space)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if t.encode_utf16().count() > 500 {
        return None;
    }
    for (kind, steps) in RULES {
        if let Some((a, b)) = matches_steps(steps, &t) {
            let body = t[a..b].trim_end_matches(['.', '!', '?']);
            return Some(AddIntent {
                kind,
                text: js_trim(body).to_string(),
            });
        }
    }
    None
}

const CONFIRM: &[St] = &[
    St::Lead,
    St::Please,
    St::Yes,
    St::Lit(&["confirm", "save it", "confirm save", "save"]),
    St::Opt(&[" it", " that", " this"]),
    St::Opt(&[".", "!"]),
    St::End,
];
const CANCEL: &[St] = &[
    St::Lead,
    St::Please,
    St::Lit(&[
        "cancel",
        "nevermind",
        "never mind",
        "discard",
        "don't save it",
        "don't save",
        "dont save it",
        "dont save",
    ]),
    St::Opt(&[".", "!"]),
    St::End,
];

pub(crate) fn is_add_confirm(s: &str) -> bool {
    matches_steps(CONFIRM, js_trim(s)).is_some()
}
pub(crate) fn is_add_cancel(s: &str) -> bool {
    matches_steps(CANCEL, js_trim(s)).is_some()
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
    fn files_under(root: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(root) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    files_under(&p, out);
                } else {
                    out.push(p);
                }
            }
        }
    }

    /// Chat and voice are the same save path as typing (`via` is only a label); a voice add arrives as transcript text
    /// and the app path writes no audio file: only the JSON/markdown stores exist afterwards.
    #[test]
    fn chat_and_voice_adds_use_the_one_save_path_and_write_no_audio() {
        let d = dirs("addc");
        let t = now_jst();
        for (kind, text, via) in [
            ("goal", "Meditate 10 minutes", "chat"),
            ("task", "Order jars", "voice"),
            ("idea", "Reel: label approval", "voice"),
            ("date", "Residence card Sept 14 2027", "chat"),
        ] {
            let p = preview(kind, text, &d);
            save_in(kind, &p, Some(via), &d, t).unwrap();
            assert_eq!(history_in(kind, &d).unwrap()[0].via, via);
        }
        let mut files = Vec::new();
        files_under(&d.data, &mut files);
        files_under(&d.workspace, &mut files);
        assert!(!files.is_empty());
        for f in &files {
            let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
            assert!(
                ["json", "md", "tmp"].contains(&ext),
                "unexpected file {f:?}"
            );
        }
        // The production code (above the tests) has no audio capture or audio-file API.
        let src = include_str!("adds.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        for needle in ["cpal", "hound", "wav", "ffmpeg", "AudioContext", "arecord"] {
            assert!(!prod.contains(needle), "{needle}");
        }
    }

    fn pv(payload: Value) -> AddPreview {
        AddPreview {
            title: "t".into(),
            fields: vec![],
            note: None,
            payload: Some(payload),
        }
    }

    #[test]
    fn tokens_refuse_unknown_expired_reused_and_swapped_payloads() {
        let mut t = Tokens::default();
        let real = serde_json::json!({"kind":"task","title":"x","venture":null,"due":null});
        let tok = t.issue("task", real.clone(), 1_000);
        // unknown token, or a raw payload with no token (the old swap attack), is refused
        assert!(t
            .redeem("task", &pv(serde_json::json!({"token":"nope"})), 1_001)
            .is_err());
        assert!(t
            .redeem(
                "task",
                &pv(serde_json::json!({"kind":"task","title":"evil"})),
                1_001
            )
            .is_err());
        // a different kind can't redeem it (and it is spent by the attempt)
        let tok2 = t.issue("task", real.clone(), 1_000);
        assert!(t
            .redeem("goal", &pv(serde_json::json!({ "token": tok2 })), 1_001)
            .is_err());
        assert!(t
            .redeem("task", &pv(serde_json::json!({ "token": tok2 })), 1_001)
            .is_err());
        // the saved payload is the stored one, whatever else the client sent
        let got = t
            .redeem(
                "task",
                &pv(serde_json::json!({ "token": tok, "title": "evil" })),
                1_001,
            )
            .unwrap();
        assert_eq!(got, real);
        // reuse is refused
        assert!(t
            .redeem("task", &pv(serde_json::json!({ "token": tok })), 1_002)
            .is_err());
        // expiry
        let old = t.issue("task", real, 0);
        assert!(t
            .redeem(
                "task",
                &pv(serde_json::json!({ "token": old })),
                TOKEN_TTL_MS + 1
            )
            .is_err());
    }

    #[test]
    fn goal_numbers_and_unit_are_bounded_and_cleaned() {
        let d = dirs("addg");
        let bad = |per: Option<u32>, min: Option<u32>, unit: &str| {
            let mut p = preview("goal", "Read", &d);
            p.payload = Some(
                serde_json::json!({"kind":"goal","title":"Read","per_week":per,"unit":unit,"minutes":min}),
            );
            save_in("goal", &p, None, &d, now_jst())
        };
        assert!(bad(Some(0), None, "days a week").is_err());
        assert!(bad(Some(9999), None, "days a week").is_err());
        assert!(bad(None, Some(100000), "x").is_err());
        assert!(bad(Some(3), Some(10), &"u".repeat(200)).is_err());
        assert!(bad(Some(3), Some(10), "nights a week").is_ok());
        let goals: Vec<Goal> =
            serde_json::from_str(&std::fs::read_to_string(d.goals()).unwrap()).unwrap();
        assert_eq!(goals.len(), 1, "only the valid goal was written");
        assert!(bad(Some(3), None, "days\n#x a week").is_ok());
        let goals: Vec<Goal> =
            serde_json::from_str(&std::fs::read_to_string(d.goals()).unwrap()).unwrap();
        assert_eq!(
            goals[1].unit, "days #x a week",
            "control characters are flattened"
        );
    }

    #[test]
    fn intents_match_the_shared_ts_fixture() {
        let f: Value =
            serde_json::from_str(include_str!("../../../test/fixtures/add-intents.json")).unwrap();
        let intents = f["intents"].as_array().unwrap();
        assert!(intents.len() >= 50);
        for c in intents {
            let text = c["text"].as_str().unwrap();
            let got = detect_add_intent(text);
            match c["intent"].as_object() {
                None => assert_eq!(got, None, "{text:?}"),
                Some(o) => {
                    let got = got.unwrap_or_else(|| panic!("expected an intent: {text:?}"));
                    assert_eq!(got.kind, o["kind"].as_str().unwrap(), "{text:?}");
                    assert_eq!(got.text, o["text"].as_str().unwrap(), "{text:?}");
                }
            }
        }
        for c in f["turns"].as_array().unwrap() {
            let text = c["text"].as_str().unwrap();
            assert_eq!(
                is_add_confirm(text),
                c["confirm"].as_bool().unwrap(),
                "{text:?}"
            );
            assert_eq!(
                is_add_cancel(text),
                c["cancel"].as_bool().unwrap(),
                "{text:?}"
            );
        }
    }
}
