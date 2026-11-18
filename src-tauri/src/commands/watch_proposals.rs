//! 1118a lane B2, watch-only proposals (`tasks/HANDOFF-1118a.md` section 9.2; decision 0036).
//!
//! A `WatchProposal` is a note Amara may leave on the Ventures > Trading page: a ticker, a reason, a source,
//! a confidence and a risk line. Taisun can RECORD his choice (after the page's second confirm) or dismiss it.
//! Recording is a log entry and nothing else.
//!
//! Rules this file keeps (each has a test):
//! - Nothing here sends anything: no HTTP client, no process, no wallet or account code, no credentials. The
//!   source scan test reads this file and refuses those words.
//! - Record and dismiss go through `adds::logged_write`, the one History + 8 s Undo path
//!   (`add_undo("watch", id)` puts the proposal back to `proposed`).
//! - Creating a proposal is NOT a command (`create_in` is `pub(crate)`): page code cannot plant a proposal that
//!   reads as Amara's. Only a `FeedSource` run by a service may call it. The one live source is Amara's
//!   research in `watchlist.rs` (decision 0038); this file still has no network code.
//! - `sample: true` proposals are never stored; `watch_proposals_list` adds fictional ones only when the page
//!   asks (its sample flag). Record and dismiss refuse them.
//! - All feed text is untrusted: flattened, capped, ticker `^[A-Z0-9.\-]{1,12}$`, `sourceUrl` https only and
//!   display-only (nothing here fetches it).
//! - The daily loss cap and kill switch are not read or changed here.
//!
//! Never logs tickers, reasons or sources.

use super::adds::{self, Dirs, UndoRef};
use super::goal_roadmap::clean_text;
use super::local_store::{iso, new_id, now_jst, read_json, write_json_atomic};
use super::shared::task::{blocking, JoinErr};
use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const NOT_ADVICE: &str = "Not financial advice";
pub const TOAST_RECORDED: &str = "Recorded. Watch-only \u{2014} no order was sent.";
pub const TOAST_DISMISSED: &str = "Dismissed.";

const FILE: &str = "proposals.json";
const MAX_BYTES: u64 = 1024 * 1024;
const MAX_PROPOSALS: usize = 200;
const MAX_REASON: usize = 300;
const MAX_RISK: usize = 200;
const MAX_SOURCE: usize = 60;
const MAX_URL: usize = 300;
const MAX_ID: usize = 64;

