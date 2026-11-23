//! Personal money, READ-ONLY (decision 0008, design-1111t §9). Nothing here moves, pays, transfers,
//! trades, converts or withdraws anything, and a source test fails if a command is named like that.
//!
//! Everything is local: `<data>/money/` (dir 0700; files 0600, written atomically, size-capped).
//! - `accounts.json`: the accounts (nickname, institution, route, currency, scope). No account numbers.
//! - `watch.json`: guard-accepted PUBLIC values only (xpub or address). A seed phrase or private key is
//!   refused (`refused_secret`) before anything is stored, logged or returned.
//! - `balances.json`: manual balances (source "manual").
//! - `tx/<id>.jsonl`: imported statement rows (JPY). Descriptions stay on this Mac.
//! - `fx.json`: the one JPY-per-USD reference rate, with its source and date; never invented.
//!
//! Errors are fixed codes (`invalid_input`, `refused_secret`, ...) that never echo the input.

use super::local_store::{clean_text, iso, new_id, now_jst, read_json, write_json_atomic};
use super::shared::task::{blocking, JoinErr};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The one approved outbound host for this module (decision 0008 §2).
const ECB_DAILY_URL: &str = "https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml";
const FX_TIMEOUT_SECS: u64 = 5;
const FX_MAX_BODY: usize = 256 * 1024;
const FX_MIN_INTERVAL_SECS: i64 = 600;

const MAX_IMPORT_BYTES: u64 = 5 * 1024 * 1024;
const MAX_TX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STORE_BYTES: u64 = 1024 * 1024;
const MAX_ACCOUNTS: usize = 50;
const MAX_WATCH: usize = 200;
const MAX_ROWS: usize = 100_000;
const MAX_CELLS: usize = 64;
const HEADER_SCAN_ROWS: usize = 50;
const MAX_DESC_CHARS: usize = 200;
const REMOVED: &str = "[removed]";

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ───────────────────────────── seed guard ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKind {
    Phrase,
    PrivateKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chain {
    Btc,
    Eth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Xpub,
    Address,
}

/// Rust mirror of `src-console/money/seedGuard.ts` (same verdicts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    Empty,
    Secret(SecretKind),
    Public(Chain, Form),
    Invalid,
}

fn is_b58(c: char) -> bool {
    c.is_ascii_alphanumeric() && !matches!(c, '0' | 'O' | 'I' | 'l')
}

fn all_b58_len(s: &str, min: usize, max: usize) -> bool {
    let n = s.chars().count();
    n >= min && n <= max && s.chars().all(is_b58)
}

/// A "word": latin letters or Japanese kana (the BIP-39 Japanese list).
fn is_word(w: &str) -> bool {
    !w.is_empty()
        && w.chars()
            .all(|c| c.is_ascii_alphabetic() || ('\u{3040}'..='\u{30ff}').contains(&c))
}

fn hex_len(s: &str, n: usize) -> bool {
    s.len() == n && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_xprv(t: &str) -> bool {
    let mut it = t.chars();
    matches!(it.next(), Some('x' | 'y' | 'z' | 't' | 'u' | 'v'))
        && it
            .as_str()
            .strip_prefix("prv")
            .is_some_and(|r| all_b58_len(r, 20, usize::MAX))
}

fn is_wif(t: &str) -> bool {
    let mut it = t.chars();
    matches!(it.next(), Some('5' | '9' | 'c' | 'K' | 'L')) && all_b58_len(it.as_str(), 50, 51)
}

fn is_hex_key(t: &str) -> bool {
    hex_len(t.strip_prefix("0x").unwrap_or(t), 64)
}

fn is_xpub(t: &str) -> bool {
    let mut it = t.chars();
    matches!(it.next(), Some('x' | 'y' | 'z' | 't'))
        && it
            .as_str()
            .strip_prefix("pub")
            .is_some_and(|r| all_b58_len(r, 100, 112))
}

fn is_btc_legacy(t: &str) -> bool {
    let mut it = t.chars();
    matches!(it.next(), Some('1' | '3')) && all_b58_len(it.as_str(), 25, 34)
}

fn is_btc_bech32(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    match l.strip_prefix("bc1") {
        Some(rest) => {
            (11..=87).contains(&rest.len())
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() && !matches!(c, '1' | 'b' | 'i' | 'o'))
        }
        None => false,
    }
}

fn is_eth(t: &str) -> bool {
    t.strip_prefix("0x").is_some_and(|h| hex_len(h, 40))
}

fn guard_basic(raw: &str) -> Guard {
    let text = raw.trim();
    if text.is_empty() {
        return Guard::Empty;
    }
    let words: Vec<&str> = text
        .split(char::is_whitespace)
        .filter(|w| !w.is_empty())
        .collect();
    if words.len() >= 11
        && words
            .iter()
            .all(|w| is_word(w.strip_suffix(['.', ',', ';', ':']).unwrap_or(w)))
    {
        return Guard::Secret(SecretKind::Phrase);
    }
    if words.len() == 1 {
        let t = words[0];
        if is_xprv(t) || is_wif(t) || is_hex_key(t) {
            return Guard::Secret(SecretKind::PrivateKey);
        }
        if is_xpub(t) {
            return Guard::Public(Chain::Btc, Form::Xpub);
        }
        if is_btc_bech32(t) || is_btc_legacy(t) {
            return Guard::Public(Chain::Btc, Form::Address);
        }
        if is_eth(t) {
            return Guard::Public(Chain::Eth, Form::Address);
        }
    }
    Guard::Invalid
}

/// Hand-rolled fold (`unicode-normalization` is not in Cargo.lock): full-width ASCII (U+FF01..U+FF5E)
/// becomes ASCII, and combining diacritics (U+0300..U+036F) are dropped.
fn fold(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xfee0),
            '\u{0300}'..='\u{036f}' => None,
            c => Some(c),
        })
        .collect()
}

