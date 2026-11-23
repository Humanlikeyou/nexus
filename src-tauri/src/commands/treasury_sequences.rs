//! Treasury, phase 4: email sequence drafts, per-sequence Gmail checklists and a provider-results import.
//!
//! Decision 0032: Gmail is the provider. Gmail has no timed sequences or automatic unsubscribe handling,
//! so a draft is paste-ready text plus a setup checklist for a manual or scheduled-send Gmail workflow.
//! Amara never sends email and holds no contact data.
//!
//! Rules this file keeps (each has a test):
//! - The writer is `llm_router::local_complete` only (the light local tier; a reply from the Claude tier
//!   is refused there). The prompt carries the sequence kind, the email count and the venture's name
//!   from the hand-authored ledger, and nothing else: no contact data of any kind.
//! - Every email body must contain both placeholders, `{{unsubscribe_link}}` and `{{mailing_address}}`
//!   (present anywhere in the body; position is not checked). A draft or edit missing either is rejected
//!   and nothing is stored. Copy is also refused if it has control or hidden Unicode characters, `<`/`>`,
//!   `javascript:`, URLs, an `@`, a phone-like digit run or a `{{...}}` other than `{{first_name}}`.
//! - Approval of a template is one native dialog (`approval_gate::require_approval`), stored as
//!   `approved_at`. Any edit makes the draft `draft` again, and an edit that lands while the dialog is
//!   open is refused (`changed:`).
//! - The checklist is built in code, never by the model, and always says: only contacts with recorded
//!   consent, honour opt-outs at once, and the Japan opt-in / GDPR note (not legal advice).
//! - Provider results (sends, bounces, unsubscribes, purchases after the sequence) are counts per
//!   sequence per day from a CSV; only the mapped columns are read, so recipient columns stay unread. A
//!   metric with no mapped column is `null`, never 0.
//! - [GATE] No Gmail API: `ProviderResultsSource` is an interface with a fixture-backed test only.
//!
//! Outside calls from this file: none (the local writer is the existing loopback router client).

use super::approval_gate;
use super::business_ledger::read_ledger_in;
use super::local_store::new_id;
use super::shared::task::{blocking, JoinErr};
use super::treasury::{
    is_date, now_iso, open_db, parse_csv, parse_date, pick_csv_titled, read_import_file, store_err,
    DateOrder, Slot, MAX_CELLS,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

pub const UNSUBSCRIBE_PLACEHOLDER: &str = "{{unsubscribe_link}}";
pub const ADDRESS_PLACEHOLDER: &str = "{{mailing_address}}";
const MAX_SUBJECT_CHARS: usize = 150;
const MAX_BODY_CHARS: usize = 4_000;
const MAX_METRIC: i64 = 1_000_000_000;

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasurySequenceEmail", rename_all = "camelCase")]
pub struct SeqEmail {
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename = "TreasurySequenceEmailInput", rename_all = "camelCase")]
pub struct SeqEmailInput {
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasurySequenceDraft", rename_all = "camelCase")]
pub struct SeqDraft {
    pub id: String,
    pub venture_id: String,
    pub kind: String,
    pub emails: Vec<SeqEmail>,
    pub checklist: Vec<String>,
    /// `draft | approved`.
    pub status: String,
    pub approved_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename = "TreasuryResultsMapping", rename_all = "camelCase")]
pub struct ResultsMapping {
    pub date_column: String,
    pub sends_column: Option<String>,
    pub bounces_column: Option<String>,
    pub unsubscribes_column: Option<String>,
    /// Purchases made after the sequence (a count, never who).
    pub purchases_column: Option<String>,
    /// `ymd` (default), `mdy` or `dmy`.
    pub date_format: Option<String>,
}

/// Counts for one sequence on one day. `None` = not reported (never 0).
#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasurySequenceResultRow", rename_all = "camelCase")]
pub struct ResultsDay {
    pub sequence_id: String,
    pub day: String,
    pub sends: Option<i64>,
    pub bounces: Option<i64>,
    pub unsubscribes: Option<i64>,
    pub purchases_after: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename = "TreasuryResultsImportResult", rename_all = "camelCase")]
pub struct ResultsImportResult {
    /// Days written (new or changed).
    pub added: i64,
    /// Days whose counts were already stored unchanged.
    pub skipped: i64,
    pub failed: i64,
    pub import_id: String,
}

/// [GATE] A provider API sync is not built. This is the seam a later, separately approved sync (or a
/// sequence tool behind the same results import) would fill; only a fixture implements it, in tests.
pub trait ProviderResultsSource {
    fn fetch_days(
        &self,
        sequence_id: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<ResultsDay>, String>;
}

// ───────────────────────────── drafts ─────────────────────────────

fn email_count(kind: &str) -> Option<usize> {
    match kind {
        "welcome" => Some(3),
        "post_purchase" | "reorder" | "win_back" => Some(1),
        _ => None,
    }
}

/// The brief for each email, so the model writes the right message and nothing else.
fn briefs(kind: &str) -> &'static [&'static str] {
    match kind {
        "welcome" => &[
            "Email 1, sent right after sign-up: thank them, say what to expect and how often you write.",
            "Email 2, two days later: tell the brand story and what makes the product different.",
            "Email 3, five days later: a gentle invitation to a first purchase, no invented discounts.",
        ],
        "post_purchase" => &[
            "One email three days after a first purchase: thank them, how to get the most from it, invite a reply with questions.",
        ],
        "reorder" => &[
            "One email around the time the product usually runs out: a short reminder that reordering is easy, no invented discounts.",
        ],
        _ => &[
            "One email to a customer who has gone quiet: warm, short, no guilt, no invented discounts, an easy way to say yes or to opt out.",
        ],
    }
}

/// The system addendum and the user message for the local writer. Neither holds contact data.
pub(crate) fn writer_prompt(kind: &str, venture_name: &str) -> Option<(String, String)> {
    let n = email_count(kind)?;
    let instruction = format!(
        "You draft plain-text marketing emails for a small brand. Write exactly {n} email(s). Format each as a line \
'=== EMAIL <number> ===', then a line 'Subject: <subject>', then the body. Use the literal token {{{{first_name}}}} \
for the reader's name. End every body with a footer that contains the two literal tokens {UNSUBSCRIBE_PLACEHOLDER} \
and {ADDRESS_PLACEHOLDER} exactly as written. Do not invent discounts, prices, links, names or email addresses. \
No markdown, no commentary outside the emails."
    );
    let venture_name = venture_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let venture_name: String = venture_name.chars().take(60).collect();
    let mut user = format!(
        "Brand: {}\nSequence: {kind}, {n} email(s).\n",
        venture_name.trim()
    );
    for b in briefs(kind) {
        user.push_str(b);
        user.push('\n');
    }
    Some((instruction, user))
}

const ALLOWED_PLACEHOLDERS: [&str; 3] = [
    "{{first_name}}",
    UNSUBSCRIBE_PLACEHOLDER,
    ADDRESS_PLACEHOLDER,
];
const MAX_EMAILS: usize = 3;
const MAX_DRAFTS: i64 = 200;

