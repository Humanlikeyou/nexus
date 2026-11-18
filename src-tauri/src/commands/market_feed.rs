//! Watchlist price feed (decision 0038 #5, Taisun 2026-11-19). The only code that talks to the price hosts.
//!
//! Hosts (read-only, keyless, `host:port` pinned, https only): `api.coingecko.com:443` (crypto simple price),
//! `stooq.com:443` (stock CSV quote) and `api.gdeltproject.org:443` (headlines for Amara's research, used by
//! `watchlist.rs` through the same pinned client). Any other host is refused before a connection is made.
//!
//! Rules this file keeps (each has a test):
//! - Every call goes through `Pinned::get`. It answers `Blocked` BEFORE any request when
//!   `telegram::outbound_disabled()` is true (env kill switch, navy/local-only, or Settings > Outbound off).
//!   `Pinned::production()` reads that gate on every construction, and the scheduler builds one per poll.
//! - https only, no redirects (a 3xx is refused, never followed), connect and total timeouts, a hard cap on
//!   the response body, no credentials. The scan test refuses key words in this file.
//! - Parsing is strict. A price that is missing, not a finite number, zero or negative is `None`, never `0`
//!   and never a guess. The last good price stays cached with its `fetchedAt`; nothing is ever invented.
//! - At most one poll per 15 minutes (`POLL`), only while the app runs (`start()` is called from `lib.rs`
//!   `setup` only; no test starts it). Status is `open`, `closed` or `stale` (stale when the cached price is
//!   older than 2x the poll interval, or there is no price yet).
//! - Watch-only: nothing here knows about accounts, orders or keys.
//!
//! Never logs tickers, prices or response bodies.

use super::local_store::{iso, now_jst, read_json, write_json_atomic};
use super::shared::http::{self, HttpSpec};
use super::shared::task::{blocking, JoinErr};
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// The three hosts decision 0038 approved, as `host:port`. Nothing else is reachable from this module.
pub(crate) const COINGECKO: &str = "api.coingecko.com:443";
pub(crate) const STOOQ: &str = "stooq.com:443";
pub(crate) const GDELT: &str = "api.gdeltproject.org:443";
pub(crate) const PROD_HOSTS: &[&str] = &[COINGECKO, STOOQ, GDELT];

pub(crate) const COINGECKO_BASE: &str = "https://api.coingecko.com";
pub(crate) const STOOQ_BASE: &str = "https://stooq.com";
pub(crate) const GDELT_BASE: &str = "https://api.gdeltproject.org";

/// Poll interval, in minutes: at most one poll this often.
pub const POLL_MINUTES: i64 = 15;
const POLL: ChronoDuration = ChronoDuration::minutes(POLL_MINUTES);
/// A cached price older than this reads as `stale`.
const STALE_AFTER: ChronoDuration = ChronoDuration::minutes(2 * POLL_MINUTES);
const TICK: Duration = Duration::from_secs(5 * 60);
const FIRST_DELAY: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(15);
const CAP_PRICE: usize = 64 * 1024;
const CAP_CSV: usize = 4 * 1024;
const FILE: &str = "prices.json";
const MAX_BYTES: u64 = 256 * 1024;
const MAX_QUOTES: usize = 40;
/// A stock quote whose own time is older than this (under the most generous time-zone reading) is closed.
const QUOTE_FRESH_MIN: i64 = 45;

static STORE: Mutex<()> = Mutex::new(());

// ── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeedErr {
    Blocked,
    HostNotAllowed,
    Redirect,
    Status,
    TooLarge,
    Network,
    Parse,
    Unavailable,
}

impl FeedErr {
    pub(crate) fn code(self) -> &'static str {
        match self {
            FeedErr::Blocked => "blocked",
            FeedErr::HostNotAllowed => "host_not_allowed",
            FeedErr::Redirect => "redirect_refused",
            FeedErr::Status => "bad_status",
            FeedErr::TooLarge => "too_large",
            FeedErr::Network => "network",
            FeedErr::Parse => "unparseable",
            FeedErr::Unavailable => "unavailable",
        }
    }
}

// ── The pinned client ────────────────────────────────────────────────────────

/// An https client that may reach only the listed `host:port` values. `blocked` is the outbound gate,
/// read when the client is made.
pub(crate) struct Pinned {
    allowed: Vec<String>,
    https_only: bool,
    /// The outbound gate, asked before EVERY request (true = closed). Production: `telegram::outbound_disabled`.
    gate: fn() -> bool,
    client: reqwest::Client,
}

impl Pinned {
    /// The production client: the three approved hosts, https only, environment proxies ignored, and the
    /// outbound gate asked again before every request.
    pub(crate) fn production() -> Result<Self, FeedErr> {
        Self::build(
            PROD_HOSTS.iter().map(|h| h.to_string()).collect(),
            production_spec(),
            super::telegram::outbound_disabled,
        )
    }

    fn build(allowed: Vec<String>, spec: HttpSpec, gate: fn() -> bool) -> Result<Self, FeedErr> {
        let client = http::builder(spec)
            .user_agent("AmaraOS-watchlist/1")
            .build()
            .map_err(|_| FeedErr::Network)?;
        Ok(Pinned {
            allowed,
            https_only: spec.https_only,
            gate,
            client,
        })
    }

    /// True when the outbound gate is closed right now.
    pub(crate) fn is_blocked(&self) -> bool {
        (self.gate)()
    }