/// Folded text with every non-alphanumeric char (spaces, punctuation, `_`, `-`, and all control and
/// format chars such as U+200B, U+2060, U+FEFF) turned into a separator, split into tokens. A separator,
/// not a deletion, so zero-width characters can't glue words together.
fn norm_tokens(text: &str) -> Vec<String> {
    fold(text)
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// 11+ tokens of which at least 80% are alphabetic in any script.
fn looks_like_phrase(text: &str) -> bool {
    let t = norm_tokens(text);
    t.len() >= 11
        && t.iter()
            .filter(|w| w.chars().all(char::is_alphabetic))
            .count()
            * 5
            >= t.len() * 4
}

/// A hex run of 64+ or a base58 run of 50+ chars.
fn has_key_run(token: &str) -> bool {
    let (mut hex, mut b58) = (0usize, 0usize);
    for c in token.chars() {
        hex = if c.is_ascii_hexdigit() { hex + 1 } else { 0 };
        b58 = if is_b58(c) { b58 + 1 } else { 0 };
        if hex >= 64 || b58 >= 50 {
            return true;
        }
    }
    false
}

/// Private-key shapes anywhere in free text (a nickname or a statement description).
fn contains_private_key(text: &str) -> bool {
    let key_token = |t: &str| is_xprv(t) || is_wif(t) || is_hex_key(t) || has_key_run(t);
    fold(text).split_whitespace().any(key_token) || norm_tokens(text).iter().any(|t| key_token(t))
}

/// Runs on any text before it is kept. Over-refuses on purpose: a phrase (11+ words, any separator or
/// script, no checksum needed) or a private key is Secret; only a public xpub or address passes. The
/// verdict is the stricter of the raw text, the folded text and the normalised token view.
pub fn guard_input(raw: &str) -> Guard {
    let a = guard_basic(raw);
    if matches!(a, Guard::Secret(_)) {
        return a;
    }
    let b = guard_basic(&fold(raw));
    if matches!(b, Guard::Secret(_)) {
        return b;
    }
    if looks_like_phrase(raw) {
        return Guard::Secret(SecretKind::Phrase);
    }
    a
}

/// For free text (nicknames, descriptions): a phrase, or a private key anywhere in it.
fn is_secret(raw: &str) -> bool {
    matches!(guard_input(raw), Guard::Secret(_)) || contains_private_key(raw)
}

// ───────────────────────────── accounts ─────────────────────────────

const INSTITUTIONS: [&str; 5] = ["smbc", "japan_post", "chime", "fidelity", "ledger"];
const KINDS: [&str; 8] = [
    "checking",
    "savings",
    "brokerage",
    "ira",
    "ordinary",
    "time_deposit",
    "btc",
    "eth",
];
const ROUTES: [&str; 3] = ["csv", "watch_only", "manual"];
const CURRENCIES: [&str; 4] = ["JPY", "USD", "BTC", "ETH"];
const SCOPES: [&str; 2] = ["personal", "venture"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct MoneyAccount {
    pub id: String,
    pub nickname: String,
    pub institution: String,
    pub kind: String,
    pub route: String,
    pub currency: String,
    pub scope: String,
    pub created_at: String,
}

/// What the UI sends. Enum-like fields are plain strings so a bad value gets a fixed error, not a
/// serde message that quotes it.
#[derive(Debug, Clone, Deserialize, specta::Type)]
pub struct MoneyAccountDraft {
    pub nickname: String,
    pub institution: String,
    pub kind: String,
    pub route: String,
    pub currency: String,
    pub scope: String,
}

fn money_dir(data: &Path) -> PathBuf {
    data.join("money")
}

fn load_accounts(data: &Path) -> Result<Vec<MoneyAccount>, String> {
    read_json(&money_dir(data).join("accounts.json"), MAX_STORE_BYTES)
        .map_err(|_| "store_error".to_string())
}

fn find_account(data: &Path, id: &str) -> Result<MoneyAccount, String> {
    load_accounts(data)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| "not_found".to_string())
}

pub(crate) fn accounts_list_in(data: &Path) -> Result<Vec<MoneyAccount>, String> {
    load_accounts(data)
}

pub(crate) fn account_add_in(
    data: &Path,
    draft: &MoneyAccountDraft,
) -> Result<MoneyAccount, String> {
    // Seed guard first, on the raw text, so a phrase is always `refused_secret` and never echoed.
    if is_secret(&draft.nickname) || norm_tokens(&draft.nickname).len() >= 8 {
        return Err("refused_secret".to_string());
    }
    let nickname = clean_text(&draft.nickname, 1, 60).map_err(|_| "invalid_input".to_string())?;
    // Allowlist: letters and digits of any script, space, `_ . ' -`.
    if !nickname
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '.' | '\'' | '-'))
    {
        return Err("invalid_input".to_string());
    }
    let pick = |v: &str, allowed: &[&str]| -> Result<String, String> {
        if allowed.contains(&v) {
            Ok(v.to_string())
        } else {
            Err("invalid_input".to_string())
        }
    };
    let institution = pick(&draft.institution, &INSTITUTIONS)?;
    let kind = pick(&draft.kind, &KINDS)?;
    let route = pick(&draft.route, &ROUTES)?;
    let currency = pick(&draft.currency, &CURRENCIES)?;
    let scope = pick(&draft.scope, &SCOPES)?;
    // Consistency: statement CSVs exist for the two Japanese banks only; watch-only and BTC/ETH are Ledger.
    let jp = matches!(institution.as_str(), "smbc" | "japan_post");
    if (route == "csv" && !(jp && currency == "JPY"))
        || (route == "watch_only" && institution != "ledger")
        || (matches!(currency.as_str(), "BTC" | "ETH") && institution != "ledger")
    {
        return Err("invalid_input".to_string());
    }
    let _g = lock();
    let mut accounts = load_accounts(data)?;
    if accounts.len() >= MAX_ACCOUNTS {
        return Err("store_full".to_string());
    }
    let account = MoneyAccount {
        id: new_id("acct"),
        nickname,
        institution,
        kind,
        route,
        currency,
        scope,
        created_at: iso(&now_jst()),
    };
    accounts.push(account.clone());
    write_json_atomic(
        &money_dir(data).join("accounts.json"),
        &accounts,
        MAX_STORE_BYTES,
    )
    .map_err(|_| "store_error".to_string())?;
    Ok(account)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct WatchEntry {
    account_id: String,
    chain: String,
    form: String,
    value: String,
    added_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct WatchAdded {
    pub chain: String,
    pub form: String,
}

pub(crate) fn watch_add_in(
    data: &Path,
    account_id: &str,
    value: &str,
) -> Result<WatchAdded, String> {
    let (chain, form) = match guard_input(value) {
        Guard::Secret(_) => return Err("refused_secret".to_string()),
        Guard::Public(c, f) => (c, f),
        Guard::Empty | Guard::Invalid => return Err("invalid_input".to_string()),
    };
    let account = find_account(data, account_id)?;
    let want = match chain {
        Chain::Btc => "BTC",
        Chain::Eth => "ETH",
    };
    if account.route != "watch_only" || account.currency != want {
        return Err("invalid_input".to_string());
    }
    let added = WatchAdded {
        chain: if chain == Chain::Btc { "btc" } else { "eth" }.to_string(),
        form: if form == Form::Xpub {
            "xpub"
        } else {
            "address"
        }
        .to_string(),
    };
    let _g = lock();
    let path = money_dir(data).join("watch.json");
    let mut entries: Vec<WatchEntry> =
        read_json(&path, MAX_STORE_BYTES).map_err(|_| "store_error".to_string())?;
    let value = value.trim().to_string();
    if entries
        .iter()
        .any(|e| e.account_id == account.id && e.value == value)
    {
        return Ok(added);
    }
    if entries.len() >= MAX_WATCH {
        return Err("store_full".to_string());
    }
    entries.push(WatchEntry {
        account_id: account.id,
        chain: added.chain.clone(),
        form: added.form.clone(),
        value,
        added_at: iso(&now_jst()),
    });
    write_json_atomic(&path, &entries, MAX_STORE_BYTES).map_err(|_| "store_error".to_string())?;
    Ok(added)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ManualBalance {
    account_id: String,
    native_amount: f64,
    as_of: String,
    source: String,
    set_at: String,
}

pub(crate) fn balance_set_manual_in(
    data: &Path,
    account_id: &str,
    native_amount: f64,
    as_of: &str,
) -> Result<(), String> {
    if !native_amount.is_finite() || native_amount.abs() >= 1e15 {
        return Err("invalid_input".to_string());
    }
    let as_of = parse_iso_day(as_of).ok_or_else(|| "invalid_input".to_string())?;
    let account = find_account(data, account_id)?;
    if account.route != "manual" && account.institution != "ledger" {
        return Err("invalid_input".to_string());
    }
    let _g = lock();
    let path = money_dir(data).join("balances.json");
    let mut all: Vec<ManualBalance> =
        read_json(&path, MAX_STORE_BYTES).map_err(|_| "store_error".to_string())?;
    all.retain(|b| b.account_id != account.id);
    all.push(ManualBalance {
        account_id: account.id,
        native_amount,
        as_of,
        source: "manual".to_string(),
        set_at: iso(&now_jst()),
    });
    write_json_atomic(&path, &all, MAX_STORE_BYTES).map_err(|_| "store_error".to_string())
}

/// `YYYY-MM-DD` only, a real calendar day; returns it normalised.
fn parse_iso_day(s: &str) -> Option<String> {
    let d = NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()?;
    Some(d.format("%Y-%m-%d").to_string())
}

// ───────────────────────────── statement CSV import ─────────────────────────────

/// Full-width digits and signs to ASCII, drops separators and currency marks. `Ok(None)` = empty cell,
/// `Err` = not a whole-yen number. A `-`, `−`, `△` or `▲` means negative.
fn parse_yen(raw: &str) -> Result<Option<i64>, ()> {
    let mut neg = false;
    let mut digits = String::new();
    for (i, c) in raw.trim().chars().enumerate() {
        match c {
            '0'..='9' => digits.push(c),
            '\u{ff10}'..='\u{ff19}' => {
                digits.push(char::from(b'0' + (c as u32 - 0xff10) as u8));
            }
            // A sign counts only as the very first character.
            '-' | '\u{2212}' | '\u{ff0d}' | '△' | '▲' if i == 0 => neg = true,
            '+' | '\u{ff0b}' if i == 0 => {}
            ',' | '\u{ff0c}' | '円' | '¥' | '\u{ffe5}' => {}
            c if c.is_whitespace() => {}
            _ => return Err(()),
        }
    }
    if digits.is_empty() {
        return if raw.chars().all(char::is_whitespace) {
            Ok(None)
        } else {
            Err(())
        };
    }
    if digits.len() > 15 {
        return Err(());
    }
    let n: i64 = digits.parse().map_err(|_| ())?;
    Ok(Some(if neg { -n } else { n }))
}

fn ascii_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{ff10}'..='\u{ff19}' => char::from(b'0' + (c as u32 - 0xff10) as u8),
            '\u{ff0f}' => '/',
            '\u{ff0d}' | '\u{2212}' => '-',
            '\u{ff0e}' => '.',
            c => c,
        })
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn ymd(y: i32, m: u32, d: u32) -> Option<String> {
    if !(1900..=2100).contains(&y) {
        return None;
    }
    NaiveDate::from_ymd_opt(y, m, d).map(|d| d.format("%Y-%m-%d").to_string())
}