/// Unicode format characters (bidi controls, zero-width, joiners, BOM, soft hyphen) and line/paragraph separators.
fn is_hidden(c: char) -> bool {
    matches!(c,
        '\u{00ad}' | '\u{061c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}')
}

/// A run of seven or more digits, allowing `space - ( ) + .` between them, looks like a phone number.
fn has_phone_like_run(s: &str) -> bool {
    let (mut digits, mut gap) = (0usize, 0usize);
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits += 1;
            gap = 0;
        } else if matches!(c, ' ' | '-' | '(' | ')' | '+' | '.') && digits > 0 && gap < 2 {
            gap += 1;
        } else {
            digits = 0;
            gap = 0;
        }
        if digits >= 7 {
            return true;
        }
    }
    false
}

/// Text rules shared by subject and body. `multiline`: the body may hold `\n` and `\t`.
fn clean_copy(text: &str, multiline: bool) -> Result<(), String> {
    let bad = || "bad_draft".to_string();
    for c in text.chars() {
        let ok_control = multiline && (c == '\n' || c == '\t');
        if (c.is_control() && !ok_control) || is_hidden(c) || c == '<' || c == '>' {
            return Err(bad());
        }
    }
    let low = text.to_lowercase();
    if low.contains("javascript:")
        || low.contains(concat!("http", "://"))
        || low.contains(concat!("https", "://"))
        || low.contains("www.")
    {
        return Err(bad());
    }
    // A raw email address, or a phone-like digit run, is contact data and has no place in a template.
    if text.contains('@') || has_phone_like_run(text) {
        return Err(bad());
    }
    // Only the three known placeholders; any other `{{...}}` (or a stray brace pair) is refused.
    let mut rest = text;
    while let Some(a) = rest.find("{{") {
        let after = &rest[a..];
        let Some(b) = after.find("}}") else {
            return Err(bad());
        };
        if !ALLOWED_PLACEHOLDERS.contains(&&after[..b + 2]) {
            return Err(bad());
        }
        rest = &after[b + 2..];
    }
    if rest.contains("}}") {
        return Err(bad());
    }
    Ok(())
}

fn check_email(subject: &str, body: &str) -> Result<SeqEmail, String> {
    let subject = subject.trim();
    let body = body.trim();
    if subject.is_empty()
        || subject.chars().count() > MAX_SUBJECT_CHARS
        || body.is_empty()
        || body.chars().count() > MAX_BODY_CHARS
    {
        return Err("bad_draft".into());
    }
    clean_copy(subject, false)?;
    clean_copy(body, true)?;
    // Both placeholders must be present somewhere in the body. The check does not say where, so the
    // footer position is up to the reader of the draft.
    if !body.contains(UNSUBSCRIBE_PLACEHOLDER) || !body.contains(ADDRESS_PLACEHOLDER) {
        return Err("missing_placeholder".into());
    }
    Ok(SeqEmail {
        subject: subject.to_string(),
        body: body.to_string(),
    })
}

fn check_emails(kind: &str, emails: &[(String, String)]) -> Result<Vec<SeqEmail>, String> {
    let want = email_count(kind).ok_or_else(|| "invalid_input".to_string())?;
    if emails.len() != want {
        return Err("bad_draft".into());
    }
    emails.iter().map(|(s, b)| check_email(s, b)).collect()
}

/// Splits the writer's reply into emails. Anything that doesn't parse is `bad_draft`.
fn parse_writer_reply(kind: &str, reply: &str) -> Result<Vec<SeqEmail>, String> {
    let mut sections: Vec<Vec<&str>> = Vec::new();
    for line in reply.lines() {
        if line.trim_start().starts_with("=== EMAIL") {
            sections.push(Vec::new());
        } else if let Some(cur) = sections.last_mut() {
            cur.push(line);
        }
    }
    let mut pairs = Vec::new();
    for sec in sections {
        let mut it = sec.into_iter().skip_while(|l| l.trim().is_empty());
        let first = it.next().ok_or_else(|| "bad_draft".to_string())?;
        let subject = first
            .trim()
            .strip_prefix("Subject:")
            .or_else(|| first.trim().strip_prefix("subject:"))
            .ok_or_else(|| "bad_draft".to_string())?;
        let body = it.collect::<Vec<_>>().join("\n");
        pairs.push((subject.to_string(), body));
    }
    check_emails(kind, &pairs)
}

const CONSENT_LINE: &str = "Send only to contacts with recorded consent. Honour opt-outs at once: take the contact off every list and cancel any scheduled message the same day.";
const LEGAL_LINE: &str = "Japan requires opt-in (prior consent) for marketing email, and contacts in the EU bring GDPR. This is not legal advice: finish the consent check before the sequence goes live.";

/// The per-sequence Gmail setup checklist. Built here, never by the model.
pub(crate) fn checklist_for(kind: &str) -> Vec<String> {
    let mut v: Vec<String> = vec![CONSENT_LINE.into()];
    let specific: &[&str] = match kind {
        "welcome" => &[
            "Trigger: a new contact who has given recorded consent (stage: lead).",
            "Delays: email 1 on sign-up day, email 2 two days later, email 3 five days after sign-up.",
            "Stage changes: stays lead until their first purchase, then set stage to first_purchase and stop the sequence.",
            "Tags: add welcome-1, welcome-2, welcome-3 as each email goes out.",
        ],
        "post_purchase" => &[
            "Trigger: a contact's first purchase.",
            "Delay: three days after the purchase.",
            "Stage changes: set stage to first_purchase when the purchase is recorded; set repeat after a second purchase.",
            "Tags: add post-purchase when the email goes out.",
        ],
        "reorder" => &[
            "Trigger: a customer who bought and has not bought again.",
            "Delay: about as long as the product usually lasts (set this to your real reuse time).",
            "Stage changes: none; set repeat when they buy again.",
            "Tags: add reorder-reminder when the email goes out.",
        ],
        _ => &[
            "Trigger: a customer with no purchase and no reply for a long time (choose the number of days that fits your product).",
            "Delay: send once; do not repeat.",
            "Stage changes: set stage to lapsed when the email goes out; back to repeat if they buy again.",
            "Tags: add win-back; add no-more-emails for anyone who opts out and never write to them again.",
        ],
    };
    v.extend(specific.iter().map(|s| s.to_string()));
    v.push("Gmail has no timed sequences: save each email as a Gmail draft and use Schedule send for the delay above, or send it by hand on the day.".into());
    v.push(format!(
        "In every email replace {UNSUBSCRIBE_PLACEHOLDER} with a working unsubscribe link or a plain 'reply STOP' line you honour by hand, and {ADDRESS_PLACEHOLDER} with your real physical mailing address."
    ));
    v.push("Amara does not send email and holds no contact names or addresses. Bring counts back with the customer counts import and the results import.".into());
    v.push(LEGAL_LINE.into());
    v
}

fn content_hash(emails: &[SeqEmail]) -> Result<String, String> {
    let json = serde_json::to_string(emails).map_err(store_err)?;
    Ok(hex::encode(Sha256::digest(json.as_bytes())))
}