    /// A plain-http client for exactly one loopback `host:port`, for tests only.
    #[cfg(test)]
    pub(crate) fn test_loopback(addr: &str, blocked: bool) -> Pinned {
        fn closed() -> bool {
            true
        }
        fn open() -> bool {
            false
        }
        Pinned::test_loopback_gate(addr, if blocked { closed } else { open })
    }

    #[cfg(test)]
    pub(crate) fn test_loopback_gate(addr: &str, gate: fn() -> bool) -> Pinned {
        let mut spec = HttpSpec::loopback().timeout(Duration::from_secs(5));
        spec.https_only = false;
        Pinned::build(vec![addr.to_string()], spec, gate).unwrap()
    }

    /// Fetches one URL. Checks, in order: the outbound gate, the scheme, no user info, the exact
    /// `host:port`. Only then connects. A redirect, a non-200 answer or a body over `cap` is an error.
    pub(crate) async fn get(&self, url: &str, cap: usize) -> Result<Vec<u8>, FeedErr> {
        if self.is_blocked() {
            return Err(FeedErr::Blocked);
        }
        let u = url::Url::parse(url).map_err(|_| FeedErr::HostNotAllowed)?;
        let scheme_ok = u.scheme() == "https" || (!self.https_only && u.scheme() == "http");
        let host = u.host_str().ok_or(FeedErr::HostNotAllowed)?;
        let port = u.port_or_known_default().ok_or(FeedErr::HostNotAllowed)?;
        let hp = format!("{host}:{port}");
        if !scheme_ok
            || !u.username().is_empty()
            || u.password().is_some()
            || !self.allowed.iter().any(|a| a.eq_ignore_ascii_case(&hp))
        {
            return Err(FeedErr::HostNotAllowed);
        }
        let mut resp = self
            .client
            .get(u)
            .send()
            .await
            .map_err(|_| FeedErr::Network)?;
        let st = resp.status();
        if st.is_redirection() {
            return Err(FeedErr::Redirect);
        }
        if !st.is_success() {
            return Err(FeedErr::Status);
        }
        if resp.content_length().is_some_and(|n| n > cap as u64) {
            return Err(FeedErr::TooLarge);
        }
        let mut out: Vec<u8> = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|_| FeedErr::Network)? {
            if out.len() + chunk.len() > cap {
                return Err(FeedErr::TooLarge);
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }
}

/// The production client settings: https only, no redirects, environment proxies ignored.
fn production_spec() -> HttpSpec {
    HttpSpec {
        timeout: Some(TOTAL_TIMEOUT),
        connect_timeout: Some(CONNECT_TIMEOUT),
        no_proxy: true,
        no_redirects: true,
        https_only: true,
        launch_token: false,
    }
}

/// Where each service lives. Production uses the three constants; tests point at a loopback server.
pub(crate) struct Bases {
    pub coingecko: String,
    pub stooq: String,
}

impl Bases {
    pub(crate) fn production() -> Self {
        Bases {
            coingecko: COINGECKO_BASE.into(),
            stooq: STOOQ_BASE.into(),
        }
    }
}

// ── Pure parsing ─────────────────────────────────────────────────────────────

pub(crate) fn valid_coin_id(s: &str) -> bool {
    let ok_chars = s
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    (1..=64).contains(&s.len())
        && ok_chars
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

/// A US stock symbol: 1 to 5 capital letters, optionally one `-X` class suffix (`BRK-B`).
pub(crate) fn valid_stock_ticker(s: &str) -> bool {
    let (head, tail) = match s.split_once('-') {
        Some((h, t)) => (h, Some(t)),
        None => (s, None),
    };
    (1..=5).contains(&head.len())
        && head.bytes().all(|b| b.is_ascii_uppercase())
        && tail.is_none_or(|t| t.len() == 1 && t.bytes().all(|b| b.is_ascii_uppercase()))
}

fn good_price(p: f64) -> Option<f64> {
    (p.is_finite() && p > 0.0 && p < 1.0e12).then_some(p)
}

/// CoinGecko `simple/price`: `{"bitcoin":{"usd":1.5}}`. A missing coin is `Unavailable`, a wrong type or a
/// bad number is `Parse`. Never `0`.
pub(crate) fn parse_coingecko(
    body: &[u8],
    ids: &[String],
) -> BTreeMap<String, Result<f64, FeedErr>> {
    let root: Option<serde_json::Value> = serde_json::from_slice(body).ok();
    ids.iter()
        .map(|id| {
            let r = match root.as_ref().and_then(|v| v.as_object()) {
                None => Err(FeedErr::Parse),
                Some(o) => match o.get(id) {
                    None => Err(FeedErr::Unavailable),
                    Some(c) => match c.get("usd") {
                        None => Err(FeedErr::Unavailable),
                        Some(p) => p.as_f64().and_then(good_price).ok_or(FeedErr::Parse),
                    },
                },
            };
            (id.clone(), r)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StooqQuote {
    pub price: f64,
    pub date: NaiveDate,
    pub time: NaiveTime,
}

/// Stooq CSV (`f=sd2t2c&h&e=csv`): header `Symbol,Date,Time,Close` and exactly one row for `want`
/// (`AAPL.US`). `N/D` anywhere is `Unavailable`; anything else off-shape is `Parse`.
pub(crate) fn parse_stooq(body: &[u8], want: &str) -> Result<StooqQuote, FeedErr> {
    let text = std::str::from_utf8(body).map_err(|_| FeedErr::Parse)?;
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() != 2 || lines[0] != "Symbol,Date,Time,Close" {
        return Err(FeedErr::Parse);
    }
    let cols: Vec<&str> = lines[1].split(',').map(str::trim).collect();
    if cols.len() != 4 || !cols[0].eq_ignore_ascii_case(want) {
        return Err(FeedErr::Parse);
    }
    if cols[1..].iter().any(|c| c.eq_ignore_ascii_case("N/D")) {
        return Err(FeedErr::Unavailable);
    }
    let date = NaiveDate::parse_from_str(cols[1], "%Y-%m-%d").map_err(|_| FeedErr::Parse)?;
    let time = NaiveTime::parse_from_str(cols[2], "%H:%M:%S").map_err(|_| FeedErr::Parse)?;
    let price = cols[3]
        .parse::<f64>()
        .ok()
        .and_then(good_price)
        .ok_or(FeedErr::Parse)?;
    Ok(StooqQuote { price, date, time })
}

/// Market closed as the quote itself reports it: no trade today, or the last trade is older than
/// `QUOTE_FRESH_MIN` minutes. Stooq stamps Central European time (UTC+1 or +2); the later reading (UTC+1)
/// is used so a quote is never called closed too early.
pub(crate) fn stooq_closed(q: &StooqQuote, now: DateTime<FixedOffset>) -> bool {
    let now_utc = now.with_timezone(&Utc).naive_utc();
    let quote_utc = q.date.and_time(q.time) - ChronoDuration::hours(1);
    now_utc - quote_utc > ChronoDuration::minutes(QUOTE_FRESH_MIN)
}

// ── Cache ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Good {
    price: f64,
    currency: String,
    fetched_at: String,
    closed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Slot {
    #[serde(default)]
    good: Option<Good>,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PriceStore {
    #[serde(default)]
    quotes: BTreeMap<String, Slot>,
    #[serde(default)]
    last_attempt_at: Option<String>,
    #[serde(default)]
    last_ok_at: Option<String>,
}

fn path(data: &Path) -> PathBuf {
    data.join("watch").join(FILE)
}

fn with_store<T>(
    data: &Path,
    write: bool,
    f: impl FnOnce(&mut PriceStore) -> T,
) -> Result<T, String> {
    let _g = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let mut s: PriceStore = read_json(&path(data), MAX_BYTES)?;
    let out = f(&mut s);
    if write {
        write_json_atomic(&path(data), &s, MAX_BYTES)?;
    }
    Ok(out)
}

fn parse_ts(s: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s).ok()
}

/// What a pick needs priced.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PickRef {
    pub crypto: bool,
    pub ticker: String,
    pub coin_id: Option<String>,
}

pub(crate) fn slot_key(p: &PickRef) -> String {
    match (&p.coin_id, p.crypto) {
        (Some(c), true) => format!("crypto:{c}"),
        _ => format!("stock:{}", p.ticker),
    }
}

// ── Views ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum QuoteStatus {
    Open,
    Closed,
    Stale,
}

/// The latest price for one pick. `price` is `null` when nothing good was ever fetched. `status` is
/// `stale` when the price is older than 2x the poll interval (or there is none).
#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub price: Option<f64>,
    pub currency: Option<String>,
    pub fetched_at: Option<String>,
    pub status: QuoteStatus,
    /// Why the last fetch gave nothing (fixed code), if it did.
    pub error: Option<String>,
}

fn quote_of(slot: Option<&Slot>, now: DateTime<FixedOffset>) -> Quote {
    let Some(s) = slot else {
        return Quote {
            price: None,
            currency: None,
            fetched_at: None,
            status: QuoteStatus::Stale,
            error: None,
        };
    };
    match &s.good {
        None => Quote {
            price: None,
            currency: None,
            fetched_at: None,
            status: QuoteStatus::Stale,
            error: s.last_error.clone(),
        },
        Some(g) => {
            let fresh = parse_ts(&g.fetched_at).is_some_and(|t| now - t <= STALE_AFTER);
            let status = if !fresh {
                QuoteStatus::Stale
            } else if g.closed {
                QuoteStatus::Closed
            } else {
                QuoteStatus::Open
            };
            Quote {
                price: Some(g.price),
                currency: Some(g.currency.clone()),
                fetched_at: Some(g.fetched_at.clone()),
                status,
                error: s.last_error.clone(),
            }
        }
    }
}

/// The cached quote for each pick (same order). Reads only.
pub(crate) fn quotes_in(
    data: &Path,
    picks: &[PickRef],
    now: DateTime<FixedOffset>,
) -> Result<Vec<Quote>, String> {
    with_store(data, false, |s| {
        picks
            .iter()
            .map(|p| quote_of(s.quotes.get(&slot_key(p)), now))
            .collect()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FeedState {
    /// No fetch has been tried yet.
    NotConnected,
    /// The outbound gate is closed: nothing is being fetched.
    Blocked,
    /// The last good fetch is within 2x the poll interval.
    Connected,
    /// The last good fetch is older than that.
    Stale,
    /// Tried, and nothing good has ever come back.
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MarketFeedStatus {
    pub state: FeedState,
    pub last_attempt_at: Option<String>,
    pub last_ok_at: Option<String>,
    pub poll_minutes: u32,
}

pub(crate) fn status_in(
    data: &Path,
    blocked: bool,
    now: DateTime<FixedOffset>,
) -> Result<MarketFeedStatus, String> {
    let (attempt, ok) = with_store(data, false, |s| {
        (s.last_attempt_at.clone(), s.last_ok_at.clone())
    })?;
    let state = if blocked {
        FeedState::Blocked
    } else {
        match (&attempt, ok.as_deref().and_then(parse_ts)) {
            (None, None) => FeedState::NotConnected,
            (Some(_), None) => FeedState::Error,
            (_, Some(t)) if now - t > STALE_AFTER => FeedState::Stale,
            (_, Some(_)) => FeedState::Connected,
        }
    };
    Ok(MarketFeedStatus {
        state,
        last_attempt_at: attempt,
        last_ok_at: ok,
        poll_minutes: POLL_MINUTES as u32,
    })
}

// ── Polling ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PollOutcome {
    /// The outbound gate is closed; no request was made and nothing was written.
    Blocked,
    /// Nothing on the watchlist.
    Idle,
    /// The last attempt was under 15 minutes ago.
    TooSoon,
    /// A poll ran: how many prices came back good.
    Ran { good: usize, wanted: usize },
}

fn record(
    s: &mut PriceStore,
    key: String,
    r: Result<(f64, bool), FeedErr>,
    currency: &str,
    now: DateTime<FixedOffset>,
) -> bool {
    let slot = s.quotes.entry(key).or_default();
    match r {
        Ok((price, closed)) => {
            slot.good = Some(Good {
                price,
                currency: currency.into(),
                fetched_at: iso(&now),
                closed,
            });
            slot.last_error = None;
            true
        }
        Err(e) => {
            // The last good price stays; only the reason changes.
            slot.last_error = Some(e.code().into());
            false
        }
    }
}

/// One fetched price: slot key, (price, market closed) or the failure, currency.
type Fetched = (String, Result<(f64, bool), FeedErr>, &'static str);

/// One poll: a single CoinGecko call for all crypto picks, then one Stooq call per stock pick. Returns
/// `Blocked` before any request when the gate is closed, and `TooSoon` inside the 15-minute window.
pub(crate) async fn poll_in(
    data: &Path,
    client: &Pinned,
    bases: &Bases,
    picks: &[PickRef],
    now: DateTime<FixedOffset>,
) -> Result<PollOutcome, String> {
    if client.is_blocked() {
        return Ok(PollOutcome::Blocked);
    }
    let picks: Vec<PickRef> = picks.iter().take(MAX_QUOTES).cloned().collect();
    if picks.is_empty() {
        return Ok(PollOutcome::Idle);
    }
    let last = with_store(data, false, |s| s.last_attempt_at.clone())?;
    if last
        .as_deref()
        .and_then(parse_ts)
        .is_some_and(|t| now - t < POLL)
    {
        return Ok(PollOutcome::TooSoon);
    }

    let mut results: Vec<Fetched> = Vec::new();
    let coins: Vec<String> = picks
        .iter()
        .filter(|p| p.crypto)
        .filter_map(|p| p.coin_id.clone())
        .filter(|c| valid_coin_id(c))
        .collect();
    if !coins.is_empty() {
        let url = format!(
            "{}/api/v3/simple/price?ids={}&vs_currencies=usd",
            bases.coingecko,
            coins.join(",")
        );
        match client.get(&url, CAP_PRICE).await {
            Ok(body) => {
                for (id, r) in parse_coingecko(&body, &coins) {
                    results.push((format!("crypto:{id}"), r.map(|p| (p, false)), "USD"));
                }
            }
            Err(e) => {
                for id in &coins {
                    results.push((format!("crypto:{id}"), Err(e), "USD"));
                }
            }
        }
    }
    for p in picks.iter().filter(|p| !p.crypto) {
        if !valid_stock_ticker(&p.ticker) {
            continue;
        }
        let sym = format!("{}.us", p.ticker.to_lowercase());
        let url = format!("{}/q/l/?s={sym}&f=sd2t2c&h&e=csv", bases.stooq);
        let r = match client.get(&url, CAP_CSV).await {
            Ok(body) => parse_stooq(&body, &sym).map(|q| (q.price, stooq_closed(&q, now))),
            Err(e) => Err(e),
        };
        results.push((format!("stock:{}", p.ticker), r, "USD"));
    }

    let wanted = results.len();
    let good = with_store(data, true, |s| {
        let mut good = 0;
        for (key, r, cur) in results {
            if record(s, key, r, cur, now) {
                good += 1;
            }
        }
        s.last_attempt_at = Some(iso(&now));
        if good > 0 {
            s.last_ok_at = Some(iso(&now));
        }
        // Keep only quotes for what is on the list now.
        let live: Vec<String> = picks.iter().map(slot_key).collect();
        s.quotes.retain(|k, _| live.contains(k));
        good
    })?;
    Ok(PollOutcome::Ran { good, wanted })
}

async fn poll_tick() {
    let data = crate::app_paths::data_dir();
    let d = data.clone();
    let Ok(picks) = blocking(JoinErr::Plain, move || super::watchlist::pick_refs_in(&d)).await
    else {
        return;
    };
    let Ok(client) = Pinned::production() else {
        return;
    };
    let _ = poll_in(&data, &client, &Bases::production(), &picks, now_jst()).await;
}

/// Starts the poller. Called once from the app's `setup`; nothing runs while the app is closed.
pub fn start() {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        loop {
            poll_tick().await;
            tokio::time::sleep(TICK).await;
        }
    });
}

// ── Command ──────────────────────────────────────────────────────────────────

/// The real feed state. Reads only; never makes a request.
#[tauri::command]
#[specta::specta]
pub async fn market_feed_status() -> Result<MarketFeedStatus, String> {
    let blocked = super::telegram::outbound_disabled();
    blocking(JoinErr::Plain, move || {
        status_in(&crate::app_paths::data_dir(), blocked, now_jst())
    })
    .await
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::local_store::tests::temp_dir;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn t(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }
    const NOW: &str = "2026-11-19T12:00:00+09:00";

    /// A loopback test client: plain http to exactly `addr`, never the real hosts.
    fn test_client(addr: &str, blocked: bool) -> Pinned {
        Pinned::test_loopback(addr, blocked)
    }

    /// Serves `answers` (raw HTTP text) one per connection, recording each request line.
    async fn serve(answers: Vec<String>) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let hits = Arc::new(AtomicUsize::new(0));
        let lines = Arc::new(Mutex::new(Vec::new()));
        let (h, ln) = (hits.clone(), lines.clone());
        tokio::spawn(async move {
            let mut i = 0;
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    break;
                };
                h.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let first = String::from_utf8_lossy(&buf[..n])
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                ln.lock().unwrap().push(first);
                let a = answers
                    .get(i)
                    .or(answers.last())
                    .cloned()
                    .unwrap_or_default();
                i += 1;
                let _ = s.write_all(a.as_bytes()).await;
                let _ = s.shutdown().await;
            }
        });
        (addr, hits, lines)
    }

    fn ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn bases(addr: &str) -> Bases {
        Bases {
            coingecko: format!("http://{addr}"),
            stooq: format!("http://{addr}"),
        }
    }

    fn crypto(id: &str, sym: &str) -> PickRef {
        PickRef {
            crypto: true,
            ticker: sym.into(),
            coin_id: Some(id.into()),
        }
    }
    fn stock(sym: &str) -> PickRef {
        PickRef {
            crypto: false,
            ticker: sym.into(),
            coin_id: None,
        }
    }

    #[test]
    fn the_three_hosts_are_pinned_exactly() {
        assert_eq!(
            PROD_HOSTS,
            [
                "api.coingecko.com:443",
                "stooq.com:443",
                "api.gdeltproject.org:443"
            ]
        );
        assert!(COINGECKO_BASE.starts_with("https://") && STOOQ_BASE.starts_with("https://"));
        assert!(GDELT_BASE.starts_with("https://"));
        let spec_src = include_str!("market_feed.rs");
        let prod = &spec_src[spec_src
            .find("pub(crate) fn production() -> Result")
            .unwrap()..];
        let prod = &prod[..prod.find("fn build(").unwrap()];
        let ps = &spec_src[spec_src.find("fn production_spec()").unwrap()..];
        let ps = &ps[..ps.find("\n}\n").unwrap()];
        assert!(ps.contains("https_only: true") && ps.contains("no_redirects: true"));
        assert!(ps.contains("no_proxy: true"));
        assert!(prod.contains("production_spec()"));
        assert!(prod.contains("super::telegram::outbound_disabled,"));
    }

    #[tokio::test]
    async fn blocked_makes_no_request_at_all() {
        let (addr, hits, _) = serve(vec![ok("{}")]).await;
        let c = test_client(&addr, true);
        let r = c.get(&format!("http://{addr}/x"), 100).await;
        assert_eq!(r, Err(FeedErr::Blocked));
        let d = temp_dir("mf-blocked");
        let out = poll_in(&d, &c, &bases(&addr), &[crypto("bitcoin", "BTC")], t(NOW))
            .await
            .unwrap();
        assert_eq!(out, PollOutcome::Blocked);
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        // Nothing was written either: the gate opening later polls at once.
        assert!(!path(&d).exists());
    }

    #[tokio::test]
    async fn other_hosts_schemes_and_user_info_are_refused_before_connecting() {
        let (addr, hits, _) = serve(vec![ok("{}")]).await;
        let c = test_client(&addr, false);
        let port = addr.rsplit(':').next().unwrap();
        for u in [
            format!("http://localhost:{port}/x"),
            format!("http://127.0.0.1:{}/x", port.parse::<u16>().unwrap() + 1),
            format!("http://user:pw@{addr}/x"),
            "http://example.com/x".to_string(),
            "not a url".to_string(),
        ] {
            assert_eq!(c.get(&u, 100).await, Err(FeedErr::HostNotAllowed), "{u}");
        }
        // Production clients are https only, so plain http to a pinned host is refused too.
        let prod = Pinned::build(
            PROD_HOSTS.iter().map(|h| h.to_string()).collect(),
            HttpSpec {
                https_only: true,
                ..HttpSpec::loopback()
            },
            || false,
        )
        .unwrap();
        for u in [
            "http://api.coingecko.com/api",
            "https://evil.example/x",
            "https://api.coingecko.com.evil.example/x",
            "https://api.coingecko.com:8443/x",
            "https://stooq.com@evil.example/x",
        ] {
            assert_eq!(prod.get(u, 100).await, Err(FeedErr::HostNotAllowed), "{u}");
        }
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn redirects_status_and_size_caps_are_errors() {
        let redirect = "HTTP/1.1 302 Found\r\nLocation: http://example.com/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string();
        let (addr, hits, _) = serve(vec![redirect]).await;
        let c = test_client(&addr, false);
        assert_eq!(
            c.get(&format!("http://{addr}/a"), 100).await,
            Err(FeedErr::Redirect)
        );
        // The redirect target was never contacted: one connection only.
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let (addr, _, _) = serve(vec![
            "HTTP/1.1 500 Err\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        ])
        .await;
        let c = test_client(&addr, false);
        assert_eq!(
            c.get(&format!("http://{addr}/a"), 100).await,
            Err(FeedErr::Status)
        );
        let (addr, _, _) = serve(vec![ok(&"x".repeat(500))]).await;
        let c = test_client(&addr, false);
        assert_eq!(
            c.get(&format!("http://{addr}/a"), 100).await,
            Err(FeedErr::TooLarge)
        );
        // No Content-Length: the streamed size is capped too.
        let (addr, _, _) = serve(vec![format!(
            "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}",
            "x".repeat(500)
        )])
        .await;
        let c = test_client(&addr, false);
        assert_eq!(
            c.get(&format!("http://{addr}/a"), 100).await,
            Err(FeedErr::TooLarge)
        );
        let (addr, _, _) = serve(vec![ok("fine")]).await;
        let c = test_client(&addr, false);
        assert_eq!(
            c.get(&format!("http://{addr}/a"), 100).await.unwrap(),
            b"fine"
        );
    }

    #[test]
    fn coingecko_parse_gives_null_never_zero() {
        let ids: Vec<String> = [
            "bitcoin", "ethereum", "ghost", "zero", "neg", "text", "huge",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let body = br#"{"bitcoin":{"usd":67000.5},"ethereum":{},"zero":{"usd":0},"neg":{"usd":-3},"text":{"usd":"12"},"huge":{"usd":1e300}}"#;
        let r = parse_coingecko(body, &ids);
        assert_eq!(r["bitcoin"], Ok(67000.5));
        assert_eq!(r["ethereum"], Err(FeedErr::Unavailable));
        assert_eq!(r["ghost"], Err(FeedErr::Unavailable));
        assert_eq!(r["zero"], Err(FeedErr::Parse));
        assert_eq!(r["neg"], Err(FeedErr::Parse));
        assert_eq!(r["text"], Err(FeedErr::Parse));
        assert_eq!(r["huge"], Err(FeedErr::Parse));
        for body in [&b"not json"[..], b"[]", b"", b"null"] {
            let r = parse_coingecko(body, &ids[..1]);
            assert_eq!(r["bitcoin"], Err(FeedErr::Parse));
        }
    }

    #[test]
    fn stooq_parse_edge_cases() {
        let good = b"Symbol,Date,Time,Close\r\nAAPL.US,2026-11-18,22:00:00,230.5\r\n";
        let q = parse_stooq(good, "aapl.us").unwrap();
        assert_eq!(q.price, 230.5);
        assert_eq!(q.date.to_string(), "2026-11-18");
        // Unknown symbol: every field is N/D.
        let nd = b"Symbol,Date,Time,Close\nXXXX.US,N/D,N/D,N/D\n";
        assert_eq!(parse_stooq(nd, "xxxx.us"), Err(FeedErr::Unavailable));
        for (body, why) in [
            (&b""[..], "empty"),
            (b"Symbol,Date,Time,Close\n", "header only"),
            (b"Symbol,Date,Time,Open\nAAPL.US,2026-11-18,22:00:00,1\n", "wrong header"),
            (b"Symbol,Date,Time,Close\nMSFT.US,2026-11-18,22:00:00,1\n", "wrong symbol"),
            (b"Symbol,Date,Time,Close\nAAPL.US,2026-11-18,22:00:00,abc\n", "text price"),
            (b"Symbol,Date,Time,Close\nAAPL.US,2026-11-18,22:00:00,0\n", "zero"),
            (b"Symbol,Date,Time,Close\nAAPL.US,2026-11-18,22:00:00,-1\n", "negative"),
            (b"Symbol,Date,Time,Close\nAAPL.US,2026-13-18,22:00:00,1\n", "bad date"),
            (b"Symbol,Date,Time,Close\nAAPL.US,2026-11-18,25:00:00,1\n", "bad time"),
            (b"Symbol,Date,Time,Close\nAAPL.US,2026-11-18,22:00:00,1,9\n", "extra column"),
            (b"Symbol,Date,Time,Close\nAAPL.US,2026-11-18,22:00:00,1\nAAPL.US,2026-11-18,22:00:00,2\n", "two rows"),
            (b"Symbol,Date,Time,Close\nAAPL.US,2026-11-18,22:00:00,NaN\n", "NaN"),
            (&[0xff, 0xfe][..], "not utf8"),
        ] {
            assert_eq!(parse_stooq(body, "aapl.us"), Err(FeedErr::Parse), "{why}");
        }
    }

    #[tokio::test]
    async fn an_environment_proxy_is_never_used() {
        // A fake proxy: if the client honoured HTTP_PROXY, the request would land here.
        let proxy_hits = Arc::new(AtomicUsize::new(0));
        let (addr, hits, _) = serve(vec![ok("fine")]).await;
        let proxy2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let paddr = proxy2.local_addr().unwrap();
        let ph2 = proxy_hits.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = proxy2.accept().await {
                ph2.fetch_add(1, Ordering::SeqCst);
                let _ = s.shutdown().await;
            }
        });
        std::env::set_var("HTTP_PROXY", format!("http://{paddr}"));
        std::env::set_var("http_proxy", format!("http://{paddr}"));
        // The production spec, with only https_only relaxed so a loopback server can answer.
        let mut spec = production_spec();
        assert!(spec.no_proxy, "production must ignore environment proxies");
        spec.https_only = false;
        let c = Pinned::build(vec![addr.clone()], spec, || false).unwrap();
        let r = c.get(&format!("http://{addr}/x"), 100).await;
        std::env::remove_var("HTTP_PROXY");
        std::env::remove_var("http_proxy");
        assert_eq!(r.unwrap(), b"fine");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 0);
    }

    static MID_GATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    fn mid_gate() -> bool {
        MID_GATE.load(Ordering::SeqCst)
    }

    #[tokio::test]
    async fn flipping_the_gate_mid_poll_stops_the_next_request() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                h.fetch_add(1, Ordering::SeqCst);
                // The first answer closes the gate, as Settings or navy would.
                MID_GATE.store(true, Ordering::SeqCst);
                let mut b = [0u8; 2048];
                let _ = s.read(&mut b).await;
                let body = r#"{"bitcoin":{"usd":5.5}}"#;
                let r = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = s.write_all(r.as_bytes()).await;
                let _ = s.shutdown().await;
            }
        });
        let d = temp_dir("mf-mid");
        let c = Pinned::test_loopback_gate(&addr, mid_gate);
        let picks = [crypto("bitcoin", "BTC"), stock("AAPL"), stock("MSFT")];
        let out = poll_in(&d, &c, &bases(&addr), &picks, t(NOW))
            .await
            .unwrap();
        assert_eq!(out, PollOutcome::Ran { good: 1, wanted: 3 });
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the stock requests were never sent"
        );
        let q = quotes_in(&d, &picks, t(NOW)).unwrap();
        assert_eq!(q[0].price, Some(5.5));
        assert_eq!(q[1].price, None);
        assert_eq!(q[1].error.as_deref(), Some("blocked"));
        MID_GATE.store(false, Ordering::SeqCst);
    }

    #[test]
    fn ticker_and_coin_id_shapes() {
        for ok in ["AAPL", "A", "BRK-B", "GOOGL"] {
            assert!(valid_stock_ticker(ok), "{ok}");
        }
        for bad in [
            "", "aapl", "TOOLONG", "AAPL.US", "A-BC", "A B", "AAPL-", "../x", "BTC1",
        ] {
            assert!(!valid_stock_ticker(bad), "{bad}");
        }
        for ok in ["bitcoin", "usd-coin", "x", "a1-b2"] {
            assert!(valid_coin_id(ok), "{ok}");
        }
        for bad in [
            "",
            "Bitcoin",
            "-a",
            "a-",
            "a--b",
            "a b",
            "a,b",
            "a/b",
            &"a".repeat(65),
        ] {
            assert!(!valid_coin_id(bad), "{bad}");
        }
    }

    #[test]
    fn stooq_closed_by_quote_age() {
        let q = |time: &str| StooqQuote {
            price: 1.0,
            date: NaiveDate::from_ymd_opt(2026, 11, 18).unwrap(),
            time: NaiveTime::parse_from_str(time, "%H:%M:%S").unwrap(),
        };
        // 22:00 CET is 21:00 UTC. At 21:20 UTC (06:20 JST next day) the quote is 20 min old: open.
        assert!(!stooq_closed(
            &q("22:00:00"),
            t("2026-11-19T06:20:00+09:00")
        ));
        // An hour later it is closed.
        assert!(stooq_closed(&q("22:00:00"), t("2026-11-19T07:30:00+09:00")));
        // A quote from yesterday is closed.
        assert!(stooq_closed(&q("17:00:00"), t("2026-11-19T12:00:00+09:00")));
    }

    #[tokio::test]
    async fn poll_caches_good_prices_keeps_the_last_good_and_reports_status() {
        let d = temp_dir("mf-poll");
        let csv = "Symbol,Date,Time,Close\nAAPL.US,2026-11-19,02:50:00,230.5\n";
        let (addr, hits, lines) = serve(vec![
            ok(r#"{"bitcoin":{"usd":67000.5},"nope":{}}"#),
            ok(csv),
        ])
        .await;
        let c = test_client(&addr, false);
        let picks = [
            crypto("bitcoin", "BTC"),
            crypto("nope", "NOPE"),
            stock("AAPL"),
        ];
        let now = t("2026-11-19T12:00:00+09:00"); // 03:00 UTC: AAPL quote 02:50 CET = 01:50 UTC, 70 min old
        let out = poll_in(&d, &c, &bases(&addr), &picks, now).await.unwrap();
        assert_eq!(out, PollOutcome::Ran { good: 2, wanted: 3 });
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        let l = lines.lock().unwrap().clone();
        assert!(l[0].contains("ids=bitcoin,nope") && l[0].contains("vs_currencies=usd"));
        assert!(l[1].contains("s=aapl.us"));

        let q = quotes_in(&d, &picks, now).unwrap();
        assert_eq!(q[0].price, Some(67000.5));
        assert_eq!(q[0].status, QuoteStatus::Open);
        assert_eq!(q[0].currency.as_deref(), Some("USD"));
        assert_eq!(
            q[0].fetched_at.as_deref(),
            Some("2026-11-19T12:00:00.000+09:00")
        );
        // A coin the source does not know: null with a reason, never 0.
        assert_eq!(q[1].price, None);
        assert_eq!(q[1].error.as_deref(), Some("unavailable"));
        assert_eq!(q[1].status, QuoteStatus::Stale);
        assert_eq!(q[2].status, QuoteStatus::Closed);

        // Too soon: no request.
        let again = poll_in(
            &d,
            &c,
            &bases(&addr),
            &picks,
            t("2026-11-19T12:10:00+09:00"),
        )
        .await
        .unwrap();
        assert_eq!(again, PollOutcome::TooSoon);
        assert_eq!(hits.load(Ordering::SeqCst), 2);

        // Stale after more than 2x the interval; the last good price is still shown.
        let late = t("2026-11-19T12:31:00+09:00");
        let q = quotes_in(&d, &picks, late).unwrap();
        assert_eq!(q[0].price, Some(67000.5));
        assert_eq!(q[0].status, QuoteStatus::Stale);
        let on_edge = t("2026-11-19T12:30:00+09:00");
        assert_eq!(
            quotes_in(&d, &picks[..1], on_edge).unwrap()[0].status,
            QuoteStatus::Open
        );

        // A later bad answer keeps the old price and records why.
        let (addr2, _, _) = serve(vec![ok("garbage"), ok("garbage")]).await;
        let c2 = test_client(&addr2, false);
        let out = poll_in(
            &d,
            &c2,
            &bases(&addr2),
            &picks,
            t("2026-11-19T12:20:00+09:00"),
        )
        .await
        .unwrap();
        assert_eq!(out, PollOutcome::Ran { good: 0, wanted: 3 });
        let q = quotes_in(&d, &picks, t("2026-11-19T12:21:00+09:00")).unwrap();
        assert_eq!(q[0].price, Some(67000.5));
        assert_eq!(q[0].error.as_deref(), Some("unparseable"));
        assert_eq!(
            q[0].fetched_at.as_deref(),
            Some("2026-11-19T12:00:00.000+09:00")
        );
    }

    #[tokio::test]
    async fn empty_list_and_unusable_ids_make_no_request() {
        let d = temp_dir("mf-idle");
        let (addr, hits, _) = serve(vec![ok("{}")]).await;
        let c = test_client(&addr, false);
        assert_eq!(
            poll_in(&d, &c, &bases(&addr), &[], t(NOW)).await.unwrap(),
            PollOutcome::Idle
        );
        // A hand-edited bad id or ticker is skipped, never put in a URL.
        let bad = [crypto("../etc", "X"), stock("a b")];
        let out = poll_in(&d, &c, &bases(&addr), &bad, t(NOW)).await.unwrap();
        assert_eq!(out, PollOutcome::Ran { good: 0, wanted: 0 });
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn feed_status_reflects_real_state() {
        let d = temp_dir("mf-status");
        let s = status_in(&d, false, t(NOW)).unwrap();
        assert_eq!(s.state, FeedState::NotConnected);
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"state":"not_connected","lastAttemptAt":null,"lastOkAt":null,"pollMinutes":15}"#
        );
        assert_eq!(
            status_in(&d, true, t(NOW)).unwrap().state,
            FeedState::Blocked
        );
        with_store(&d, true, |s| s.last_attempt_at = Some(iso(&t(NOW)))).unwrap();
        assert_eq!(
            status_in(&d, false, t(NOW)).unwrap().state,
            FeedState::Error
        );
        with_store(&d, true, |s| s.last_ok_at = Some(iso(&t(NOW)))).unwrap();
        assert_eq!(
            status_in(&d, false, t("2026-11-19T12:30:00+09:00"))
                .unwrap()
                .state,
            FeedState::Connected
        );
        assert_eq!(
            status_in(&d, false, t("2026-11-19T12:31:00+09:00"))
                .unwrap()
                .state,
            FeedState::Stale
        );
        assert_eq!(
            status_in(&d, true, t("2026-11-19T12:31:00+09:00"))
                .unwrap()
                .state,
            FeedState::Blocked
        );
    }

    #[test]
    fn the_poller_starts_only_from_setup() {
        let lib = include_str!("../lib.rs");
        assert_eq!(lib.matches("market_feed::start()").count(), 1);
        let src = include_str!("market_feed.rs");
        let prod = &src[..src.find("#[cfg(test)]\nmod tests").unwrap()];
        assert!(prod.contains("pub fn start()"));
        // No other production call site spawns a poll.
        assert_eq!(prod.matches("poll_tick().await").count(), 1);
    }

    #[test]
    fn source_has_no_keys_accounts_or_other_hosts() {
        let src = include_str!("market_feed.rs");
        let prod = src[..src.find("#[cfg(test)]\nmod tests").unwrap()].to_lowercase();
        for w in [
            "api_key",
            "apikey",
            "x-cg-",
            "bearer",
            "authorization",
            "keychain",
            "broker",
            "wallet",
            "mnemonic",
            "place_order",
            "cookie",
            "http://",
        ] {
            assert!(!prod.contains(w), "market_feed.rs must not contain {w}");
        }
        // Only the three approved hosts appear.
        let hosts: Vec<&str> = prod
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'))
            .filter(|w| w.ends_with(".com") || w.ends_with(".org") || w.ends_with(".net"))
            .collect();
        for h in hosts {
            assert!(
                ["api.coingecko.com", "stooq.com", "api.gdeltproject.org"].contains(&h),
                "unexpected host {h}"
            );
        }
    }
}