fn nums3(s: &str) -> Option<(i32, u32, u32)> {
    let mut it = s.split('.');
    let (a, b, c) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() {
        return None;
    }
    let n = |x: &str| -> Option<u32> {
        if x == "元" {
            return Some(1);
        }
        if x.is_empty() || x.len() > 4 || !x.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        x.parse().ok()
    };
    Some((n(a)? as i32, n(b)?, n(c)?))
}

fn era_prefix(s: &str) -> (i32, &str) {
    let letter = |set: [char; 2], offset: i32| -> Option<(i32, &str)> {
        s.strip_prefix(set)
            .filter(|r| r.starts_with(|c: char| c.is_ascii_digit()))
            .map(|r| (offset, r))
    };
    if let Some(r) = s.strip_prefix("令和") {
        (2018, r)
    } else if let Some(r) = s.strip_prefix("平成") {
        (1988, r)
    } else if let Some(r) = s.strip_prefix("昭和") {
        (1925, r)
    } else if let Some(x) = letter(['R', 'r'], 2018)
        .or_else(|| letter(['H', 'h'], 1988))
        .or_else(|| letter(['S', 's'], 1925))
    {
        x
    } else {
        (0, s)
    }
}

/// YYYY/MM/DD, YYYY-MM-DD, YYYYMMDD, YYYY年M月D日, 令和N年M月D日, RN.MM.DD (also 平成/H and 昭和/S).
pub(crate) fn parse_date(raw: &str) -> Option<String> {
    let s = ascii_digits(raw);
    let (offset, rest) = era_prefix(&s);
    if offset == 0 && rest.len() == 8 && rest.chars().all(|c| c.is_ascii_digit()) {
        return ymd(
            rest[..4].parse().ok()?,
            rest[4..6].parse().ok()?,
            rest[6..].parse().ok()?,
        );
    }
    let norm = rest.replace(['年', '月', '/', '-'], ".").replace('日', "");
    let (y, m, d) = nums3(norm.trim_end_matches('.'))?;
    if offset == 0 {
        if !(1000..=9999).contains(&y) {
            return None;
        }
        ymd(y, m, d)
    } else if y < 1 {
        None
    } else {
        ymd(y + offset, m, d)
    }
}