fn row_to_draft(r: &rusqlite::Row<'_>) -> rusqlite::Result<(SeqDraft, String)> {
    let emails_json: String = r.get(3)?;
    let checklist_json: String = r.get(4)?;
    let emails: Vec<SeqEmail> = serde_json::from_str::<Vec<EmailRow>>(&emails_json)
        .map(|v| {
            v.into_iter()
                .map(|e| SeqEmail {
                    subject: e.subject,
                    body: e.body,
                })
                .collect()
        })
        .unwrap_or_default();
    let checklist: Vec<String> = serde_json::from_str(&checklist_json).unwrap_or_default();
    Ok((
        SeqDraft {
            id: r.get(0)?,
            venture_id: r.get(1)?,
            kind: r.get(2)?,
            emails,
            checklist,
            status: r.get(5)?,
            approved_at: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        },
        r.get(9)?,
    ))
}

#[derive(Deserialize)]
struct EmailRow {
    subject: String,
    body: String,
}

const DRAFT_COLS: &str = "id, venture_id, kind, emails_json, checklist_json, status, approved_at, created_at, updated_at, content_hash";

fn get_draft(conn: &Connection, id: &str) -> Result<Option<(SeqDraft, String)>, String> {
    conn.query_row(
        &format!("SELECT {DRAFT_COLS} FROM sequence_drafts WHERE id = ?1"),
        params![id],
        row_to_draft,
    )
    .optional()
    .map_err(store_err)
}

fn ensure_room(conn: &Connection) -> Result<(), String> {
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM sequence_drafts", [], |r| r.get(0))
        .map_err(store_err)?;
    if n >= MAX_DRAFTS {
        return Err("too_many_drafts".into());
    }
    Ok(())
}

/// Checks the kind and that the venture exists in the ledger; returns the venture's name.
pub(crate) fn venture_name_for_draft(
    data: &Path,
    kind: &str,
    venture_id: &str,
) -> Result<String, String> {
    if email_count(kind).is_none() {
        return Err("invalid_input".into());
    }
    let venture_id = venture_id.trim();
    if venture_id.is_empty() || venture_id.chars().count() > 64 {
        return Err("invalid_input".into());
    }
    let name = match read_ledger_in(data) {
        Err(_) => return Err("ledger_invalid".into()),
        Ok(None) => return Err("ledger_missing".into()),
        Ok(Some(l)) => l
            .ventures
            .into_iter()
            .find(|v| v.id == venture_id)
            .map(|v| v.name)
            .ok_or_else(|| "unknown_venture".to_string())?,
    };
    ensure_room(&open_db(data)?)?;
    Ok(name)
}

/// Validates the writer's reply and stores it as a `draft`. A reply missing a placeholder stores nothing.
pub(crate) fn draft_store_in(
    data: &Path,
    kind: &str,
    venture_id: &str,
    writer_reply: &str,
) -> Result<SeqDraft, String> {
    venture_name_for_draft(data, kind, venture_id)?;
    let emails = parse_writer_reply(kind, writer_reply)?;
    let conn = open_db(data)?;
    ensure_room(&conn)?;
    let id = new_id("seq");
    let now = now_iso();
    conn.execute(
        "INSERT INTO sequence_drafts(id, venture_id, kind, emails_json, checklist_json, status, content_hash, approved_at, created_at, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, 'draft', ?6, NULL, ?7, ?7)",
        params![
            id,
            venture_id.trim(),
            kind,
            serde_json::to_string(&emails).map_err(store_err)?,
            serde_json::to_string(&checklist_for(kind)).map_err(store_err)?,
            content_hash(&emails)?,
            now
        ],
    )
    .map_err(store_err)?;
    Ok(get_draft(&conn, &id)?.ok_or("store_error")?.0)
}

pub(crate) fn sequence_list_in(data: &Path) -> Result<Vec<SeqDraft>, String> {
    let conn = open_db(data)?;
    let mut q = conn
        .prepare(&format!(
            "SELECT {DRAFT_COLS} FROM sequence_drafts ORDER BY created_at DESC, id"
        ))
        .map_err(store_err)?;
    let rows = q.query_map([], row_to_draft).map_err(store_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(store_err)?.0);
    }
    Ok(out)
}

/// An edit. Always validated again; the draft goes back to `draft` and loses `approved_at`.
pub(crate) fn sequence_update_in(
    data: &Path,
    id: &str,
    emails: &[SeqEmailInput],
) -> Result<SeqDraft, String> {
    if id.chars().count() > 64 {
        return Err("invalid_input".into());
    }
    if emails.len() > MAX_EMAILS
        || emails
            .iter()
            .any(|e| e.subject.len() > MAX_SUBJECT_CHARS * 4 || e.body.len() > MAX_BODY_CHARS * 4)
    {
        return Err("bad_draft".into());
    }
    let conn = open_db(data)?;
    let (cur, _) = get_draft(&conn, id)?.ok_or_else(|| "unknown_draft".to_string())?;
    let pairs: Vec<(String, String)> = emails
        .iter()
        .map(|e| (e.subject.clone(), e.body.clone()))
        .collect();
    let emails = check_emails(&cur.kind, &pairs)?;
    conn.execute(
        "UPDATE sequence_drafts SET emails_json = ?1, content_hash = ?2, status = 'draft', approved_at = NULL, updated_at = ?3 WHERE id = ?4",
        params![
            serde_json::to_string(&emails).map_err(store_err)?,
            content_hash(&emails)?,
            now_iso(),
            id
        ],
    )
    .map_err(store_err)?;
    Ok(get_draft(&conn, id)?.ok_or("store_error")?.0)
}

/// What the native dialog shows, and the hash of what was shown. Subjects only (the dialog is small);
/// the body is read in the app. Refuses an already approved draft.
pub(crate) fn approval_prepare_in(data: &Path, id: &str) -> Result<(String, String), String> {
    if id.chars().count() > 64 {
        return Err("invalid_input".into());
    }
    let conn = open_db(data)?;
    let (d, hash) = get_draft(&conn, id)?.ok_or_else(|| "unknown_draft".to_string())?;
    if d.status != "draft" {
        return Err("not_draft".into());
    }
    let subjects: Vec<String> = d
        .emails
        .iter()
        .enumerate()
        .map(|(i, e)| format!("{}. {}", i + 1, e.subject))
        .collect();
    let chars: usize = d.emails.iter().map(|e| e.body.chars().count()).sum();
    let detail = format!(
        "Template: {} ({} email(s), {chars} characters).\nSubjects:\n{}\n\nThis only marks the text as approved. Amara does not send email.",
        d.kind,
        d.emails.len(),
        subjects.join("\n")
    );
    let text =
        approval_gate::approval_text("Approve this email template as ready to use", &detail)?;
    Ok((text, hash))
}