static STORE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum WatchKind {
    Crypto,
    Stock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum WatchSide {
    Buy,
    Sell,
    Watch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum WatchConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum WatchStatus {
    Proposed,
    Recorded,
    Dismissed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchSize {
    pub amount: f64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchProposal {
    pub id: String,
    pub created_at: String,
    pub kind: WatchKind,
    pub side: WatchSide,
    pub ticker: String,
    /// Crypto only: the price source's coin id (`^[a-z0-9-]{1,64}$`), used to look up the price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coin_id: Option<String>,
    pub reason_text: String,
    pub source_name: String,
    /// https only, display-only: never fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    pub confidence: WatchConfidence,
    pub risk_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<WatchSize>,
    pub status: WatchStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<bool>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    proposals: Vec<WatchProposal>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchList {
    pub proposals: Vec<WatchProposal>,
    /// Shown on every proposal.
    pub disclaimer: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchDecision {
    /// History id: `add_undo("watch", id)` within 8 s takes it back.
    pub id: String,
    pub proposal: WatchProposal,
    /// Fixed text for the toast.
    pub toast: String,
}

// ── Feed interface (no live implementation: gated) ───────────────────────────

/// One candidate from a feed. Untrusted until `create_in` has cleaned it.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct FeedItem {
    pub kind: WatchKind,
    pub side: WatchSide,
    pub ticker: String,
    pub coin_id: Option<String>,
    pub reason_text: String,
    pub source_name: String,
    pub source_url: Option<String>,
    pub confidence: WatchConfidence,
    pub risk_text: String,
    pub size: Option<WatchSize>,
}

/// Where candidates come from: Amara's research (`watchlist.rs`, decision 0038) and test fixtures.
#[allow(dead_code)]
pub(crate) trait FeedSource {
    fn name(&self) -> &str;
    fn items(&self) -> Result<Vec<FeedItem>, String>;
}

// ── Cleaning ─────────────────────────────────────────────────────────────────

pub(crate) fn valid_ticker(t: &str) -> bool {
    (1..=12).contains(&t.len())
        && t.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// https only, a plain host, no user info, no spaces or control characters. Display-only.
pub(crate) fn clean_url(u: &str) -> Option<String> {
    let rest = u.strip_prefix("https://")?;
    if u.len() > MAX_URL
        || u.chars()
            .any(|c| c.is_control() || c.is_whitespace() || !c.is_ascii())
    {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') || host.starts_with('.') {
        return None;
    }
    Some(u.to_string())
}

fn clean_size(s: &WatchSize) -> Option<WatchSize> {
    let ok = s.amount.is_finite()
        && s.amount > 0.0
        && s.amount <= 1_000_000_000.0
        && (3..=5).contains(&s.currency.len())
        && s.currency.bytes().all(|b| b.is_ascii_uppercase());
    ok.then(|| s.clone())
}

// ── Disk ─────────────────────────────────────────────────────────────────────

fn path(data: &Path) -> PathBuf {
    data.join("watch").join(FILE)
}

fn with_store<T>(
    data: &Path,
    write: bool,
    f: impl FnOnce(&mut Store) -> Result<T, String>,
) -> Result<T, String> {
    let _g = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let mut s: Store = read_json(&path(data), MAX_BYTES)?;
    let out = f(&mut s)?;
    if write {
        // Never persist a sample row, whatever the caller did.
        s.proposals.retain(|p| p.sample != Some(true));
        while s.proposals.len() > MAX_PROPOSALS {
            match s
                .proposals
                .iter()
                .position(|p| p.status != WatchStatus::Proposed)
            {
                Some(i) => {
                    s.proposals.remove(i);
                }
                None => break,
            }
        }
        write_json_atomic(&path(data), &s, MAX_BYTES)?;
    }
    Ok(out)
}

/// Stored text is untrusted too: clean it again on the way out.
fn recheck(mut p: WatchProposal) -> Option<WatchProposal> {
    if !valid_ticker(&p.ticker) || p.sample == Some(true) {
        return None;
    }
    p.reason_text = clean_text(&p.reason_text, MAX_REASON);
    p.risk_text = clean_text(&p.risk_text, MAX_RISK);
    p.source_name = clean_text(&p.source_name, MAX_SOURCE);
    if p.reason_text.is_empty() || p.risk_text.is_empty() || p.source_name.is_empty() {
        return None;
    }
    p.source_url = p.source_url.as_deref().and_then(clean_url);
    p.size = p.size.as_ref().and_then(clean_size);
    if p.coin_id
        .as_deref()
        .is_some_and(|c| !super::market_feed::valid_coin_id(c))
    {
        return None;
    }
    Some(p)
}

fn samples(now: &str) -> Vec<WatchProposal> {
    let mk = |id: &str, kind, side, ticker: &str, reason: &str, risk: &str, conf| WatchProposal {
        id: id.into(),
        created_at: now.into(),
        kind,
        side,
        ticker: ticker.into(),
        coin_id: None,
        reason_text: reason.into(),
        source_name: "Sample news feed".into(),
        source_url: None,
        confidence: conf,
        risk_text: risk.into(),
        size: None,
        status: WatchStatus::Proposed,
        decided_at: None,
        sample: Some(true),
    };
    vec![
        mk(
            "sample-aurx",
            WatchKind::Crypto,
            WatchSide::Buy,
            "AURX",
            "A sample shipping-industry note stands in for a world-news reason. This headline is not real.",
            "Sample only. No size is set.",
            WatchConfidence::Medium,
        ),
        mk(
            "sample-hklo",
            WatchKind::Stock,
            WatchSide::Watch,
            "HKLO",
            "A sample regulatory brief stands in for a source. This brief is not real.",
            "Sample only. Watch only.",
            WatchConfidence::Low,
        ),
    ]
}

// ── Reads ────────────────────────────────────────────────────────────────────

/// Open proposals first (newest first), then decided ones. Samples are added only when asked.
pub(crate) fn list_in(
    data: &Path,
    include_sample: bool,
    now: DateTime<FixedOffset>,
) -> Result<WatchList, String> {
    let mut proposals = with_store(data, false, |s| Ok(s.proposals.clone()))?;
    // A hand-edited or damaged row is cleaned again, and dropped if it still fails.
    proposals = proposals.into_iter().filter_map(recheck).collect();
    proposals.sort_by(|a, b| {
        (a.status != WatchStatus::Proposed)
            .cmp(&(b.status != WatchStatus::Proposed))
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    if include_sample {
        proposals.extend(samples(&iso(&now)));
    }
    Ok(WatchList {
        proposals,
        disclaimer: NOT_ADVICE.into(),
    })
}

// ── Service-only create ──────────────────────────────────────────────────────

/// Cleans and stores one candidate as `proposed`. Returns None when it is unusable or already open for the
/// same ticker and side. SERVICE ONLY (not a command).
#[allow(dead_code)]
pub(crate) fn create_in(
    data: &Path,
    item: &FeedItem,
    now: DateTime<FixedOffset>,
) -> Result<Option<WatchProposal>, String> {
    if !valid_ticker(&item.ticker) {
        return Ok(None);
    }
    let reason = clean_text(&item.reason_text, MAX_REASON);
    let risk = clean_text(&item.risk_text, MAX_RISK);
    let source = clean_text(&item.source_name, MAX_SOURCE);
    if reason.is_empty() || risk.is_empty() || source.is_empty() {
        return Ok(None);
    }
    if item.kind == WatchKind::Crypto
        && item.coin_id.is_some()
        && !item
            .coin_id
            .as_deref()
            .is_some_and(super::market_feed::valid_coin_id)
    {
        return Ok(None);
    }
    let p = WatchProposal {
        id: new_id("wp"),
        created_at: iso(&now),
        kind: item.kind,
        side: item.side,
        ticker: item.ticker.clone(),
        coin_id: item
            .coin_id
            .clone()
            .filter(|c| super::market_feed::valid_coin_id(c)),
        reason_text: reason,
        source_name: source,
        source_url: item.source_url.as_deref().and_then(clean_url),
        confidence: item.confidence,
        risk_text: risk,
        size: item.size.as_ref().and_then(clean_size),
        status: WatchStatus::Proposed,
        decided_at: None,
        sample: None,
    };
    with_store(data, true, |s| {
        let dup = s
            .proposals
            .iter()
            .any(|x| x.status == WatchStatus::Proposed && x.ticker == p.ticker && x.side == p.side);
        if dup {
            return Ok(None);
        }
        s.proposals.insert(0, p.clone());
        Ok(Some(p))
    })
}

/// Runs a source once and stores what it gave. Returns how many were stored.
#[allow(dead_code)]
pub(crate) fn ingest_in(
    data: &Path,
    src: &dyn FeedSource,
    now: DateTime<FixedOffset>,
) -> Result<usize, String> {
    let mut n = 0;
    for item in src.items()?.iter().take(20) {
        if create_in(data, item, now)?.is_some() {
            n += 1;
        }
    }
    Ok(n)
}

// ── Taisun's two writes ──────────────────────────────────────────────────────

fn decide(
    dirs: &Dirs,
    now: DateTime<FixedOffset>,
    id: &str,
    via: Option<&str>,
    to: WatchStatus,
    text: &str,
    toast: &str,
) -> Result<WatchDecision, String> {
    if id.is_empty() || id.len() > MAX_ID {
        return Err("invalid input".into());
    }
    if id.starts_with("sample-") {
        return Err("watch: sample data can't be decided".into());
    }
    let now_s = iso(&now);
    let (hist, proposal) = adds::logged_write("watch", text.to_string(), via, dirs, now, || {
        with_store(&dirs.data, true, |s| {
            let p = s
                .proposals
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or("watch: not found")?;
            if p.sample == Some(true) {
                return Err("watch: sample data can't be decided".to_string());
            }
            if p.status != WatchStatus::Proposed {
                return Err("watch: already decided".to_string());
            }
            p.status = to;
            p.decided_at = Some(now_s.clone());
            // Accepting a watch pick puts it on the watchlist (0038). Same write, same Undo.
            if to == WatchStatus::Recorded {
                super::watchlist::add_pick_in(&dirs.data, p, now)?;
            }
            Ok((p.clone(), UndoRef::Watch { id: id.to_string() }))
        })
    })?;
    Ok(WatchDecision {
        id: hist,
        proposal,
        toast: toast.into(),
    })
}

/// Records his choice. Writes a log entry; sends nothing anywhere.
pub(crate) fn record_in(
    dirs: &Dirs,
    now: DateTime<FixedOffset>,
    id: &str,
    via: Option<&str>,
) -> Result<WatchDecision, String> {
    decide(
        dirs,
        now,
        id,
        via,
        WatchStatus::Recorded,
        "Recorded a watch-only choice",
        TOAST_RECORDED,
    )
}

pub(crate) fn dismiss_in(
    dirs: &Dirs,
    now: DateTime<FixedOffset>,
    id: &str,
    via: Option<&str>,
) -> Result<WatchDecision, String> {
    decide(
        dirs,
        now,
        id,
        via,
        WatchStatus::Dismissed,
        "Dismissed a watch-only proposal",
        TOAST_DISMISSED,
    )
}

/// Undo: the proposal is open again.
pub(crate) fn reopen_in(data: &Path, id: &str) -> Result<(), String> {
    super::watchlist::drop_for_proposal_in(data, id)?;
    with_store(data, true, |s| {
        if let Some(p) = s.proposals.iter_mut().find(|p| p.id == id) {
            if p.sample != Some(true) {
                p.status = WatchStatus::Proposed;
                p.decided_at = None;
            }
        }
        Ok(())
    })
}

// ── Commands ─────────────────────────────────────────────────────────────────

/// `include_sample` is true only when the page's sample flag is on.
#[tauri::command]
#[specta::specta]
pub async fn watch_proposals_list(include_sample: Option<bool>) -> Result<WatchList, String> {
    blocking(JoinErr::Plain, move || {
        list_in(
            &crate::app_paths::data_dir(),
            include_sample.unwrap_or(false),
            now_jst(),
        )
    })
    .await
}

/// Only after the page's second confirm. Records the choice; never sends anything.
#[tauri::command]
#[specta::specta]
pub async fn watch_proposal_record(
    id: String,
    via: Option<String>,
) -> Result<WatchDecision, String> {
    blocking(JoinErr::Plain, move || {
        record_in(&Dirs::real(), now_jst(), &id, via.as_deref())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn watch_proposal_dismiss(
    id: String,
    via: Option<String>,
) -> Result<WatchDecision, String> {
    blocking(JoinErr::Plain, move || {
        dismiss_in(&Dirs::real(), now_jst(), &id, via.as_deref())
    })
    .await
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::local_store::tests::temp_dir;

    fn dirs() -> Dirs {
        let d = temp_dir("watch");
        Dirs {
            data: d.join("data"),
            workspace: d.join("ws"),
            legacy: d.join("legacy"),
        }
    }

    fn t(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn item(ticker: &str) -> FeedItem {
        FeedItem {
            kind: WatchKind::Stock,
            side: WatchSide::Watch,
            ticker: ticker.into(),
            coin_id: None,
            reason_text: "A fixture headline.".into(),
            source_name: "Fixture".into(),
            source_url: Some("https://example.com/a".into()),
            confidence: WatchConfidence::Low,
            risk_text: "Fixture risk.".into(),
            size: None,
        }
    }

    struct Fixture(Vec<FeedItem>);
    impl FeedSource for Fixture {
        fn name(&self) -> &str {
            "fixture"
        }
        fn items(&self) -> Result<Vec<FeedItem>, String> {
            Ok(self.0.clone())
        }
    }

    const NOW: &str = "2026-11-18T10:00:00+09:00";

    fn seeded() -> (Dirs, String) {
        let d = dirs();
        let p = create_in(&d.data, &item("ABC"), t(NOW)).unwrap().unwrap();
        (d, p.id)
    }

    fn status(d: &Dirs, id: &str) -> WatchStatus {
        list_in(&d.data, false, t(NOW))
            .unwrap()
            .proposals
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .status
    }

    #[test]
    fn record_marks_recorded_returns_toast_and_writes_history() {
        let (d, id) = seeded();
        let r = record_in(&d, t(NOW), &id, Some("typed")).unwrap();
        assert_eq!(r.toast, "Recorded. Watch-only \u{2014} no order was sent.");
        assert_eq!(r.proposal.status, WatchStatus::Recorded);
        assert!(r.proposal.decided_at.is_some());
        assert_eq!(status(&d, &id), WatchStatus::Recorded);
        let h = adds::history_in("watch", &d).unwrap();
        assert_eq!(h.len(), 1);
        // A second decision is refused.
        assert!(record_in(&d, t(NOW), &id, None).is_err());
        assert!(dismiss_in(&d, t(NOW), &id, None).is_err());
    }

    #[test]
    fn dismiss_and_undo_restore_proposed() {
        let (d, id) = seeded();
        let r = dismiss_in(&d, t(NOW), &id, Some("voice")).unwrap();
        assert_eq!(status(&d, &id), WatchStatus::Dismissed);
        assert!(adds::undo_in("watch", &r.id, &d, t("2026-11-18T10:00:05+09:00")).unwrap());
        assert_eq!(status(&d, &id), WatchStatus::Proposed);
        let r = record_in(&d, t(NOW), &id, None).unwrap();
        // Too late: nothing changes.
        assert!(!adds::undo_in("watch", &r.id, &d, t("2026-11-18T10:00:30+09:00")).unwrap());
        assert_eq!(status(&d, &id), WatchStatus::Recorded);
    }

    #[test]
    fn undo_of_record_restores_proposed_and_clears_decided_at() {
        let (d, id) = seeded();
        let r = record_in(&d, t(NOW), &id, None).unwrap();
        assert!(adds::undo_in("watch", &r.id, &d, t("2026-11-18T10:00:03+09:00")).unwrap());
        let l = list_in(&d.data, false, t(NOW)).unwrap();
        assert_eq!(l.proposals[0].status, WatchStatus::Proposed);
        assert!(l.proposals[0].decided_at.is_none());
    }

    #[test]
    fn sample_is_refused_and_never_stored() {
        let d = dirs();
        let l = list_in(&d.data, true, t(NOW)).unwrap();
        assert_eq!(l.proposals.len(), 2);
        assert!(l.proposals.iter().all(|p| p.sample == Some(true)));
        assert!(list_in(&d.data, false, t(NOW))
            .unwrap()
            .proposals
            .is_empty());
        for p in &l.proposals {
            assert!(record_in(&d, t(NOW), &p.id, None).is_err());
            assert!(dismiss_in(&d, t(NOW), &p.id, None).is_err());
        }
        // A stored row tagged sample is refused too, and a write drops it.
        let mut raw = Store::default();
        let mut p = create_in(&d.data, &item("ZZZ"), t(NOW)).unwrap().unwrap();
        p.sample = Some(true);
        p.id = "wp-stored".into();
        raw.proposals.push(p);
        write_json_atomic(&path(&d.data), &raw, MAX_BYTES).unwrap();
        assert!(record_in(&d, t(NOW), "wp-stored", None).is_err());
        assert!(adds::history_in("watch", &d).unwrap().is_empty());
    }

    #[test]
    fn unknown_id_and_bad_via_are_refused() {
        let (d, id) = seeded();
        assert!(record_in(&d, t(NOW), "wp-nope", None).is_err());
        assert!(record_in(&d, t(NOW), &id, Some("robot")).is_err());
        assert_eq!(status(&d, &id), WatchStatus::Proposed);
    }

    #[test]
    fn untrusted_text_is_cleaned_and_capped() {
        let d = dirs();
        let mut bad = item("ABC");
        bad.reason_text = format!("a\u{202E}b\n\tc {}", "x".repeat(1000));
        bad.risk_text = "r".repeat(1000);
        bad.source_name = "s".repeat(500);
        let p = create_in(&d.data, &bad, t(NOW)).unwrap().unwrap();
        assert!(p.reason_text.chars().count() <= MAX_REASON);
        assert!(p.reason_text.starts_with("ab c "));
        assert!(!p.reason_text.contains('\u{202E}'));
        assert_eq!(p.risk_text.chars().count(), MAX_RISK);
        assert_eq!(p.source_name.chars().count(), MAX_SOURCE);
        for t_ in ["abc", "A B", "", "TOOLONGTICKER1", "A/B", "A;B", "ÀBC"] {
            assert!(
                create_in(&d.data, &item(t_), t(NOW)).unwrap().is_none(),
                "{t_}"
            );
        }
        for ok in ["BTC", "BRK.B", "A-B", "X1"] {
            assert!(valid_ticker(ok));
        }
        let mut empty = item("DEF");
        empty.reason_text = " \u{200B} ".into();
        assert!(create_in(&d.data, &empty, t(NOW)).unwrap().is_none());
    }

    #[test]
    fn source_url_is_https_only() {
        assert!(clean_url("https://example.com/a?b=1").is_some());
        for bad in [
            "http://example.com",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://user:pw@example.com",
            "https://",
            "https://exa mple.com",
            "https://example.com/\n",
            "//example.com",
            "https://\u{e9}.com",
        ] {
            assert!(clean_url(bad).is_none(), "{bad}");
        }
        let d = dirs();
        let mut i = item("ABC");
        i.source_url = Some("http://example.com".into());
        assert!(create_in(&d.data, &i, t(NOW))
            .unwrap()
            .unwrap()
            .source_url
            .is_none());
    }

    #[test]
    fn stored_rows_are_rechecked_and_long_ids_refused() {
        let (d, id) = seeded();
        let mut raw: Store = read_json(&path(&d.data), MAX_BYTES).unwrap();
        let mut bad = raw.proposals[0].clone();
        bad.id = "wp-bad".into();
        bad.ticker = "no good".into();
        let mut dirty = raw.proposals[0].clone();
        dirty.id = "wp-dirty".into();
        dirty.reason_text = "a\u{202E}b".into();
        dirty.source_url = Some("http://x.com".into());
        raw.proposals.push(bad);
        raw.proposals.push(dirty);
        write_json_atomic(&path(&d.data), &raw, MAX_BYTES).unwrap();
        let l = list_in(&d.data, false, t(NOW)).unwrap();
        assert_eq!(l.proposals.len(), 2);
        let q = l.proposals.iter().find(|p| p.id == "wp-dirty").unwrap();
        assert_eq!(q.reason_text, "ab");
        assert!(q.source_url.is_none());
        let long = "x".repeat(65);
        assert!(record_in(&d, t(NOW), &long, None).is_err());
        assert!(dismiss_in(&d, t(NOW), "", None).is_err());
        assert_eq!(status(&d, &id), WatchStatus::Proposed);
    }

    #[test]
    fn size_is_kept_only_when_sane() {
        let d = dirs();
        let mut i = item("AAA");
        i.size = Some(WatchSize {
            amount: 5.0,
            currency: "USD".into(),
        });
        assert!(create_in(&d.data, &i, t(NOW))
            .unwrap()
            .unwrap()
            .size
            .is_some());
        for (a, c) in [
            (f64::NAN, "USD"),
            (-1.0, "USD"),
            (0.0, "USD"),
            (1.0, "usd"),
            (1.0, "US"),
        ] {
            let mut i = item("BBB");
            i.size = Some(WatchSize {
                amount: a,
                currency: c.into(),
            });
            assert!(create_in(&d.data, &i, t(NOW))
                .unwrap()
                .unwrap()
                .size
                .is_none());
            reopen_in(&d.data, "x").unwrap();
            with_store(&d.data, true, |s| {
                s.proposals.retain(|p| p.ticker != "BBB");
                Ok(())
            })
            .unwrap();
        }
    }

    #[test]
    fn fixture_feed_ingests_and_dedups() {
        let d = dirs();
        let f = Fixture(vec![
            item("AAA"),
            item("AAA"),
            item("bad ticker"),
            item("BBB"),
        ]);
        assert_eq!(f.name(), "fixture");
        assert_eq!(ingest_in(&d.data, &f, t(NOW)).unwrap(), 2);
        let l = list_in(&d.data, false, t(NOW)).unwrap();
        assert_eq!(l.proposals.len(), 2);
        assert!(l
            .proposals
            .iter()
            .all(|p| p.status == WatchStatus::Proposed && p.sample.is_none()));
        assert_eq!(l.disclaimer, "Not financial advice");
    }

    #[test]
    fn the_store_holds_no_keys_or_credentials() {
        let (d, _) = seeded();
        let text = std::fs::read_to_string(path(&d.data)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let keys: Vec<String> = v["proposals"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for k in &keys {
            let k = k.to_lowercase();
            for w in [
                "key",
                "secret",
                "token",
                "seed",
                "password",
                "account",
                "credential",
            ] {
                assert!(!k.contains(w), "{k}");
            }
        }
    }

    #[test]
    fn proposal_serializes_camel_case_with_lowercase_enums() {
        let (d, _) = seeded();
        let p = &list_in(&d.data, false, t(NOW)).unwrap().proposals[0];
        let v = serde_json::to_value(p).unwrap();
        assert_eq!(v["kind"], "stock");
        assert_eq!(v["side"], "watch");
        assert_eq!(v["status"], "proposed");
        assert!(v.get("reasonText").is_some() && v.get("sourceName").is_some());
        assert!(v.get("sample").is_none());
    }

    #[test]
    fn module_has_no_network_process_or_money_code() {
        let src = include_str!("watch_proposals.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        let code: String = code
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        for w in [
            "reqwest",
            "hyper",
            "ureq",
            "tcpstream",
            "udpsocket",
            "tokio::net",
            "websocket",
            "command::new",
            "std::process",
            "broker",
            "exchange",
            "wallet",
            "mnemonic",
            "private_key",
            "api_key",
            "apikey",
            "keychain",
            "place_order",
            "submit",
            "std::env",
            "loopback_http",
            "outbound",
        ] {
            assert!(!code.contains(w), "watch_proposals.rs must not contain {w}");
        }
    }

    #[test]
    fn command_names_pass_the_money_word_guard() {
        let banned = [
            "trade",
            "order",
            "transfer",
            "pay",
            "convert",
            "withdraw",
            "send",
            "buy",
            "sell",
            "move",
            "wire",
            "remit",
            "swap",
            "sign",
            "broadcast",
            "deposit",
            "exchange",
        ];
        for n in [
            "watch_proposals_list",
            "watch_proposal_record",
            "watch_proposal_dismiss",
        ] {
            for b in banned {
                assert!(!n.contains(b), "{n} contains {b}");
            }
        }
    }
}