/// RFC 4180-ish: commas, double-quote escaping, newlines inside quotes, CRLF. At most `MAX_ROWS` rows
/// are kept; the count of rows beyond that comes back as the second value (they are skipped, not
/// silently dropped). A row keeps at most `MAX_CELLS + 1` cells, so `len() > MAX_CELLS` marks a wide row.
fn parse_csv(text: &str) -> (Vec<Vec<String>>, usize) {
    let mut rows = Vec::new();
    let mut overflow = 0usize;
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut end_row = |row: &mut Vec<String>, cell: &mut String, rows: &mut Vec<Vec<String>>| {
        if row.len() <= MAX_CELLS {
            row.push(std::mem::take(cell));
        }
        cell.clear();
        let done = std::mem::take(row);
        if rows.len() < MAX_ROWS {
            rows.push(done);
        } else {
            overflow += 1;
        }
    };
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            ',' => {
                if row.len() <= MAX_CELLS {
                    row.push(std::mem::take(&mut cell));
                }
                cell.clear();
            }
            '\r' => {}
            '\n' => end_row(&mut row, &mut cell, &mut rows),
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        end_row(&mut row, &mut cell, &mut rows);
    }
    (rows, overflow)
}

#[derive(Debug, Clone, Copy)]
struct Cols {
    date: usize,
    withdrawal: Option<usize>,
    deposit: Option<usize>,
    amount: Option<usize>,
    kubun: Option<usize>,
    balance: Option<usize>,
    desc: Option<usize>,
}

fn header_role(cell: &str) -> Option<usize> {
    let mut s: String = cell
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{feff}')
        .collect();
    for suffix in ["(円)", "（円）"] {
        if let Some(t) = s.strip_suffix(suffix) {
            s = t.to_string();
        }
    }
    Some(match s.as_str() {
        "年月日" | "取引日" | "日付" | "お取引日" => 0,
        "お引出し" | "お引出し金額" | "出金" | "払戻し" | "払出金額" => 1,
        "お預入れ" | "お預入れ金額" | "入金" | "預入金額" => 2,
        "金額" | "取引金額" => 3,
        "受払区分" => 4,
        "残高" | "差引残高" => 5,
        "お取り扱い内容" | "摘要" | "取引内容" | "内容" => 6,
        _ => return None,
    })
}

/// Known column names only. A row is a header if it has a date column and either split in/out columns
/// or a signed amount column, with no role claimed twice. Never guesses.
fn header_cols(row: &[String]) -> Option<Cols> {
    let mut slots: [Option<usize>; 7] = [None; 7];
    for (i, cell) in row.iter().enumerate() {
        if let Some(role) = header_role(cell) {
            if slots[role].is_some() {
                return None;
            }
            slots[role] = Some(i);
        }
    }
    let cols = Cols {
        date: slots[0]?,
        withdrawal: slots[1],
        deposit: slots[2],
        amount: slots[3],
        kubun: slots[4],
        balance: slots[5],
        desc: slots[6],
    };
    if cols.withdrawal.is_some() || cols.deposit.is_some() || cols.amount.is_some() {
        Some(cols)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedTx {
    date: String,
    amount: i64,
    balance: Option<i64>,
    description: String,
}

fn cell_at(row: &[String], i: Option<usize>) -> &str {
    i.and_then(|i| row.get(i)).map(String::as_str).unwrap_or("")
}

fn clean_description(raw: &str) -> String {
    let flat: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let text = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if is_secret(&text) {
        return REMOVED.to_string();
    }
    text.chars().take(MAX_DESC_CHARS).collect()
}

fn parse_row(row: &[String], c: &Cols) -> Option<ParsedTx> {
    let date = parse_date(cell_at(row, Some(c.date)))?;
    let amount = if c.withdrawal.is_some() || c.deposit.is_some() {
        let w = parse_yen(cell_at(row, c.withdrawal))
            .ok()?
            .unwrap_or(0)
            .abs();
        let d = parse_yen(cell_at(row, c.deposit)).ok()?.unwrap_or(0).abs();
        match (w, d) {
            (w, 0) => -w,
            (0, d) => d,
            _ => return None,
        }
    } else {
        let a = parse_yen(cell_at(row, c.amount)).ok()??;
        if c.kubun.is_some() {
            let k = cell_at(row, c.kubun);
            let out = ["出", "支", "払", "引"].iter().any(|m| k.contains(m));
            let inn = ["入", "預"].iter().any(|m| k.contains(m));
            match (out, inn) {
                (true, false) => -a.abs(),
                (false, true) => a.abs(),
                _ => return None,
            }
        } else {
            a
        }
    };
    if amount == 0 {
        return None;
    }
    let balance = parse_yen(cell_at(row, c.balance)).ok()?;
    Some(ParsedTx {
        date,
        amount,
        balance,
        description: clean_description(cell_at(row, c.desc)),
    })
}

fn decode_statement(bytes: &[u8]) -> Result<String, String> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf][..]).unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Ok(s.to_string());
    }
    let (text, _, had_errors) = encoding_rs::SHIFT_JIS.decode(bytes);
    if had_errors {
        return Err("unrecognised_format".to_string());
    }
    Ok(text.into_owned())
}