/// After Allow: stores `approved` only if the text is still the text that was shown.
pub(crate) fn approval_commit_in(
    data: &Path,
    id: &str,
    shown_hash: &str,
) -> Result<SeqDraft, String> {
    let conn = open_db(data)?;
    let (d, hash) = get_draft(&conn, id)?.ok_or_else(|| "unknown_draft".to_string())?;
    if hash != shown_hash || d.status != "draft" {
        return Err(
            "changed: The template changed while the dialog was open. Nothing was approved.".into(),
        );
    }
    let n = conn
        .execute(
            "UPDATE sequence_drafts SET status = 'approved', approved_at = ?1, updated_at = ?1 WHERE id = ?2 AND content_hash = ?3 AND status = 'draft'",
            params![now_iso(), id, shown_hash],
        )
        .map_err(store_err)?;
    if n != 1 {
        return Err(
            "changed: The template changed while the dialog was open. Nothing was approved.".into(),
        );
    }
    Ok(get_draft(&conn, id)?.ok_or("store_error")?.0)
}

// ───────────────────────────── results ─────────────────────────────

fn sequence_exists(conn: &Connection, id: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM sequence_drafts WHERE id = ?1",
        params![id],
        |_| Ok(()),
    )
    .optional()
    .map(|o| o.is_some())
    .map_err(store_err)
}

fn metric_of(raw: &str) -> Option<i64> {
    let t = raw.trim();
    if t.is_empty() || t.len() > 10 || !t.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    t.parse::<i64>().ok().filter(|n| *n <= MAX_METRIC)
}

#[derive(Default, Clone)]
struct Acc {
    sends: Option<i64>,
    bounces: Option<i64>,
    unsubscribes: Option<i64>,
    purchases: Option<i64>,
}

fn add(a: &mut Option<i64>, v: Option<i64>) {
    if let Some(v) = v {
        *a = Some((a.unwrap_or(0) + v).min(MAX_METRIC));
    }
}

const EARLIEST_DAY: &str = "2000-01-01";

/// A results day must be 2000-01-01 ..= today + 365 days.
fn day_in_range(day: &str, today: chrono::NaiveDate) -> bool {
    let latest = (today + chrono::Days::new(365))
        .format("%Y-%m-%d")
        .to_string();
    day >= EARLIEST_DAY && day <= latest.as_str()
}

struct ParsedResults {
    sha: String,
    rows_seen: i64,
    failed: i64,
    days: BTreeMap<String, Acc>,
}

fn parse_results(
    mapping: &ResultsMapping,
    path: &Path,
    today: chrono::NaiveDate,
) -> Result<ParsedResults, (Option<String>, String)> {
    let too_long = |c: &Option<String>| c.as_deref().is_some_and(|c| c.chars().count() > 128);
    if mapping.date_column.chars().count() > 128
        || too_long(&mapping.sends_column)
        || too_long(&mapping.bounces_column)
        || too_long(&mapping.unsubscribes_column)
        || too_long(&mapping.purchases_column)
    {
        return Err((None, "invalid_input".into()));
    }
    let order = match mapping.date_format.as_deref().unwrap_or("ymd") {
        "ymd" => DateOrder::Ymd,
        "mdy" => DateOrder::Mdy,
        "dmy" => DateOrder::Dmy,
        _ => return Err((None, "invalid_input".into())),
    };
    let named = |c: &Option<String>| c.as_deref().is_some_and(|c| !c.trim().is_empty());
    if mapping.date_column.trim().is_empty()
        || !(named(&mapping.sends_column)
            || named(&mapping.bounces_column)
            || named(&mapping.unsubscribes_column)
            || named(&mapping.purchases_column))
    {
        return Err((None, "invalid_input".into()));
    }
    let bytes = read_import_file(path).map_err(|e| (None, e))?;
    let sha = hex::encode(Sha256::digest(&bytes));
    let fail = |c: &str| (Some(sha.clone()), c.to_string());
    let text = std::str::from_utf8(&bytes).map_err(|_| fail("bad_encoding"))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rows = parse_csv(text).map_err(|e| fail(&e))?;
    let mut iter = rows.into_iter();
    let (_, header) = iter.next().ok_or_else(|| fail("bad_header"))?;
    let col = |c: &Option<String>| -> Result<Option<usize>, (Option<String>, String)> {
        match c.as_deref() {
            Some(n) if !n.trim().is_empty() => super::treasury::find_col(&header, n)
                .map(Some)
                .ok_or_else(|| fail("bad_header")),
            _ => Ok(None),
        }
    };
    let date_col = super::treasury::find_col(&header, &mapping.date_column)
        .ok_or_else(|| fail("bad_header"))?;
    let (sc, bc, uc, pc) = (
        col(&mapping.sends_column)?,
        col(&mapping.bounces_column)?,
        col(&mapping.unsubscribes_column)?,
        col(&mapping.purchases_column)?,
    );
    let mut days: BTreeMap<String, Acc> = BTreeMap::new();
    let (mut seen, mut failed) = (0i64, 0i64);
    for (_, cells) in iter {
        seen += 1;
        let one = (|| -> Option<(String, [Option<i64>; 4])> {
            if cells.len() > MAX_CELLS {
                return None;
            }
            let day = parse_date(cells.get(date_col)?, order)?;
            if !day_in_range(&day, today) {
                return None;
            }
            let mut vals = [None; 4];
            for (slot, c) in vals.iter_mut().zip([sc, bc, uc, pc]) {
                if let Some(i) = c {
                    *slot = Some(metric_of(cells.get(i)?)?);
                }
            }
            Some((day, vals))
        })();
        match one {
            Some((day, v)) => {
                let a = days.entry(day).or_default();
                add(&mut a.sends, v[0]);
                add(&mut a.bounces, v[1]);
                add(&mut a.unsubscribes, v[2]);
                add(&mut a.purchases, v[3]);
            }
            None => failed += 1,
        }
    }
    if days.is_empty() {
        return Err(fail("no_rows"));
    }
    Ok(ParsedResults {
        sha,
        rows_seen: seen,
        failed,
        days,
    })
}

fn store_days(
    conn: &mut Connection,
    sequence_id: &str,
    days: &BTreeMap<String, Acc>,
    sha: Option<&str>,
    rows_seen: i64,
    failed: i64,
) -> Result<ResultsImportResult, String> {
    let tx = conn.transaction().map_err(store_err)?;
    let import_id = new_id("rim");
    tx.execute(
        "INSERT INTO results_imports(id, sequence_id, file_sha256, row_count, status, at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            import_id,
            sequence_id,
            sha,
            rows_seen,
            if failed > 0 { "partial" } else { "ok" },
            now_iso()
        ],
    )
    .map_err(store_err)?;
    let (mut added, mut skipped) = (0i64, 0i64);
    for (day, a) in days {
        let same: bool = tx
            .query_row(
                "SELECT 1 FROM sequence_results WHERE sequence_id = ?1 AND day = ?2 AND sends IS ?3 AND bounces IS ?4 AND unsubscribes IS ?5 AND purchases_after IS ?6",
                params![sequence_id, day, a.sends, a.bounces, a.unsubscribes, a.purchases],
                |_| Ok(()),
            )
            .optional()
            .map_err(store_err)?
            .is_some();
        if same {
            skipped += 1;
            continue;
        }
        tx.execute(
            "INSERT OR REPLACE INTO sequence_results(sequence_id, day, sends, bounces, unsubscribes, purchases_after, import_id)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![sequence_id, day, a.sends, a.bounces, a.unsubscribes, a.purchases, import_id],
        )
        .map_err(store_err)?;
        added += 1;
    }
    tx.commit().map_err(store_err)?;
    Ok(ResultsImportResult {
        added,
        skipped,
        failed,
        import_id,
    })
}