/// Parses a statement. `unrecognised_format` when no known header row is found; nothing is guessed.
fn parse_statement(bytes: &[u8]) -> Result<(Vec<ParsedTx>, usize), String> {
    let text = decode_statement(bytes)?;
    let (rows, overflow) = parse_csv(&text);
    let (idx, cols) = rows
        .iter()
        .take(HEADER_SCAN_ROWS)
        .enumerate()
        .find_map(|(i, r)| {
            if r.len() > MAX_CELLS {
                return None;
            }
            header_cols(r).map(|c| (i, c))
        })
        .ok_or_else(|| "unrecognised_format".to_string())?;
    let mut txs = Vec::new();
    let mut skipped = overflow;
    for row in &rows[idx + 1..] {
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        if row.len() > MAX_CELLS {
            skipped += 1;
            continue;
        }
        match parse_row(row, &cols) {
            Some(t) => txs.push(t),
            None => skipped += 1,
        }
    }
    Ok((txs, skipped))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TxRecord {
    key: String,
    date: String,
    amount: i64,
    balance: Option<i64>,
    description: String,
}

/// SHA-256 of account + date + amount + balance + description (+ the n-th identical row in the file,
/// so two real same-day, same-amount rows are both kept while a re-import adds nothing).
fn dedupe_key(account_id: &str, t: &ParsedTx, nth: usize) -> String {
    let mut h = Sha256::new();
    for part in [
        account_id,
        &t.date,
        &t.amount.to_string(),
        &t.balance.map(|b| b.to_string()).unwrap_or_default(),
        &t.description,
        &nth.to_string(),
    ] {
        h.update(part.as_bytes());
        h.update([0x1f]);
    }
    hex::encode(h.finalize())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ImportSummary {
    pub added: usize,
    pub duplicates: usize,
    pub skipped_rows: usize,
}

/// Opens without following a symlink and without blocking (a FIFO or device can't hang the open), then
/// checks on the handle that it is a regular file within the size cap.
fn open_regular_nofollow(path: &Path) -> Result<fs::File, String> {
    let mut o = fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let f = o.open(path).map_err(|_| "bad_file".to_string())?;
    let meta = f.metadata().map_err(|_| "bad_file".to_string())?;
    if !meta.file_type().is_file() {
        return Err("bad_file".to_string());
    }
    if meta.len() > MAX_IMPORT_BYTES {
        return Err("too_large".to_string());
    }
    Ok(f)
}

fn read_import_file(path: &Path) -> Result<Vec<u8>, String> {
    use std::path::Component;
    if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        return Err("invalid_input".to_string());
    }
    let meta = fs::symlink_metadata(path).map_err(|_| "bad_file".to_string())?;
    if !meta.file_type().is_file() {
        return Err("bad_file".to_string());
    }
    if meta.len() > MAX_IMPORT_BYTES {
        return Err("too_large".to_string());
    }
    let f = open_regular_nofollow(path)?;
    let mut buf = Vec::new();
    f.take(MAX_IMPORT_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|_| "bad_file".to_string())?;
    if buf.len() as u64 > MAX_IMPORT_BYTES {
        return Err("too_large".to_string());
    }
    Ok(buf)
}

/// The raw text of a transaction file (symlink, non-file and oversize are refused) or "" if absent.
fn read_tx_text(path: &Path) -> Result<String, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(_) => return Err("store_error".to_string()),
    };
    if !meta.file_type().is_file() || meta.len() > MAX_TX_FILE_BYTES {
        return Err("store_error".to_string());
    }
    fs::read_to_string(path).map_err(|_| "store_error".to_string())
}

fn parse_tx_records(text: &str) -> Result<Vec<TxRecord>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(|_| "store_error".to_string()))
        .collect()
}

/// Kept for the tests (path in, rows out); no command passes a renderer path any more.
#[cfg(test)]
pub(crate) fn import_statement_in(
    data: &Path,
    account_id: &str,
    path: &Path,
) -> Result<ImportSummary, String> {
    let account = find_account(data, account_id)?;
    if account.route != "csv" {
        return Err("invalid_input".to_string());
    }
    let bytes = read_import_file(path)?;
    // Everything is parsed before anything is written, so a refused file leaves nothing behind.
    let (txs, skipped_rows) = parse_statement(&bytes)?;
    import_parsed_in(data, &account, txs, skipped_rows)
}

/// Writes already-parsed rows into an account's transaction file (dedupes against what is there).
/// The caller read and parsed the file once; nothing here touches a path.
pub(crate) fn import_parsed_in(
    data: &Path,
    account: &MoneyAccount,
    txs: Vec<ParsedTx>,
    skipped_rows: usize,
) -> Result<ImportSummary, String> {
    let _g = lock();
    let file = money_dir(data)
        .join("tx")
        .join(format!("{}.jsonl", account.id));
    let mut text = read_tx_text(&file)?;
    let mut seen: HashSet<String> = parse_tx_records(&text)?
        .into_iter()
        .map(|r| r.key)
        .collect();
    let mut counts: HashMap<(String, i64, Option<i64>, String), usize> = HashMap::new();
    let mut added = 0usize;
    let mut duplicates = 0usize;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for t in txs {
        let n = counts
            .entry((t.date.clone(), t.amount, t.balance, t.description.clone()))
            .or_insert(0);
        let key = dedupe_key(&account.id, &t, *n);
        *n += 1;
        if !seen.insert(key.clone()) {
            duplicates += 1;
            continue;
        }
        let rec = TxRecord {
            key,
            date: t.date,
            amount: t.amount,
            balance: t.balance,
            description: t.description,
        };
        text.push_str(&serde_json::to_string(&rec).map_err(|_| "store_error".to_string())?);
        text.push('\n');
        added += 1;
    }
    if added > 0 {
        super::local_store::write_text_atomic(&file, &text, MAX_TX_FILE_BYTES)
            .map_err(|_| "store_error".to_string())?;
    }
    Ok(ImportSummary {
        added,
        duplicates,
        skipped_rows,
    })
}

/// What the Connect sheet's "Import a statement" returns: counts only, never a path or a row.
#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct ImportPicked {
    pub imported: usize,
    pub skipped: usize,
}

/// Fixed error codes for the file-picker import. Nothing from a path or an error text gets through.
fn import_pick_code(e: &str) -> String {
    match e {
        "cancelled"
        | "bad_file"
        | "too_large"
        | "unrecognised_format"
        | "store_error"
        | "store_full"
        | "invalid_input" => e.to_string(),
        _ => "failed".to_string(),
    }
}

/// The import behind the native file picker. `path` is what the user picked (None: they cancelled).
/// The statement is read and parsed first, so a refused file creates nothing. Parsing finds the
/// bank's layout from the header row, so the one yen statement account (made on first use, nickname
/// "Japanese bank") serves both banks. Never asks the renderer for a path.
pub(crate) fn import_pick_in(data: &Path, path: Option<&Path>) -> Result<ImportPicked, String> {
    let run = || -> Result<ImportPicked, String> {
        let path = path.ok_or_else(|| "cancelled".to_string())?;
        // One read, one parse: the rows imported below are the rows parsed here.
        let (txs, skipped_rows) = parse_statement(&read_import_file(path)?)?;
        let existing = load_accounts(data)?
            .into_iter()
            .find(|a| a.route == "csv" && matches!(a.institution.as_str(), "smbc" | "japan_post"));
        let (account, created) = match existing {
            Some(a) => (a, false),
            None => (
                account_add_in(
                    data,
                    &MoneyAccountDraft {
                        nickname: "Japanese bank".into(),
                        institution: "smbc".into(),
                        kind: "checking".into(),
                        route: "csv".into(),
                        currency: "JPY".into(),
                        scope: "personal".into(),
                    },
                )?,
                true,
            ),
        };
        match import_parsed_in(data, &account, txs, skipped_rows) {
            Ok(s) => Ok(ImportPicked {
                imported: s.added,
                skipped: s.skipped_rows + s.duplicates,
            }),
            Err(e) => {
                // An account this call made, with nothing imported into it, is taken back out.
                if created {
                    rollback_new_account(data, &account.id);
                }
                Err(e)
            }
        }
    };
    run().map_err(|e| import_pick_code(&e))
}

/// Removes an account this import just created (never one that existed before) and any tx file it
/// started. Best effort: the failure being reported is the import's, not this one's.
fn rollback_new_account(data: &Path, id: &str) {
    let _g = lock();
    if let Ok(mut all) = load_accounts(data) {
        all.retain(|a| a.id != id);
        let _ = write_json_atomic(
            &money_dir(data).join("accounts.json"),
            &all,
            MAX_STORE_BYTES,
        );
    }
    let _ = fs::remove_file(money_dir(data).join("tx").join(format!("{id}.jsonl")));
}

// ───────────────────────────── FX reference rate ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename = "MoneyFxRate")]
pub struct FxRate {
    /// JPY per 1 USD.
    pub rate: f64,
    /// The rate's own date (the ECB publication date, or the day the user typed).
    pub date: String,
    /// "ecb" | "manual"
    pub source: String,
    pub fetched_at: String,
}

fn fx_path(data: &Path) -> PathBuf {
    money_dir(data).join("fx.json")
}

pub(crate) fn fx_get_in(data: &Path) -> Result<Option<FxRate>, String> {
    read_json::<Option<FxRate>>(&fx_path(data), 16 * 1024).map_err(|_| "store_error".to_string())
}

fn valid_rate(rate: f64) -> bool {
    rate.is_finite() && rate > 0.0 && rate < 1000.0
}

fn fx_store(data: &Path, rate: &FxRate) -> Result<(), String> {
    let _g = lock();
    write_json_atomic(&fx_path(data), rate, 16 * 1024).map_err(|_| "store_error".to_string())
}

pub(crate) fn fx_set_manual_in(data: &Path, rate: f64, date: &str) -> Result<FxRate, String> {
    if !valid_rate(rate) {
        return Err("invalid_input".to_string());
    }
    let date = parse_iso_day(date).ok_or_else(|| "invalid_input".to_string())?;
    let r = FxRate {
        rate,
        date,
        source: "manual".to_string(),
        fetched_at: iso(&now_jst()),
    };
    fx_store(data, &r)?;
    Ok(r)
}