fn record_results_failed(conn: &Connection, sequence_id: &str, sha: Option<&str>, code: &str) {
    let _ = conn.execute(
        "INSERT INTO results_imports(id, sequence_id, file_sha256, row_count, status, at) VALUES(?1, ?2, ?3, 0, ?4, ?5)",
        params![new_id("rim"), sequence_id, sha, format!("failed:{code}"), now_iso()],
    );
}

pub(crate) fn results_import_in(
    data: &Path,
    sequence_id: &str,
    mapping: &ResultsMapping,
    path: Option<&Path>,
) -> Result<ResultsImportResult, String> {
    results_import_on(data, sequence_id, mapping, path, super::treasury::today())
}

pub(crate) fn results_import_on(
    data: &Path,
    sequence_id: &str,
    mapping: &ResultsMapping,
    path: Option<&Path>,
    today: chrono::NaiveDate,
) -> Result<ResultsImportResult, String> {
    if sequence_id.chars().count() > 64 {
        return Err("invalid_input".into());
    }
    let path = path.ok_or_else(|| "cancelled".to_string())?;
    let mut conn = open_db(data)?;
    if !sequence_exists(&conn, sequence_id)? {
        return Err("unknown_sequence".into());
    }
    let p = match parse_results(mapping, path, today) {
        Ok(p) => p,
        Err((sha, code)) => {
            if code != "invalid_input" {
                record_results_failed(&conn, sequence_id, sha.as_deref(), &code);
            }
            return Err(code);
        }
    };
    store_days(
        &mut conn,
        sequence_id,
        &p.days,
        Some(&p.sha),
        p.rows_seen,
        p.failed,
    )
}

/// [GATE] Stores what a `ProviderResultsSource` returns. Used by the fixture test only: no source that
/// talks to a provider exists.
#[allow(dead_code)]
pub(crate) fn results_sync_in(
    data: &Path,
    sequence_id: &str,
    source: &dyn ProviderResultsSource,
    from: &str,
    to: &str,
) -> Result<ResultsImportResult, String> {
    if !is_date(from) || !is_date(to) || from > to {
        return Err("invalid_input".into());
    }
    let mut conn = open_db(data)?;
    if !sequence_exists(&conn, sequence_id)? {
        return Err("unknown_sequence".into());
    }
    let rows = source.fetch_days(sequence_id, from, to)?;
    let mut days: BTreeMap<String, Acc> = BTreeMap::new();
    for r in &rows {
        let ok = |v: Option<i64>| v.is_none_or(|n| (0..=MAX_METRIC).contains(&n));
        if r.sequence_id != sequence_id
            || !is_date(&r.day)
            || !day_in_range(&r.day, super::treasury::today())
            || !ok(r.sends)
            || !ok(r.bounces)
            || !ok(r.unsubscribes)
            || !ok(r.purchases_after)
        {
            return Err("upstream_error".into());
        }
        let a = days.entry(r.day.clone()).or_default();
        add(&mut a.sends, r.sends);
        add(&mut a.bounces, r.bounces);
        add(&mut a.unsubscribes, r.unsubscribes);
        add(&mut a.purchases, r.purchases_after);
    }
    if days.is_empty() {
        return Err("no_rows".into());
    }
    store_days(&mut conn, sequence_id, &days, None, rows.len() as i64, 0)
}

pub(crate) fn results_list_in(
    data: &Path,
    sequence_id: Option<&str>,
) -> Result<Vec<ResultsDay>, String> {
    let conn = open_db(data)?;
    let mut q = conn
        .prepare(
            "SELECT sequence_id, day, sends, bounces, unsubscribes, purchases_after FROM sequence_results
             WHERE (?1 IS NULL OR sequence_id = ?1) ORDER BY sequence_id, day",
        )
        .map_err(store_err)?;
    let rows = q
        .query_map(params![sequence_id], |r| {
            Ok(ResultsDay {
                sequence_id: r.get(0)?,
                day: r.get(1)?,
                sends: r.get(2)?,
                bounces: r.get(3)?,
                unsubscribes: r.get(4)?,
                purchases_after: r.get(5)?,
            })
        })
        .map_err(store_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(store_err)?);
    }
    Ok(out)
}

// ───────────────────────────── commands ─────────────────────────────

static DRAFTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One model call at a time. Dropping it frees the slot, panics included.
struct DraftSlot;

impl DraftSlot {
    fn take() -> Option<DraftSlot> {
        use std::sync::atomic::Ordering;
        DRAFTING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| DraftSlot)
    }
}