fn xml_attr(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(p) = tag[from..].find(name) {
        let at = from + p;
        let after = tag[at + name.len()..].trim_start();
        if tag[..at].ends_with(char::is_whitespace) {
            if let Some(rest) = after.strip_prefix('=') {
                let rest = rest.trim_start();
                let q = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
                let body = &rest[1..];
                return body.find(q).map(|e| body[..e].to_string());
            }
        }
        from = at + name.len();
    }
    None
}

/// The ECB daily file to (date, JPY per USD). `JPY per USD = JPY per EUR / USD per EUR`. Missing or bad
/// USD or JPY, or no date, is an error; there is no default.
pub(crate) fn parse_ecb_daily(xml: &str) -> Result<(String, f64), String> {
    let bad = || "ecb_parse_failed".to_string();
    let mut date: Option<String> = None;
    let (mut usd, mut jpy) = (None, None);
    let mut rest = xml;
    while let Some(p) = rest.find("<Cube") {
        let after = &rest[p..];
        let end = after.find('>').ok_or_else(bad)?;
        let tag = &after[..end];
        if let Some(t) = xml_attr(tag, "time") {
            if date.is_some() {
                break;
            }
            date = Some(parse_iso_day(&t).ok_or_else(bad)?);
        } else if date.is_some() {
            if let (Some(cur), Some(r)) = (xml_attr(tag, "currency"), xml_attr(tag, "rate")) {
                let v: f64 = r.trim().parse().map_err(|_| bad())?;
                match cur.as_str() {
                    "USD" if usd.is_none() => usd = Some(v),
                    "JPY" if jpy.is_none() => jpy = Some(v),
                    _ => {}
                }
            }
        }
        rest = &after[end..];
    }
    let (date, usd, jpy) = (
        date.ok_or_else(bad)?,
        usd.ok_or_else(bad)?,
        jpy.ok_or_else(bad)?,
    );
    if !(usd.is_finite() && usd > 0.0 && jpy.is_finite() && jpy > 0.0) {
        return Err(bad());
    }
    let rate = ((jpy / usd) * 10_000.0).round() / 10_000.0;
    if !valid_rate(rate) {
        return Err(bad());
    }
    Ok((date, rate))
}

/// Kill switch and navy mode are checked first; `fetch` (the network) only runs when both are off.
pub(crate) async fn fx_refresh_in<F, Fut>(
    data: &Path,
    outbound_disabled: bool,
    local_only: bool,
    now: chrono::DateTime<chrono::FixedOffset>,
    fetch: F,
) -> Result<FxRate, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    if outbound_disabled || local_only {
        return Err("outbound_disabled".to_string());
    }
    // At most one fetch per FX_MIN_INTERVAL_SECS: inside the window the stored ECB rate is returned.
    if let Some(r) = fx_get_in(data)? {
        if r.source == "ecb" {
            if let Ok(at) = chrono::DateTime::parse_from_rfc3339(&r.fetched_at) {
                let elapsed = (now - at).num_seconds();
                if (0..FX_MIN_INTERVAL_SECS).contains(&elapsed) {
                    return Ok(r);
                }
            }
        }
    }
    let body = fetch().await?;
    let (date, rate) = parse_ecb_daily(&body)?;
    let r = FxRate {
        rate,
        date,
        source: "ecb".to_string(),
        fetched_at: iso(&now),
    };
    fx_store(data, &r)?;
    Ok(r)
}

/// GET the ECB daily file: one host, 5 s timeout, no redirects, no retries, capped body.
async fn fetch_ecb() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FX_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .https_only(true)
        .build()
        .map_err(|_| "fetch_failed".to_string())?;
    let mut resp = client
        .get(ECB_DAILY_URL)
        .send()
        .await
        .map_err(|_| "fetch_failed".to_string())?;
    if !resp.status().is_success() {
        return Err("fetch_failed".to_string());
    }
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|_| "fetch_failed".to_string())? {
        if body.len() + chunk.len() > FX_MAX_BODY {
            return Err("too_large".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| "fetch_failed".to_string())
}

// ───────────────────────────── balances (read-only) ─────────────────────────────

/// How far back `history` reaches, in days.
const HISTORY_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct BalancePoint {
    pub date: String,
    pub amount: f64,
}

/// One account's current balance. Only accounts with a real stored value appear; nothing is estimated.
#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct AccountBalance {
    pub account_id: String,
    pub currency: String,
    /// In the account's own currency.
    pub amount: f64,
    pub as_of: String,
    /// "manual" (typed by the user) | "statement" (the running balance in an imported CSV).
    pub source: String,
    /// One point per day, last `HISTORY_DAYS` days, oldest first. Manual balances keep no history (the
    /// store holds the latest value only), so they have at most their own point.
    pub history: Vec<BalancePoint>,
}

#[derive(Debug, Clone, Serialize, PartialEq, specta::Type)]
pub struct MoneyBalances {
    pub balances: Vec<AccountBalance>,
    /// The stored JPY-per-USD reference rate, or `None`. Never invented.
    pub fx: Option<FxRate>,
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// The last running balance per day from imported rows (rows without a balance are ignored).
fn statement_points(records: &[TxRecord]) -> Vec<BalancePoint> {
    let mut rows: Vec<(&str, i64)> = records
        .iter()
        .filter_map(|r| r.balance.map(|b| (r.date.as_str(), b)))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(b.0)); // stable: file order breaks same-day ties, the last one wins
    let mut out: Vec<BalancePoint> = Vec::new();
    for (date, b) in rows {
        let point = BalancePoint {
            date: date.to_string(),
            amount: b as f64,
        };
        match out.last_mut() {
            Some(last) if last.date == date => *last = point,
            _ => out.push(point),
        }
    }
    out
}

pub(crate) fn balances_list_in(data: &Path, today: NaiveDate) -> Result<MoneyBalances, String> {
    balances_list_scoped_in(data, today, None)
}