impl Drop for DraftSlot {
    fn drop(&mut self) {
        DRAFTING.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_sequence_draft(kind: String, venture_id: String) -> Result<SeqDraft, String> {
    let _slot = DraftSlot::take().ok_or_else(|| "busy".to_string())?;
    let (k, v) = (kind.clone(), venture_id.clone());
    let name = blocking(JoinErr::Code("store_error"), move || {
        venture_name_for_draft(&crate::app_paths::data_dir(), &k, &v)
    })
    .await?;
    let (instruction, user) =
        writer_prompt(&kind, &name).ok_or_else(|| "invalid_input".to_string())?;
    // The light local tier only. A reply from the Claude tier is refused inside `local_complete`.
    let reply = super::llm_router::local_complete(&instruction, &user)
        .await
        .map_err(|_| "writer_failed".to_string())?;
    blocking(JoinErr::Code("store_error"), move || {
        draft_store_in(&crate::app_paths::data_dir(), &kind, &venture_id, &reply)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_sequence_list() -> Result<Vec<SeqDraft>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        sequence_list_in(&crate::app_paths::data_dir())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_sequence_update(
    id: String,
    emails: Vec<SeqEmailInput>,
) -> Result<SeqDraft, String> {
    blocking(JoinErr::Code("store_error"), move || {
        sequence_update_in(&crate::app_paths::data_dir(), &id, &emails)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_sequence_approve(
    app: tauri::AppHandle,
    id: String,
) -> Result<SeqDraft, String> {
    let id2 = id.clone();
    let (text, hash) = blocking(JoinErr::Code("store_error"), move || {
        approval_prepare_in(&crate::app_paths::data_dir(), &id2)
    })
    .await?;
    let key = format!("treasury_seq:{id}");
    let _guard = approval_gate::require_approval(&app, &[key], text).await?;
    blocking(JoinErr::Code("store_error"), move || {
        approval_commit_in(&crate::app_paths::data_dir(), &id, &hash)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_results_import_pick(
    app: tauri::AppHandle,
    sequence_id: String,
    mapping: ResultsMapping,
) -> Result<ResultsImportResult, String> {
    let _slot = Slot::take().ok_or_else(|| "busy".to_string())?;
    let path = pick_csv_titled(app, "Choose an email results CSV").await?;
    blocking(JoinErr::Code("failed"), move || {
        results_import_in(
            &crate::app_paths::data_dir(),
            &sequence_id,
            &mapping,
            path.as_deref(),
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn treasury_results_list(sequence_id: Option<String>) -> Result<Vec<ResultsDay>, String> {
    blocking(JoinErr::Code("store_error"), move || {
        results_list_in(&crate::app_paths::data_dir(), sequence_id.as_deref())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::treasury::test_support::{csv_file, setup, BANNED};
    use std::fs;

    fn mail(n: usize, with_unsub: bool, with_addr: bool) -> String {
        let mut s = String::new();
        for i in 1..=n {
            s.push_str(&format!("=== EMAIL {i} ===\nSubject: Hello number {i}\nHi {{{{first_name}}}},\n\nThanks for being here.\n\n"));
            if with_unsub {
                s.push_str("Unsubscribe: {{unsubscribe_link}}\n");
            }
            if with_addr {
                s.push_str("{{mailing_address}}\n");
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn four_kinds_draft_with_the_right_email_counts_and_a_full_checklist() {
        let (d, _) = setup("seq-kinds");
        for (kind, n) in [
            ("welcome", 3),
            ("post_purchase", 1),
            ("reorder", 1),
            ("win_back", 1),
        ] {
            let dr = draft_store_in(&d, kind, "v1", &mail(n, true, true)).unwrap();
            assert_eq!(dr.emails.len(), n, "{kind}");
            assert_eq!(dr.status, "draft");
            assert!(dr.approved_at.is_none());
            let all = dr.checklist.join("\n").to_lowercase();
            for must in [
                "recorded consent",
                "honour opt-outs at once",
                "trigger:",
                "delay",
                "stage",
                "tags:",
                "japan requires opt-in",
                "gdpr",
                "not legal advice",
                "schedule send",
            ] {
                assert!(all.contains(must), "{kind} checklist lacks {must}");
            }
            for e in &dr.emails {
                assert!(
                    e.body.contains(UNSUBSCRIBE_PLACEHOLDER)
                        && e.body.contains(ADDRESS_PLACEHOLDER)
                );
            }
        }
        assert_eq!(sequence_list_in(&d).unwrap().len(), 4);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_draft_missing_either_placeholder_or_with_the_wrong_shape_is_rejected_and_not_stored() {
        let (d, _) = setup("seq-reject");
        assert_eq!(
            draft_store_in(&d, "welcome", "v1", &mail(3, false, true)).unwrap_err(),
            "missing_placeholder"
        );
        assert_eq!(
            draft_store_in(&d, "welcome", "v1", &mail(3, true, false)).unwrap_err(),
            "missing_placeholder"
        );
        // One email lacking the footer in an otherwise good sequence.
        let mut m = mail(2, true, true);
        m.push_str("=== EMAIL 3 ===\nSubject: No footer\nJust text.\n");
        assert_eq!(
            draft_store_in(&d, "welcome", "v1", &m).unwrap_err(),
            "missing_placeholder"
        );
        assert_eq!(
            draft_store_in(&d, "welcome", "v1", &mail(2, true, true)).unwrap_err(),
            "bad_draft"
        );
        assert_eq!(
            draft_store_in(&d, "reorder", "v1", "no markers at all").unwrap_err(),
            "bad_draft"
        );
        assert_eq!(
            draft_store_in(
                &d,
                "reorder",
                "v1",
                "=== EMAIL 1 ===\nno subject line\n{{unsubscribe_link}} {{mailing_address}}"
            )
            .unwrap_err(),
            "bad_draft"
        );
        assert_eq!(
            draft_store_in(&d, "nonsense", "v1", &mail(1, true, true)).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            draft_store_in(&d, "reorder", "nope", &mail(1, true, true)).unwrap_err(),
            "unknown_venture"
        );
        assert!(sequence_list_in(&d).unwrap().is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn editing_validates_and_returns_an_approved_draft_to_draft() {
        let (d, _) = setup("seq-edit");
        let dr = draft_store_in(&d, "reorder", "v1", &mail(1, true, true)).unwrap();
        let (text, hash) = approval_prepare_in(&d, &dr.id).unwrap();
        assert!(text.contains("Hello number 1") && !text.contains("Thanks for being here"));
        let ap = approval_commit_in(&d, &dr.id, &hash).unwrap();
        assert_eq!(ap.status, "approved");
        assert!(ap.approved_at.is_some());
        assert_eq!(approval_prepare_in(&d, &dr.id).unwrap_err(), "not_draft");
        let bad = [SeqEmailInput {
            subject: "S".into(),
            body: "no footer".into(),
        }];
        assert_eq!(
            sequence_update_in(&d, &dr.id, &bad).unwrap_err(),
            "missing_placeholder"
        );
        assert_eq!(
            sequence_list_in(&d).unwrap()[0].status,
            "approved",
            "a rejected edit changes nothing"
        );
        let good = [SeqEmailInput {
            subject: "New".into(),
            body: "Hi {{unsubscribe_link}} {{mailing_address}}".into(),
        }];
        let ed = sequence_update_in(&d, &dr.id, &good).unwrap();
        assert_eq!((ed.status.as_str(), ed.approved_at), ("draft", None));
        assert_eq!(
            sequence_update_in(&d, "nope", &good).unwrap_err(),
            "unknown_draft"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn an_edit_during_the_dialog_blocks_the_approval() {
        let (d, _) = setup("seq-race");
        let dr = draft_store_in(&d, "reorder", "v1", &mail(1, true, true)).unwrap();
        let (_, hash) = approval_prepare_in(&d, &dr.id).unwrap();
        let edit = [SeqEmailInput {
            subject: "Changed".into(),
            body: "x {{unsubscribe_link}} {{mailing_address}}".into(),
        }];
        sequence_update_in(&d, &dr.id, &edit).unwrap();
        let e = approval_commit_in(&d, &dr.id, &hash).unwrap_err();
        assert!(e.starts_with("changed:"), "{e}");
        assert_eq!(sequence_list_in(&d).unwrap()[0].status, "draft");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn the_writer_prompt_has_no_contact_data_and_names_the_placeholders() {
        let (ins, user) = writer_prompt("welcome", "Shop Venture").unwrap();
        assert!(ins.contains(UNSUBSCRIBE_PLACEHOLDER) && ins.contains(ADDRESS_PLACEHOLDER));
        let all = format!("{ins}\n{user}");
        assert!(!all.contains('@'), "no email address in the prompt");
        assert!(user.contains("Shop Venture") && user.contains("3 email"));
        assert!(writer_prompt("nonsense", "x").is_none());
    }

    fn results_map() -> ResultsMapping {
        ResultsMapping {
            date_column: "Day".into(),
            sends_column: Some("Sent".into()),
            bounces_column: Some("Bounced".into()),
            unsubscribes_column: Some("Opted out".into()),
            purchases_column: Some("Purchases".into()),
            date_format: None,
        }
    }

    const RESULTS: &str = "Day,Recipient,Sent,Bounced,Opted out,Purchases\n\
2026-11-02,zed.nobody@example.invalid,10,1,0,2\n\
2026-11-02,amy.nobody@example.invalid,5,0,1,0\n\
2026-11-03,zed.nobody@example.invalid,7,0,0,1\n";

    #[test]
    fn results_import_sums_per_day_is_idempotent_and_stores_no_recipient() {
        let (d, _) = setup("seq-results");
        let dr = draft_store_in(&d, "welcome", "v1", &mail(3, true, true)).unwrap();
        let f = csv_file(&d, "r.csv", RESULTS);
        let r1 = results_import_in(&d, &dr.id, &results_map(), Some(&f)).unwrap();
        assert_eq!((r1.added, r1.skipped, r1.failed), (2, 0, 0));
        let r2 = results_import_in(&d, &dr.id, &results_map(), Some(&f)).unwrap();
        assert_eq!((r2.added, r2.skipped), (0, 2));
        let rows = results_list_in(&d, Some(&dr.id)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (
                rows[0].sends,
                rows[0].bounces,
                rows[0].unsubscribes,
                rows[0].purchases_after
            ),
            (Some(15), Some(1), Some(1), Some(2))
        );
        assert_eq!(results_list_in(&d, Some("other")).unwrap().len(), 0);
        for entry in fs::read_dir(d.join("business")).unwrap() {
            let bytes = fs::read(entry.unwrap().path()).unwrap_or_default();
            let hay = String::from_utf8_lossy(&bytes).to_lowercase();
            assert!(!hay.contains("nobody") && !hay.contains("example.invalid"));
        }
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn an_unmapped_metric_is_null_not_zero_and_bad_files_use_fixed_codes() {
        let (d, _) = setup("seq-results2");
        let dr = draft_store_in(&d, "reorder", "v1", &mail(1, true, true)).unwrap();
        let m = ResultsMapping {
            date_column: "Day".into(),
            sends_column: Some("Sent".into()),
            bounces_column: None,
            unsubscribes_column: None,
            purchases_column: None,
            date_format: None,
        };
        let f = csv_file(&d, "r.csv", RESULTS);
        results_import_in(&d, &dr.id, &m, Some(&f)).unwrap();
        let rows = results_list_in(&d, None).unwrap();
        assert_eq!(
            (rows[0].sends, rows[0].bounces, rows[0].purchases_after),
            (Some(15), None, None)
        );
        let wrong = csv_file(&d, "w.csv", "Email,Level\nzed.nobody@example.invalid,x\n");
        let e = results_import_in(&d, &dr.id, &m, Some(&wrong)).unwrap_err();
        assert_eq!(e, "bad_header");
        assert!(!e.contains('@'));
        let junk = csv_file(
            &d,
            "j.csv",
            "Day,Sent\nnot-a-date,5\n2026-11-04,zed.nobody@example.invalid\n",
        );
        assert_eq!(
            results_import_in(&d, &dr.id, &m, Some(&junk)).unwrap_err(),
            "no_rows"
        );
        let none = ResultsMapping {
            date_column: "Day".into(),
            sends_column: None,
            bounces_column: None,
            unsubscribes_column: None,
            purchases_column: None,
            date_format: None,
        };
        assert_eq!(
            results_import_in(&d, &dr.id, &none, Some(&f)).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            results_import_in(&d, "nope", &m, Some(&f)).unwrap_err(),
            "unknown_sequence"
        );
        assert_eq!(
            results_import_in(&d, &dr.id, &m, None).unwrap_err(),
            "cancelled"
        );
        #[cfg(unix)]
        {
            let link = d.join("l.csv");
            std::os::unix::fs::symlink(&f, &link).unwrap();
            assert_eq!(
                results_import_in(&d, &dr.id, &m, Some(&link)).unwrap_err(),
                "bad_file"
            );
        }
        let _ = fs::remove_dir_all(d);
    }

    struct Fixture(Vec<ResultsDay>);
    impl ProviderResultsSource for Fixture {
        fn fetch_days(&self, _s: &str, from: &str, to: &str) -> Result<Vec<ResultsDay>, String> {
            Ok(self
                .0
                .iter()
                .filter(|r| r.day.as_str() >= from && r.day.as_str() <= to)
                .cloned()
                .collect())
        }
    }

    #[test]
    fn a_fixture_backed_source_goes_through_the_same_store_and_no_provider_client_exists() {
        let (d, _) = setup("seq-gate");
        let dr = draft_store_in(&d, "win_back", "v1", &mail(1, true, true)).unwrap();
        let day = |day: &str, s: i64| ResultsDay {
            sequence_id: dr.id.clone(),
            day: day.into(),
            sends: Some(s),
            bounces: None,
            unsubscribes: Some(0),
            purchases_after: None,
        };
        let src = Fixture(vec![
            day("2026-11-01", 4),
            day("2026-11-02", 6),
            day("2026-12-01", 9),
        ]);
        let r = results_sync_in(&d, &dr.id, &src, "2026-11-01", "2026-11-30").unwrap();
        assert_eq!((r.added, r.skipped), (2, 0));
        assert_eq!(
            results_sync_in(&d, &dr.id, &src, "2026-11-01", "2026-11-30")
                .unwrap()
                .skipped,
            2
        );
        assert_eq!(
            results_sync_in(&d, &dr.id, &src, "2026-11-30", "2026-11-01").unwrap_err(),
            "invalid_input"
        );
        let bad = Fixture(vec![ResultsDay {
            sends: Some(-1),
            ..day("2026-11-01", 0)
        }]);
        assert_eq!(
            results_sync_in(&d, &dr.id, &bad, "2026-11-01", "2026-11-30").unwrap_err(),
            "upstream_error"
        );
        // No production code implements the source trait.
        assert_eq!(
            production().matches("impl ProviderResultsSource").count(),
            0
        );
        let _ = fs::remove_dir_all(d);
    }

    fn body_with(extra: &str) -> String {
        format!("Hi {{{{first_name}}}} {extra}\n{{{{unsubscribe_link}}}}\n{{{{mailing_address}}}}")
    }

    #[test]
    fn hidden_markup_urls_contact_data_and_unknown_placeholders_are_refused() {
        assert!(check_email("Hello", &body_with("tab\there")).is_ok());
        for (what, extra) in [
            ("control", "a\u{0007}b"),
            ("carriage return", "a\rb"),
            ("bidi", "a\u{202e}b"),
            ("bidi isolate", "a\u{2066}b"),
            ("zero width", "a\u{200b}b"),
            ("line separator", "a\u{2028}b"),
            ("angle", "<b>x</b>"),
            ("javascript", "JavaScript:alert(1)"),
            ("url", concat!("see ", "https", "://x.example.invalid/p")),
            ("www", "see www.example.invalid"),
            ("email", "write to nobody@example.invalid"),
            ("phone", "call +81 90 1234 5678"),
            ("placeholder", "{{customer_email}}"),
            ("stray brace", "oops {{first_name"),
        ] {
            assert!(check_email("Hello", &body_with(extra)).is_err(), "{what}");
        }
        for (what, subj) in [
            ("control", "a\tb"),
            ("newline", "a\nb"),
            ("bidi", "a\u{202a}b"),
            ("email", "to a@b"),
        ] {
            assert!(check_email(subj, &body_with("")).is_err(), "subject {what}");
        }
        // The same rules apply to an edit.
        let (d, _) = setup("seq-copy");
        let dr = draft_store_in(&d, "reorder", "v1", &mail(1, true, true)).unwrap();
        let bad = [SeqEmailInput {
            subject: "S".into(),
            body: body_with("<script>"),
        }];
        assert_eq!(
            sequence_update_in(&d, &dr.id, &bad).unwrap_err(),
            "bad_draft"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn a_writer_reply_with_hidden_or_foreign_content_is_not_stored() {
        let (d, _) = setup("seq-reply");
        let reply = mail(1, true, true).replace("Thanks", "Thanks \u{202e}");
        assert_eq!(
            draft_store_in(&d, "reorder", "v1", &reply).unwrap_err(),
            "bad_draft"
        );
        assert!(sequence_list_in(&d).unwrap().is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn only_one_model_call_at_a_time_and_the_draft_table_is_capped() {
        let a = DraftSlot::take().expect("free");
        assert!(DraftSlot::take().is_none());
        drop(a);
        assert!(DraftSlot::take().is_some());
        let (d, _) = setup("seq-cap");
        let conn = open_db(&d).unwrap();
        for i in 0..MAX_DRAFTS {
            conn.execute(
                "INSERT INTO sequence_drafts(id, venture_id, kind, emails_json, checklist_json, status, content_hash, approved_at, created_at, updated_at)
                 VALUES(?1, 'v1', 'reorder', '[]', '[]', 'draft', 'h', NULL, 'x', 'x')",
                params![format!("seq-{i}")],
            )
            .unwrap();
        }
        assert_eq!(
            draft_store_in(&d, "reorder", "v1", &mail(1, true, true)).unwrap_err(),
            "too_many_drafts"
        );
        assert_eq!(
            venture_name_for_draft(&d, "reorder", "v1").unwrap_err(),
            "too_many_drafts"
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn oversized_edits_ids_and_columns_are_refused_early_and_the_venture_name_is_bounded() {
        let (d, _) = setup("seq-caps");
        let dr = draft_store_in(&d, "reorder", "v1", &mail(1, true, true)).unwrap();
        let big = |n: usize| SeqEmailInput {
            subject: "S".into(),
            body: "x".repeat(n),
        };
        assert_eq!(
            sequence_update_in(&d, &dr.id, &[big(10), big(10)]).unwrap_err(),
            "bad_draft"
        );
        assert_eq!(
            sequence_update_in(&d, &dr.id, &[big(MAX_BODY_CHARS * 4 + 1)]).unwrap_err(),
            "bad_draft"
        );
        assert_eq!(
            sequence_update_in(&d, &"i".repeat(65), &[big(1)]).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            approval_prepare_in(&d, &"i".repeat(65)).unwrap_err(),
            "invalid_input"
        );
        let mut m = results_map();
        m.sends_column = Some("c".repeat(129));
        let f = csv_file(&d, "r.csv", RESULTS);
        assert_eq!(
            results_import_in(&d, &dr.id, &m, Some(&f)).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            results_import_in(&d, &"i".repeat(65), &results_map(), Some(&f)).unwrap_err(),
            "invalid_input"
        );
        let (_, user) =
            writer_prompt("reorder", &format!("Shop\n\n{}  x", "n".repeat(200))).unwrap();
        let brand = user.lines().next().unwrap();
        assert!(brand.chars().count() <= "Brand: ".len() + 60, "{brand}");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn results_dates_are_clamped_to_2000_through_a_year_ahead() {
        let (d, _) = setup("seq-dates");
        let dr = draft_store_in(&d, "reorder", "v1", &mail(1, true, true)).unwrap();
        let m = ResultsMapping {
            date_column: "Day".into(),
            sends_column: Some("Sent".into()),
            bounces_column: None,
            unsubscribes_column: None,
            purchases_column: None,
            date_format: None,
        };
        let f = csv_file(
            &d,
            "r.csv",
            "Day,Sent\n1999-12-31,1\n2000-01-01,2\n2027-11-18,3\n2027-11-19,4\n9999-01-01,5\n",
        );
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
        let r = results_import_on(&d, &dr.id, &m, Some(&f), today).unwrap();
        assert_eq!((r.added, r.failed), (2, 3));
        let days: Vec<String> = results_list_in(&d, None)
            .unwrap()
            .into_iter()
            .map(|r| r.day)
            .collect();
        assert_eq!(days, ["2000-01-01", "2027-11-18"]);
        let _ = fs::remove_dir_all(d);
    }

    fn production() -> String {
        include_str!("treasury_sequences.rs")
            .split(concat!("#[cfg(test)]", "\nmod tests {"))
            .next()
            .unwrap()
            .to_string()
    }

    #[test]
    fn local_tier_only_no_logging_no_network_no_send_and_safe_command_names() {
        let src = production();
        assert!(src.contains("llm_router::local_complete"));
        // Doc comments mention Gmail and the gate in prose; only code lines are scanned.
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for needle in [
            "println!",
            "eprintln!",
            "dbg!",
            "log::",
            "tracing::",
            "info!(",
            "warn!(",
            "error!(",
            "debug!(",
            "reqwest",
            "shared::http",
            "TcpStream",
            "UdpSocket",
            concat!("http", "://"),
            concat!("https", "://"),
            "Command::new",
            concat!("money", "::"),
            "router_complete",
            "allow_claude",
            "Tier::Claude",
            "smtp",
            "notify_relay",
            "telegram",
        ] {
            assert!(
                !code.to_lowercase().contains(&needle.to_lowercase()),
                "production source contains {needle}"
            );
        }
        let mut names = Vec::new();
        let mut lines = src.lines();
        while let Some(l) = lines.next() {
            if l.trim() == "#[tauri::command]" {
                let f = lines.find(|l| l.contains("fn ")).unwrap();
                names.push(
                    f.split("fn ")
                        .nth(1)
                        .unwrap()
                        .split('(')
                        .next()
                        .unwrap()
                        .trim()
                        .to_string(),
                );
            }
        }
        assert_eq!(
            names,
            [
                "treasury_sequence_draft",
                "treasury_sequence_list",
                "treasury_sequence_update",
                "treasury_sequence_approve",
                "treasury_results_import_pick",
                "treasury_results_list"
            ]
        );
        for n in &names {
            for b in BANNED {
                assert!(!n.contains(b), "{n} contains {b}");
            }
        }
        // Approval goes through the native gate, before the write.
        let body = &src[src.find("pub async fn treasury_sequence_approve").unwrap()..];
        assert!(
            body.find("require_approval(").unwrap() < body.find("approval_commit_in(").unwrap()
        );
    }
}