/// [`balances_list_in`] limited to one account scope (`personal` or `venture`). Accounts of any other scope
/// are skipped BEFORE their statement file is touched, so a venture's file is never opened by a personal
/// read. A scoped read never reads the FX rate (`fx` is `None`); callers that need it use `fx_get_in`.
pub(crate) fn balances_list_scoped_in(
    data: &Path,
    today: NaiveDate,
    scope: Option<&str>,
) -> Result<MoneyBalances, String> {
    let _g = lock();
    let accounts = load_accounts(data)?;
    let manual: Vec<ManualBalance> =
        read_json(&money_dir(data).join("balances.json"), MAX_STORE_BYTES)
            .map_err(|_| "store_error".to_string())?;
    let cutoff = (today - chrono::Duration::days(HISTORY_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let mut balances = Vec::new();
    for a in &accounts {
        if scope.is_some_and(|s| a.scope != s) {
            continue;
        }
        if a.route == "csv" {
            if !safe_id(&a.id) {
                return Err("store_error".to_string());
            }
            let file = money_dir(data).join("tx").join(format!("{}.jsonl", a.id));
            let records = parse_tx_records(&read_tx_text(&file)?)?;
            if records.len() > MAX_ROWS {
                return Err("store_error".to_string());
            }
            let points = statement_points(&records);
            if let Some(last) = points.last() {
                balances.push(AccountBalance {
                    account_id: a.id.clone(),
                    currency: a.currency.clone(),
                    amount: last.amount,
                    as_of: last.date.clone(),
                    source: "statement".to_string(),
                    history: points
                        .iter()
                        .filter(|p| p.date >= cutoff)
                        .cloned()
                        .collect(),
                });
            }
        } else if let Some(m) = manual.iter().find(|m| m.account_id == a.id) {
            if !m.native_amount.is_finite() {
                continue;
            }
            balances.push(AccountBalance {
                account_id: a.id.clone(),
                currency: a.currency.clone(),
                amount: m.native_amount,
                as_of: m.as_of.clone(),
                source: "manual".to_string(),
                history: if m.as_of >= cutoff {
                    vec![BalancePoint {
                        date: m.as_of.clone(),
                        amount: m.native_amount,
                    }]
                } else {
                    Vec::new()
                },
            });
        }
    }
    let fx = if scope.is_some() {
        None
    } else {
        fx_get_in(data)?.filter(|r| valid_rate(r.rate))
    };
    Ok(MoneyBalances { balances, fx })
}

// ───────────────────────────── statement rows for the read-only summary ─────────────────────────────

/// One imported statement row, trimmed to what the summary needs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StmtRow {
    pub date: String,
    pub amount: i64,
    pub description: String,
}

/// One personal CSV account's imported rows (`money_summary.rs` reads these; nothing here writes).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StmtAccount {
    pub id: String,
    pub kind: String,
    pub currency: String,
    pub rows: Vec<StmtRow>,
}

/// Imported rows of every PERSONAL csv account. Venture and non-csv accounts are skipped before any file is
/// opened, so a venture statement is never read by a personal summary.
pub(crate) fn personal_statements_in(data: &Path) -> Result<Vec<StmtAccount>, String> {
    let _g = lock();
    let mut out = Vec::new();
    for a in load_accounts(data)? {
        if a.scope != "personal" || a.route != "csv" {
            continue;
        }
        if !safe_id(&a.id) {
            return Err("store_error".to_string());
        }
        let file = money_dir(data).join("tx").join(format!("{}.jsonl", a.id));
        let records = parse_tx_records(&read_tx_text(&file)?)?;
        if records.len() > MAX_ROWS {
            return Err("store_error".to_string());
        }
        out.push(StmtAccount {
            id: a.id,
            kind: a.kind,
            currency: a.currency,
            rows: records
                .into_iter()
                .map(|r| StmtRow {
                    date: r.date,
                    amount: r.amount,
                    description: r.description,
                })
                .collect(),
        });
    }
    Ok(out)
}

// ───────────────────────────── commands (all read-only) ─────────────────────────────

fn data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

#[tauri::command]
#[specta::specta]
pub async fn money_accounts_list() -> Result<Vec<MoneyAccount>, String> {
    blocking(JoinErr::Code("store_error"), || {
        accounts_list_in(&data_dir())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn money_balances_list() -> Result<MoneyBalances, String> {
    blocking(JoinErr::Code("store_error"), || {
        balances_list_in(&data_dir(), now_jst().date_naive())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn money_account_add(draft: MoneyAccountDraft) -> Result<MoneyAccount, String> {
    blocking(JoinErr::Code("store_error"), move || {
        account_add_in(&data_dir(), &draft)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn money_watch_add(account_id: String, value: String) -> Result<WatchAdded, String> {
    blocking(JoinErr::Code("store_error"), move || {
        watch_add_in(&data_dir(), &account_id, &value)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn money_balance_set_manual(
    account_id: String,
    native_amount: f64,
    as_of: String,
) -> Result<(), String> {
    blocking(JoinErr::Code("store_error"), move || {
        balance_set_manual_in(&data_dir(), &account_id, native_amount, &as_of)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn fx_rate_get() -> Result<Option<FxRate>, String> {
    blocking(JoinErr::Code("store_error"), || fx_get_in(&data_dir())).await
}

#[tauri::command]
#[specta::specta]
pub async fn fx_rate_set_manual(rate: f64, date: String) -> Result<FxRate, String> {
    blocking(JoinErr::Code("store_error"), move || {
        fx_set_manual_in(&data_dir(), rate, &date)
    })
    .await
}

#[tauri::command]
pub async fn fx_rate_refresh() -> Result<FxRate, String> {
    let disabled = super::telegram::outbound_disabled();
    let navy = super::telegram::local_only_on();
    let data = data_dir();
    fx_refresh_in(&data, disabled, navy, now_jst(), fetch_ecb).await
}

#[cfg(test)]
mod tests;
