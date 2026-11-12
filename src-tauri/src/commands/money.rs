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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Serialize, PartialEq)]
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
struct ParsedTx {
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

// ───────────────────────────── FX reference rate ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BalancePoint {
    pub date: String,
    pub amount: f64,
}

/// One account's current balance. Only accounts with a real stored value appear; nothing is estimated.
#[derive(Debug, Clone, Serialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, PartialEq)]
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

// ───────────────────────────── commands (all read-only) ─────────────────────────────

fn data_dir() -> PathBuf {
    crate::app_paths::data_dir()
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| "store_error".to_string())?
}

#[tauri::command]
pub async fn money_accounts_list() -> Result<Vec<MoneyAccount>, String> {
    blocking(|| accounts_list_in(&data_dir())).await
}

#[tauri::command]
pub async fn money_balances_list() -> Result<MoneyBalances, String> {
    blocking(|| balances_list_in(&data_dir(), now_jst().date_naive())).await
}

#[tauri::command]
pub async fn money_account_add(draft: MoneyAccountDraft) -> Result<MoneyAccount, String> {
    blocking(move || account_add_in(&data_dir(), &draft)).await
}

#[tauri::command]
pub async fn money_watch_add(account_id: String, value: String) -> Result<WatchAdded, String> {
    blocking(move || watch_add_in(&data_dir(), &account_id, &value)).await
}

#[tauri::command]
pub async fn money_balance_set_manual(
    account_id: String,
    native_amount: f64,
    as_of: String,
) -> Result<(), String> {
    blocking(move || balance_set_manual_in(&data_dir(), &account_id, native_amount, &as_of)).await
}

#[tauri::command]
pub async fn money_import_statement(
    account_id: String,
    path: String,
) -> Result<ImportSummary, String> {
    blocking(move || import_statement_in(&data_dir(), &account_id, Path::new(&path))).await
}

#[tauri::command]
pub async fn fx_rate_get() -> Result<Option<FxRate>, String> {
    blocking(|| fx_get_in(&data_dir())).await
}

#[tauri::command]
pub async fn fx_rate_set_manual(rate: f64, date: String) -> Result<FxRate, String> {
    blocking(move || fx_set_manual_in(&data_dir(), rate, &date)).await
}

#[tauri::command]
pub async fn fx_rate_refresh() -> Result<FxRate, String> {
    let disabled = super::telegram::outbound_disabled();
    let navy = super::telegram::local_only_on();
    let data = data_dir();
    fx_refresh_in(&data, disabled, navy, now_jst(), fetch_ecb).await
}

#[cfg(test)]
mod tests {
    use super::super::local_store::tests::temp_dir;
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    const PHRASE_12: &str =
        "abandon ability able about above absent absorb abstract absurd abuse access accident";
    const PHRASE_JA: &str = "あいこくしん あいさつ あいだ あおぞら あかちゃん あきる あけがた あける あこがれる あさい あさひ あしあと";

    fn xpub() -> String {
        format!("xpub6{}", "C".repeat(106))
    }

    // ── seed guard (ported from src-console/money/money.test.tsx) ──

    #[test]
    fn guard_refuses_phrases_and_private_keys() {
        let p24 = format!("{PHRASE_12} {PHRASE_12}");
        for s in [
            PHRASE_12.to_string(),
            p24,
            PHRASE_JA.to_string(),
            format!("  {}\n", PHRASE_12.to_uppercase()),
            PHRASE_JA.replace(' ', "\u{3000}"),
        ] {
            assert_eq!(guard_input(&s), Guard::Secret(SecretKind::Phrase));
        }
        for s in [
            format!("xprv9{}", "s".repeat(106)),
            format!("5{}", "H".repeat(50)),
            "a".repeat(64),
            format!("0x{}", "b".repeat(64)),
        ] {
            assert_eq!(guard_input(&s), Guard::Secret(SecretKind::PrivateKey));
        }
    }

    #[test]
    fn guard_accepts_public_values_only() {
        assert_eq!(guard_input(&xpub()), Guard::Public(Chain::Btc, Form::Xpub));
        for a in [
            "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
            "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa",
            "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
            "BC1QAR0SRRR7XFKVY5L643LYDNW9RE59GTZZWF5MDQ",
        ] {
            assert_eq!(
                guard_input(a),
                Guard::Public(Chain::Btc, Form::Address),
                "{a}"
            );
        }
        assert_eq!(
            guard_input(&format!("0x{}", "a".repeat(40))),
            Guard::Public(Chain::Eth, Form::Address)
        );
        assert_eq!(guard_input("hello there"), Guard::Invalid);
        assert_eq!(guard_input("   "), Guard::Empty);
        // Ten words is not a phrase; eleven is.
        assert_eq!(guard_input(&["word"; 10].join(" ")), Guard::Invalid);
        assert_eq!(
            guard_input(&["word"; 11].join(" ")),
            Guard::Secret(SecretKind::Phrase)
        );
        assert_eq!(
            guard_input(&format!("zpub{}", "D".repeat(108))),
            Guard::Public(Chain::Btc, Form::Xpub)
        );
    }

    // ── accounts ──

    fn draft(nick: &str) -> MoneyAccountDraft {
        MoneyAccountDraft {
            nickname: nick.into(),
            institution: "smbc".into(),
            kind: "ordinary".into(),
            route: "csv".into(),
            currency: "JPY".into(),
            scope: "personal".into(),
        }
    }

    fn ledger_draft(route: &str, cur: &str) -> MoneyAccountDraft {
        MoneyAccountDraft {
            nickname: "Cold".into(),
            institution: "ledger".into(),
            kind: "btc".into(),
            route: route.into(),
            currency: cur.into(),
            scope: "personal".into(),
        }
    }

    #[test]
    fn account_add_validates_and_lists() {
        let d = temp_dir("money-acct");
        let a = account_add_in(&d, &draft("  Main  ")).unwrap();
        assert_eq!(a.nickname, "Main");
        assert!(a.id.starts_with("acct-"));
        assert_eq!(accounts_list_in(&d).unwrap(), vec![a]);
        for bad in [
            MoneyAccountDraft {
                institution: "paypal".into(),
                ..draft("x")
            },
            MoneyAccountDraft {
                kind: "x".into(),
                ..draft("x")
            },
            MoneyAccountDraft {
                route: "api".into(),
                ..draft("x")
            },
            MoneyAccountDraft {
                currency: "EUR".into(),
                ..draft("x")
            },
            MoneyAccountDraft {
                scope: "all".into(),
                ..draft("x")
            },
            MoneyAccountDraft {
                institution: "chime".into(),
                route: "csv".into(),
                currency: "USD".into(),
                ..draft("x")
            },
            MoneyAccountDraft {
                currency: "BTC".into(),
                ..draft("x")
            },
            draft(""),
            draft(&format!(
                "{} {} {}",
                "a".repeat(20),
                "b".repeat(20),
                "c".repeat(20)
            )),
        ] {
            assert_eq!(account_add_in(&d, &bad).unwrap_err(), "invalid_input");
        }
        assert_eq!(accounts_list_in(&d).unwrap().len(), 1);
        assert!(account_add_in(
            &d,
            &draft(&format!(
                "{} {} {}",
                "a".repeat(19),
                "b".repeat(20),
                "c".repeat(19)
            ))
        )
        .is_ok());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn phrase_nickname_is_refused_without_echo() {
        let d = temp_dir("money-nick");
        let err = account_add_in(&d, &draft(PHRASE_12)).unwrap_err();
        assert_eq!(err, "refused_secret");
        assert!(!err.contains("abandon"));
        assert_eq!(
            account_add_in(&d, &draft(&format!("5{}", "H".repeat(50)))).unwrap_err(),
            "refused_secret"
        );
        assert!(accounts_list_in(&d).unwrap().is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn watch_add_stores_public_only() {
        let d = temp_dir("money-watch");
        let acct = account_add_in(&d, &ledger_draft("watch_only", "BTC")).unwrap();
        let added = watch_add_in(&d, &acct.id, &format!("  {}  ", xpub())).unwrap();
        assert_eq!((added.chain.as_str(), added.form.as_str()), ("btc", "xpub"));
        // Same value twice: stored once.
        watch_add_in(&d, &acct.id, &xpub()).unwrap();
        let w: Vec<WatchEntry> = read_json(&d.join("money/watch.json"), MAX_STORE_BYTES).unwrap();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].value, xpub());
        // Secrets: fixed error, nothing more written.
        let before = fs::read_to_string(d.join("money/watch.json")).unwrap();
        for s in [
            PHRASE_12.to_string(),
            format!("xprv9{}", "s".repeat(106)),
            "c".repeat(64),
        ] {
            assert_eq!(
                watch_add_in(&d, &acct.id, &s).unwrap_err(),
                "refused_secret"
            );
        }
        assert_eq!(
            watch_add_in(&d, &acct.id, "hello").unwrap_err(),
            "invalid_input"
        );
        // An ETH address on a BTC account, and an unknown account, are refused.
        assert_eq!(
            watch_add_in(&d, &acct.id, &format!("0x{}", "a".repeat(40))).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            watch_add_in(&d, "acct-nope", &xpub()).unwrap_err(),
            "not_found"
        );
        assert_eq!(
            fs::read_to_string(d.join("money/watch.json")).unwrap(),
            before
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn manual_balance_only_for_ledger_or_manual_accounts() {
        let d = temp_dir("money-bal");
        let ledger = account_add_in(&d, &ledger_draft("manual", "BTC")).unwrap();
        let bank = account_add_in(&d, &draft("Main")).unwrap();
        balance_set_manual_in(&d, &ledger.id, 0.5, "2026-11-12").unwrap();
        balance_set_manual_in(&d, &ledger.id, 0.75, "2026-11-13").unwrap();
        let all: Vec<ManualBalance> =
            read_json(&d.join("money/balances.json"), MAX_STORE_BYTES).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(
            (all[0].native_amount, all[0].source.as_str()),
            (0.75, "manual")
        );
        for (id, amt, day) in [
            (bank.id.as_str(), 1.0, "2026-11-12"),
            (ledger.id.as_str(), f64::NAN, "2026-11-12"),
            (ledger.id.as_str(), f64::INFINITY, "2026-11-12"),
            (ledger.id.as_str(), 1.0, "2026-13-40"),
            (ledger.id.as_str(), 1.0, "yesterday"),
        ] {
            assert_eq!(
                balance_set_manual_in(&d, id, amt, day).unwrap_err(),
                "invalid_input"
            );
        }
        assert_eq!(
            balance_set_manual_in(&d, "acct-nope", 1.0, "2026-11-12").unwrap_err(),
            "not_found"
        );
        let _ = fs::remove_dir_all(d);
    }

    // ── statement import ──
    //
    // ASSUMED FORMATS. The real SMBC and Japan Post Bank (Yucho) statement CSVs are UNVERIFIED (spike §2:
    // no primary spec was found). Every fixture below is a synthetic file built in the test from the
    // column names in the task, hence the `assumed_` prefix. REPLACE these with Taisun's redacted
    // sample files for each bank before trusting the importer on real statements.

    const SPLIT_HEADER: &str = "年月日,お引出し,お預入れ,お取り扱い内容,残高\n";

    fn csv_account(d: &Path) -> MoneyAccount {
        account_add_in(d, &draft("SMBC main")).unwrap()
    }

    fn write_file(d: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = d.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    fn tx_file(d: &Path, id: &str) -> PathBuf {
        d.join("money/tx").join(format!("{id}.jsonl"))
    }

    fn txs_of(d: &Path, id: &str) -> Vec<TxRecord> {
        parse_tx_records(&read_tx_text(&tx_file(d, id)).unwrap()).unwrap()
    }

    #[test]
    fn assumed_utf8_bom_split_columns() {
        let d = temp_dir("money-bom");
        let a = csv_account(&d);
        let body = format!(
            "\u{feff}口座情報,0000\n{SPLIT_HEADER}2026/11/02,\"3,000\",,ｺﾝﾋﾞﾆ,\"97,000\"\n2026/11/03,,\"50,000\",給与,\"147,000\"\n,,,,\n合計,,,,\n"
        );
        let p = write_file(&d, "s.csv", body.as_bytes());
        let r = import_statement_in(&d, &a.id, &p).unwrap();
        assert_eq!(
            r,
            ImportSummary {
                added: 2,
                duplicates: 0,
                skipped_rows: 1
            }
        );
        let t = txs_of(&d, &a.id);
        assert_eq!(
            (t[0].date.as_str(), t[0].amount, t[0].balance),
            ("2026-11-02", -3000, Some(97000))
        );
        assert_eq!((t[1].amount, t[1].description.as_str()), (50000, "給与"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn assumed_cp932_bytes() {
        let d = temp_dir("money-sjis");
        let a = csv_account(&d);
        let text = format!("{SPLIT_HEADER}2026/11/02,\"1,200\",,スーパー,\"8,800\"\n");
        let (bytes, _, bad) = encoding_rs::SHIFT_JIS.encode(&text);
        assert!(!bad);
        assert!(std::str::from_utf8(&bytes).is_err());
        let p = write_file(&d, "s.csv", &bytes);
        let r = import_statement_in(&d, &a.id, &p).unwrap();
        assert_eq!(r.added, 1);
        let t = txs_of(&d, &a.id);
        assert_eq!(
            (t[0].amount, t[0].description.as_str()),
            (-1200, "スーパー")
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn assumed_era_dates_and_fullwidth_digits() {
        assert_eq!(parse_date("令和6年1月5日").as_deref(), Some("2024-01-05"));
        assert_eq!(parse_date("令和元年5月1日").as_deref(), Some("2019-05-01"));
        assert_eq!(parse_date("R6.01.05").as_deref(), Some("2024-01-05"));
        assert_eq!(
            parse_date("平成30年12月31日").as_deref(),
            Some("2018-12-31")
        );
        assert_eq!(parse_date("2026/11/2").as_deref(), Some("2026-11-02"));
        assert_eq!(parse_date("2026-11-02").as_deref(), Some("2026-11-02"));
        assert_eq!(parse_date("20261102").as_deref(), Some("2026-11-02"));
        assert_eq!(
            parse_date("２０２６／１１／０２").as_deref(),
            Some("2026-11-02")
        );
        assert_eq!(
            parse_date("令和８年１１月２日").as_deref(),
            Some("2026-11-02")
        );
        assert_eq!(parse_date("2026/13/02"), None);
        assert_eq!(parse_date("hello"), None);
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_yen("１２，３４５円"), Ok(Some(12345)));
        assert_eq!(parse_yen("¥1,000"), Ok(Some(1000)));
        assert_eq!(parse_yen("△500"), Ok(Some(-500)));
        assert_eq!(parse_yen("-７"), Ok(Some(-7)));
        assert_eq!(parse_yen(""), Ok(None));
        assert_eq!(parse_yen("12.5"), Err(()));
        assert_eq!(parse_yen("abc"), Err(()));

        let d = temp_dir("money-era");
        let a = csv_account(&d);
        let body = format!(
            "{SPLIT_HEADER}令和8年11月2日,\"１，２００\",,店,\"９，８００円\"\nR8.11.03,,5000,入金,14800\n"
        );
        let p = write_file(&d, "s.csv", body.as_bytes());
        assert_eq!(import_statement_in(&d, &a.id, &p).unwrap().added, 2);
        let t = txs_of(&d, &a.id);
        assert_eq!(
            (t[0].date.as_str(), t[0].amount, t[0].balance),
            ("2026-11-02", -1200, Some(9800))
        );
        assert_eq!((t[1].date.as_str(), t[1].amount), ("2026-11-03", 5000));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn assumed_signed_amount_with_and_without_kubun() {
        let d = temp_dir("money-signed");
        let a = csv_account(&d);
        let signed =
            "日付,取引金額,差引残高,摘要\n2026/11/02,-1500,98500,A\n2026/11/03,2500,101000,B\n";
        let p = write_file(&d, "a.csv", signed.as_bytes());
        assert_eq!(import_statement_in(&d, &a.id, &p).unwrap().added, 2);
        let t = txs_of(&d, &a.id);
        assert_eq!((t[0].amount, t[1].amount), (-1500, 2500));

        let b = account_add_in(&d, &draft("Yucho")).unwrap();
        let kubun = "お取引日,金額,受払区分,残高,内容\n2026/11/02,\"1,500\",支払,98500,A\n2026/11/03,\"2,500\",入金,101000,B\n2026/11/04,300,不明,101000,C\n";
        let p = write_file(&d, "b.csv", kubun.as_bytes());
        let r = import_statement_in(&d, &b.id, &p).unwrap();
        assert_eq!((r.added, r.skipped_rows), (2, 1));
        let t = txs_of(&d, &b.id);
        assert_eq!((t[0].amount, t[1].amount), (-1500, 2500));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn assumed_unknown_header_is_refused_and_writes_nothing() {
        let d = temp_dir("money-unk");
        let a = csv_account(&d);
        for body in [
            "col1,col2,col3\n2026/11/02,100,200\n",
            "年月日,メモ\n2026/11/02,x\n",
            "年月日,お引出し,お引出し,お預入れ\n2026/11/02,1,2,3\n",
            "",
        ] {
            let p = write_file(&d, "u.csv", body.as_bytes());
            assert_eq!(
                import_statement_in(&d, &a.id, &p).unwrap_err(),
                "unrecognised_format"
            );
        }
        // Bytes that are neither UTF-8 nor Shift-JIS.
        let p = write_file(&d, "u.csv", &[0x81, 0x20, 0xff, 0xfe, 0xfd]);
        assert_eq!(
            import_statement_in(&d, &a.id, &p).unwrap_err(),
            "unrecognised_format"
        );
        assert!(!d.join("money/tx").exists());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn assumed_reimport_dedupes_and_same_day_twins_are_kept() {
        let d = temp_dir("money-dedupe");
        let a = csv_account(&d);
        let body = format!(
            "{SPLIT_HEADER}2026/11/02,500,,コーヒー,9500\n2026/11/02,500,,コーヒー,9500\n2026/11/03,,1000,入金,10500\n"
        );
        let p = write_file(&d, "s.csv", body.as_bytes());
        assert_eq!(
            import_statement_in(&d, &a.id, &p).unwrap(),
            ImportSummary {
                added: 3,
                duplicates: 0,
                skipped_rows: 0
            }
        );
        let before = fs::read_to_string(tx_file(&d, &a.id)).unwrap();
        assert_eq!(
            import_statement_in(&d, &a.id, &p).unwrap(),
            ImportSummary {
                added: 0,
                duplicates: 3,
                skipped_rows: 0
            }
        );
        assert_eq!(fs::read_to_string(tx_file(&d, &a.id)).unwrap(), before);
        // An overlapping later file adds only the new row.
        let more =
            format!("{SPLIT_HEADER}2026/11/03,,1000,入金,10500\n2026/11/04,200,,バス,10300\n");
        let p2 = write_file(&d, "s2.csv", more.as_bytes());
        let r = import_statement_in(&d, &a.id, &p2).unwrap();
        assert_eq!((r.added, r.duplicates), (1, 1));
        let after = fs::read_to_string(tx_file(&d, &a.id)).unwrap();
        assert!(after.starts_with(&before));
        assert_eq!(txs_of(&d, &a.id).len(), 4);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn import_refuses_symlink_directory_oversize_relative_and_wrong_route() {
        let d = temp_dir("money-path");
        let a = csv_account(&d);
        let real = write_file(
            &d,
            "real.csv",
            format!("{SPLIT_HEADER}2026/11/02,1,,x,2\n").as_bytes(),
        );
        #[cfg(unix)]
        {
            let link = d.join("link.csv");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert_eq!(
                import_statement_in(&d, &a.id, &link).unwrap_err(),
                "bad_file"
            );
        }
        assert_eq!(import_statement_in(&d, &a.id, &d).unwrap_err(), "bad_file");
        assert_eq!(
            import_statement_in(&d, &a.id, &d.join("missing.csv")).unwrap_err(),
            "bad_file"
        );
        assert_eq!(
            import_statement_in(&d, &a.id, Path::new("real.csv")).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            import_statement_in(&d, &a.id, &d.join("..").join("real.csv")).unwrap_err(),
            "invalid_input"
        );
        let mut big = SPLIT_HEADER.as_bytes().to_vec();
        big.resize(MAX_IMPORT_BYTES as usize + 1, b'x');
        let p = write_file(&d, "big.csv", &big);
        assert_eq!(import_statement_in(&d, &a.id, &p).unwrap_err(), "too_large");
        let ledger = account_add_in(&d, &ledger_draft("manual", "BTC")).unwrap();
        assert_eq!(
            import_statement_in(&d, &ledger.id, &real).unwrap_err(),
            "invalid_input"
        );
        assert_eq!(
            import_statement_in(&d, "acct-nope", &real).unwrap_err(),
            "not_found"
        );
        assert!(!d.join("money/tx").exists());
        let _ = fs::remove_dir_all(d);
    }

    // ── seed sentinel: never on disk, never in an error ──

    fn all_file_bytes(dir: &Path, out: &mut Vec<u8>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                all_file_bytes(&p, out);
            } else {
                out.extend(fs::read(&p).unwrap());
                out.push(b'\n');
            }
        }
    }

    #[test]
    fn seed_sentinel_never_reaches_disk_or_errors() {
        let words = [
            "zorblax", "quindle", "marvomp", "tessiqua", "flunder", "gorpwick", "yamblet",
            "crundle", "wopsel", "drivane", "plimzot", "hexabar",
        ];
        let phrase = words.join(" ");
        let data = temp_dir("money-sentinel");
        let acct = account_add_in(&data, &ledger_draft("watch_only", "BTC")).unwrap();
        let bank = csv_account(&data);
        let mut errors: Vec<String> = vec![
            account_add_in(&data, &draft(&phrase)).unwrap_err(),
            account_add_in(
                &data,
                &MoneyAccountDraft {
                    institution: phrase.clone(),
                    ..draft("x")
                },
            )
            .unwrap_err(),
            watch_add_in(&data, &acct.id, &phrase).unwrap_err(),
            watch_add_in(&data, &phrase, &xpub()).unwrap_err(),
            balance_set_manual_in(&data, &acct.id, 1.0, &phrase).unwrap_err(),
            fx_set_manual_in(&data, 150.0, &phrase).unwrap_err(),
        ];
        // A statement whose description holds the phrase: the field is replaced, the row is kept.
        let body = format!(
            "{SPLIT_HEADER}2026/11/02,100,,\"{phrase}\",900\n2026/11/03,200,,{},700\n",
            phrase.to_uppercase()
        );
        let src_dir = temp_dir("money-sentinel-src");
        let p = write_file(&src_dir, "src.csv", body.as_bytes());
        let r = import_statement_in(&data, &bank.id, &p).unwrap();
        assert_eq!(r.added, 2);
        assert!(txs_of(&data, &bank.id)
            .iter()
            .all(|x| x.description == REMOVED));
        errors.push(import_statement_in(&data, &bank.id, &data.join(&phrase)).unwrap_err());
        for e in &errors {
            for w in words {
                assert!(!e.to_lowercase().contains(w), "error echoed input: {e}");
            }
        }
        // Every file written under the data dir (the fixture lives outside it).
        let mut bytes = Vec::new();
        all_file_bytes(&data, &mut bytes);
        let blob = String::from_utf8_lossy(&bytes).to_lowercase();
        for w in words {
            assert!(!blob.contains(w), "sentinel word reached disk");
        }
        let _ = fs::remove_dir_all(data);
        let _ = fs::remove_dir_all(src_dir);
    }

    // ── modes ──

    #[cfg(unix)]
    #[test]
    fn money_files_are_0600_and_dirs_0700() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp_dir("money-modes");
        let bank = csv_account(&d);
        let ledger = account_add_in(&d, &ledger_draft("watch_only", "BTC")).unwrap();
        watch_add_in(&d, &ledger.id, &xpub()).unwrap();
        balance_set_manual_in(&d, &ledger.id, 1.0, "2026-11-12").unwrap();
        fx_set_manual_in(&d, 150.5, "2026-11-12").unwrap();
        let p = write_file(
            &d,
            "s.csv",
            format!("{SPLIT_HEADER}2026/11/02,1,,x,2\n").as_bytes(),
        );
        import_statement_in(&d, &bank.id, &p).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&d.join("money")), 0o700);
        assert_eq!(mode(&d.join("money/tx")), 0o700);
        for f in ["accounts.json", "watch.json", "balances.json", "fx.json"] {
            assert_eq!(mode(&d.join("money").join(f)), 0o600, "{f}");
        }
        assert_eq!(mode(&tx_file(&d, &bank.id)), 0o600);
        let _ = fs::remove_dir_all(d);
    }

    // ── FX ──

    const ECB_FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope><gesmes:subject>Reference rates</gesmes:subject>
<Cube>
  <Cube time='2026-11-12'>
    <Cube currency='USD' rate='1.0000'/>
    <Cube currency='JPY' rate='155.50'/>
    <Cube currency='BGN' rate='1.9558'/>
  </Cube>
</Cube></gesmes:Envelope>"#;

    fn cube(body: &str) -> String {
        format!("<Cube><Cube time='2026-11-12'>{body}</Cube></Cube>")
    }

    #[test]
    fn ecb_parse_cross_rate_and_errors() {
        let (date, rate) = parse_ecb_daily(ECB_FIXTURE).unwrap();
        assert_eq!((date.as_str(), rate), ("2026-11-12", 155.5));
        let (_, r2) = parse_ecb_daily(
            r#"<Cube><Cube time="2026-11-12"><Cube currency="USD" rate="1.1620"/><Cube currency="JPY" rate="177.12"/></Cube></Cube>"#,
        )
        .unwrap();
        assert_eq!(r2, ((177.12f64 / 1.1620) * 10_000.0).round() / 10_000.0);
        let usd = |r: &str| format!("<Cube currency='USD' rate='{r}'/>");
        let jpy = |r: &str| format!("<Cube currency='JPY' rate='{r}'/>");
        for bad in [
            String::new(),
            cube(&jpy("155.5")),
            cube(&usd("1.0")),
            cube(&format!("{}{}", usd("0"), jpy("155"))),
            cube(&format!("{}{}", usd("x"), jpy("155"))),
            cube(&format!("{}{}", usd("0.0001"), jpy("155"))),
            format!("<Cube>{}{}</Cube>", usd("1.0"), jpy("155")),
            cube(&format!("{}{}", usd("1.0"), jpy("155"))).replace("2026-11-12", "soon"),
        ] {
            assert_eq!(
                parse_ecb_daily(&bad).unwrap_err(),
                "ecb_parse_failed",
                "{bad}"
            );
        }
    }

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn refresh_is_blocked_by_kill_switch_and_navy_before_any_fetch() {
        let d = temp_dir("money-fxblock");
        for (off, navy) in [(true, false), (false, true), (true, true)] {
            let called = AtomicBool::new(false);
            let r = block_on(fx_refresh_in(&d, off, navy, now_jst(), || async {
                called.store(true, Ordering::SeqCst);
                Ok(ECB_FIXTURE.to_string())
            }));
            assert_eq!(r.unwrap_err(), "outbound_disabled");
            assert!(!called.load(Ordering::SeqCst), "fetch ran while blocked");
        }
        assert_eq!(fx_get_in(&d).unwrap(), None);
        assert!(!d.join("money").exists());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn refresh_stores_ecb_rate_and_failures_store_nothing() {
        let d = temp_dir("money-fxok");
        assert_eq!(fx_get_in(&d).unwrap(), None);
        let t0 = now_jst();
        let later = t0 + chrono::Duration::minutes(11);
        let r = block_on(fx_refresh_in(&d, false, false, t0, || async {
            Ok(ECB_FIXTURE.to_string())
        }))
        .unwrap();
        assert_eq!(
            (r.rate, r.date.as_str(), r.source.as_str()),
            (155.5, "2026-11-12", "ecb")
        );
        assert_eq!(fx_get_in(&d).unwrap(), Some(r.clone()));
        // A failed fetch or a bad body leaves the stored rate as it was.
        let e = block_on(fx_refresh_in(&d, false, false, later, || async {
            Err("fetch_failed".to_string())
        }))
        .unwrap_err();
        assert_eq!(e, "fetch_failed");
        let e = block_on(fx_refresh_in(&d, false, false, later, || async {
            Ok("<Cube/>".to_string())
        }))
        .unwrap_err();
        assert_eq!(e, "ecb_parse_failed");
        assert_eq!(fx_get_in(&d).unwrap(), Some(r));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn l4_refresh_waits_ten_minutes_after_a_successful_fetch() {
        let d = temp_dir("money-fxgap");
        let t0 = now_jst();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let run = |now| {
            block_on(fx_refresh_in(&d, false, false, now, || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ECB_FIXTURE.to_string())
            }))
        };
        let first = run(t0).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // Inside the window: the stored rate comes back and nothing is fetched.
        let again = run(t0 + chrono::Duration::minutes(9)).unwrap();
        assert_eq!(again, first);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // After it: a fetch.
        run(t0 + chrono::Duration::minutes(10)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        // A manual rate never blocks a refresh, and the kill switch still wins inside the window.
        fx_set_manual_in(&d, 150.0, "2026-11-12").unwrap();
        run(t0 + chrono::Duration::minutes(11)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let blocked = block_on(fx_refresh_in(&d, true, false, t0, || async {
            Ok(String::new())
        }));
        assert_eq!(blocked.unwrap_err(), "outbound_disabled");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn l4_rows_past_the_cap_are_counted_and_wide_rows_skipped() {
        let mut text = String::from(SPLIT_HEADER);
        for _ in 0..(MAX_ROWS + 6) {
            text.push_str("2026/11/02,1,,x,2\n");
        }
        let (rows, overflow) = parse_csv(&text);
        assert_eq!((rows.len(), overflow), (MAX_ROWS, 7));
        let (txs, skipped) = parse_statement(text.as_bytes()).unwrap();
        assert_eq!((txs.len(), skipped), (MAX_ROWS - 1, 7));
        // More than 64 cells: the row is skipped, and a wide row is never a header.
        let wide = vec!["1"; MAX_CELLS + 1].join(",");
        let body = format!("{wide}\n{SPLIT_HEADER}2026/11/02,1,,x,2\n2026/11/03,1,,x,2,{wide}\n");
        let (txs, skipped) = parse_statement(body.as_bytes()).unwrap();
        assert_eq!((txs.len(), skipped), (1, 1));
        let ok64 = vec!["1"; MAX_CELLS].join(",");
        let (rows, _) = parse_csv(&format!("{ok64}\n"));
        assert_eq!(rows[0].len(), MAX_CELLS);
    }

    #[cfg(unix)]
    #[test]
    fn l2_a_fifo_is_refused_without_hanging() {
        let d = temp_dir("money-fifo");
        let fifo = d.join("pipe.csv");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        // The open itself must not block and must be refused on the handle.
        assert_eq!(open_regular_nofollow(&fifo).unwrap_err(), "bad_file");
        let a = csv_account(&d);
        assert_eq!(
            import_statement_in(&d, &a.id, &fifo).unwrap_err(),
            "bad_file"
        );
        // A regular file still opens.
        let ok = write_file(&d, "ok.csv", b"x");
        assert!(open_regular_nofollow(&ok).is_ok());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn manual_rate_bounds() {
        let d = temp_dir("money-fxman");
        let r = fx_set_manual_in(&d, 151.25, "2026-11-12").unwrap();
        assert_eq!((r.source.as_str(), r.rate), ("manual", 151.25));
        assert_eq!(fx_get_in(&d).unwrap(), Some(r));
        for rate in [0.0, -1.0, 1000.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                fx_set_manual_in(&d, rate, "2026-11-12").unwrap_err(),
                "invalid_input"
            );
        }
        assert_eq!(
            fx_set_manual_in(&d, 150.0, "11/12").unwrap_err(),
            "invalid_input"
        );
        assert_eq!(fx_get_in(&d).unwrap().unwrap().rate, 151.25);
        let _ = fs::remove_dir_all(d);
    }

    // ── security review fixes (M1, M2, L1, L4) ──

    const SHORT: [&str; 12] = [
        "zoo", "act", "add", "age", "air", "all", "and", "arm", "art", "ask", "bag", "bar",
    ];

    fn bypass_phrases() -> Vec<(&'static str, String)> {
        let w = SHORT;
        let accented = [
            "ábaco", "ñandú", "épico", "óleo", "útil", "íntimo", "ñoño", "año", "café", "señal",
            "avión", "árbol",
        ];
        let zh = [
            "的", "一", "是", "在", "不", "了", "有", "和", "人", "这", "中", "大",
        ];
        let ko = [
            "가격",
            "가끔",
            "가난",
            "가능",
            "가득",
            "가르침",
            "가방",
            "가위",
            "가을",
            "가이드",
            "가입",
            "가족",
        ];
        let fullwidth: Vec<String> = w
            .iter()
            .map(|x| {
                x.chars()
                    .map(|c| char::from_u32(c as u32 + 0xfee0).unwrap())
                    .collect()
            })
            .collect();
        let mut stray: Vec<String> = w.iter().map(|x| x.to_string()).collect();
        stray.insert(6, "x9#7".to_string());
        vec![
            ("commas", w.join(", ")),
            ("hyphens", w.join("-")),
            ("dots", w.join(".")),
            ("underscores", w.join("_")),
            ("zwsp", w.join("\u{200b}")),
            ("word joiner", w.join("\u{2060}")),
            ("bom", w.join("\u{feff}")),
            ("zwsp plus space", w.join(" \u{200b}")),
            ("number prefix", format!("1 {}", w.join(" "))),
            ("stray token", stray.join(" ")),
            ("accented", accented.join(" ")),
            ("chinese", zh.join(" ")),
            ("korean", ko.join(" ")),
            ("fullwidth latin", fullwidth.join("\u{3000}")),
        ]
    }

    #[test]
    fn m1_phrase_bypasses_are_refused_as_nicknames_and_by_the_guard() {
        let d = temp_dir("money-m1");
        for (name, phrase) in bypass_phrases() {
            assert_eq!(
                guard_input(&phrase),
                Guard::Secret(SecretKind::Phrase),
                "guard: {name}"
            );
            let err = account_add_in(&d, &draft(&phrase)).unwrap_err();
            assert_eq!(err, "refused_secret", "nickname: {name}");
        }
        // Eight or more words is refused as a nickname even when it is not a phrase.
        assert_eq!(
            account_add_in(&d, &draft("zoo act add age air all and arm")).unwrap_err(),
            "refused_secret"
        );
        assert!(account_add_in(&d, &draft("Ledger cold storage")).is_ok());
        assert!(account_add_in(&d, &draft("Yucho_main-2 o'neil.1")).is_ok());
        for odd in ["a,b", "Main\u{200b}", "a/b", "名前#1", "x@y"] {
            assert_eq!(
                account_add_in(&d, &draft(odd)).unwrap_err(),
                "invalid_input",
                "{odd:?}"
            );
        }
        assert_eq!(accounts_list_in(&d).unwrap().len(), 2);
        let on_disk = fs::read_to_string(d.join("money/accounts.json")).unwrap();
        assert!(!on_disk.contains("zoo"));
        // The same phrases in a statement description are removed.
        let bank = csv_account(&d);
        let mut body = String::from(SPLIT_HEADER);
        for (i, (_, phrase)) in bypass_phrases().iter().enumerate() {
            body.push_str(&format!("2026/11/{:02},1,,\"{phrase}\",9\n", i + 1));
        }
        let p = write_file(&d, "b.csv", body.as_bytes());
        let r = import_statement_in(&d, &bank.id, &p).unwrap();
        assert_eq!(r.added, bypass_phrases().len());
        assert!(txs_of(&d, &bank.id)
            .iter()
            .all(|t| t.description == REMOVED));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn l1_private_key_tokens_inside_text_are_refused() {
        let wif = format!("5{}", "H".repeat(50));
        let hex = "ab".repeat(32);
        let b58 = "K".repeat(60);
        let xprv = format!("xprv9{}", "s".repeat(106));
        let d = temp_dir("money-l1");
        for text in [
            format!("my {wif}"),
            format!("key:{wif}"),
            format!("tx {hex}"),
            format!("a 0x{hex}"),
            format!("x {b58}"),
            format!("see {xprv} ok"),
        ] {
            assert_eq!(
                account_add_in(&d, &draft(&text)).unwrap_err(),
                "refused_secret",
                "{text}"
            );
            assert_eq!(clean_description(&text), REMOVED, "{text}");
        }
        assert_eq!(clean_description("Konbini 500 yen"), "Konbini 500 yen");
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn l4_a_minus_sign_only_leads() {
        assert_eq!(parse_yen("1-2"), Err(()));
        assert_eq!(parse_yen("12-"), Err(()));
        assert_eq!(parse_yen("--5"), Err(()));
        assert_eq!(parse_yen("5△"), Err(()));
        assert_eq!(parse_yen("-12"), Ok(Some(-12)));
        assert_eq!(parse_yen("△12"), Ok(Some(-12)));
        assert_eq!(parse_yen("▲1,200円"), Ok(Some(-1200)));
        assert_eq!(parse_yen(" -３ "), Ok(Some(-3)));
    }

    #[test]
    fn m2_and_l4_source_assertions() {
        let src = production_source();
        assert_eq!(src.matches(".no_proxy()").count(), 1);
        assert_eq!(src.matches(".https_only(true)").count(), 1);
        let client = src.split("async fn fetch_ecb").nth(1).unwrap();
        let client = client.split("// ──").next().unwrap();
        assert!(client.contains(".no_proxy()") && client.contains(".https_only(true)"));
    }

    // ── balances (read-only) ──

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn balances_empty_store_is_empty_with_null_fx() {
        let d = temp_dir("money-bal-empty");
        let r = balances_list_in(&d, day("2026-11-12")).unwrap();
        assert!(r.balances.is_empty());
        assert!(r.fx.is_none());
        // A read creates nothing.
        assert!(!d.join("money").exists());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn balances_manual_has_source_and_as_of_and_fx_when_stored() {
        let d = temp_dir("money-bal-manual");
        let ledger = account_add_in(&d, &ledger_draft("manual", "BTC")).unwrap();
        let empty = account_add_in(&d, &ledger_draft("manual", "ETH")).unwrap();
        balance_set_manual_in(&d, &ledger.id, 0.75, "2026-11-10").unwrap();
        let r = balances_list_in(&d, day("2026-11-12")).unwrap();
        assert!(r.fx.is_none());
        // An account with no stored balance is absent, never 0.
        assert_eq!(r.balances.len(), 1);
        assert!(r.balances.iter().all(|b| b.account_id != empty.id));
        let b = &r.balances[0];
        assert_eq!(
            (
                b.account_id.as_str(),
                b.currency.as_str(),
                b.amount,
                b.as_of.as_str(),
                b.source.as_str()
            ),
            (ledger.id.as_str(), "BTC", 0.75, "2026-11-10", "manual")
        );
        assert_eq!(b.history.len(), 1);
        fx_set_manual_in(&d, 150.5, "2026-11-11").unwrap();
        let fx = balances_list_in(&d, day("2026-11-12")).unwrap().fx.unwrap();
        assert_eq!(
            (fx.rate, fx.date.as_str(), fx.source.as_str()),
            (150.5, "2026-11-11", "manual")
        );
        // Stale manual balance: still reported, but no history point in the window.
        let r = balances_list_in(&d, day("2026-12-31")).unwrap();
        assert_eq!(r.balances[0].amount, 0.75);
        assert!(r.balances[0].history.is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn balances_statement_current_and_history_window() {
        let d = temp_dir("money-bal-stmt");
        let a = csv_account(&d);
        let text = format!(
            "{SPLIT_HEADER}2026/09/01,\"1,000\",,old,\"50,000\"\n2026/10/20,\"1,000\",,a,\"49,000\"\n2026/11/02,\"3,000\",,b,\"46,000\"\n2026/11/02,\"1,000\",,c,\"45,000\"\n2026/11/10,,\"5,000\",d,\"50,000\"\n"
        );
        let p = write_file(&d, "s.csv", text.as_bytes());
        import_statement_in(&d, &a.id, &p).unwrap();
        let r = balances_list_in(&d, day("2026-11-12")).unwrap();
        assert_eq!(r.balances.len(), 1);
        let b = &r.balances[0];
        assert_eq!(
            (
                b.amount,
                b.as_of.as_str(),
                b.source.as_str(),
                b.currency.as_str()
            ),
            (50_000.0, "2026-11-10", "statement", "JPY")
        );
        // Window is 2026-10-13..: the September row is out, same-day rows collapse to the last one.
        let pts: Vec<(&str, f64)> = b
            .history
            .iter()
            .map(|p| (p.date.as_str(), p.amount))
            .collect();
        assert_eq!(
            pts,
            vec![
                ("2026-10-20", 49_000.0),
                ("2026-11-02", 45_000.0),
                ("2026-11-10", 50_000.0)
            ]
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn balances_statement_without_running_balance_is_absent_not_zero() {
        let d = temp_dir("money-bal-nobal");
        let a = csv_account(&d);
        let p = write_file(
            &d,
            "s.csv",
            "年月日,お引出し,お預入れ,お取り扱い内容\n2026/11/02,\"1,200\",,x\n".as_bytes(),
        );
        assert_eq!(import_statement_in(&d, &a.id, &p).unwrap().added, 1);
        assert!(balances_list_in(&d, day("2026-11-12"))
            .unwrap()
            .balances
            .is_empty());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn balances_bad_fx_is_null_and_caps_are_fixed_errors() {
        let d = temp_dir("money-bal-caps");
        let a = csv_account(&d);
        let mdir = d.join("money");
        fs::create_dir_all(&mdir).unwrap();
        fs::write(
            mdir.join("fx.json"),
            r#"{"rate":0,"date":"2026-11-11","source":"ecb","fetched_at":"x"}"#,
        )
        .unwrap();
        assert!(balances_list_in(&d, day("2026-11-12"))
            .unwrap()
            .fx
            .is_none());
        // Oversize balances.json and oversize tx file are refused with a fixed code.
        fs::write(
            mdir.join("balances.json"),
            vec![b' '; MAX_STORE_BYTES as usize + 1],
        )
        .unwrap();
        assert_eq!(
            balances_list_in(&d, day("2026-11-12")).unwrap_err(),
            "store_error"
        );
        fs::write(mdir.join("balances.json"), "[]").unwrap();
        fs::create_dir_all(mdir.join("tx")).unwrap();
        let f = fs::File::create(tx_file(&d, &a.id)).unwrap();
        f.set_len(MAX_TX_FILE_BYTES + 1).unwrap();
        assert_eq!(
            balances_list_in(&d, day("2026-11-12")).unwrap_err(),
            "store_error"
        );
        // Corrupt JSON: fixed code, nothing echoed.
        fs::write(tx_file(&d, &a.id), "SECRET-ish {not json\n").unwrap();
        let e = balances_list_in(&d, day("2026-11-12")).unwrap_err();
        assert_eq!(e, "store_error");
        let _ = fs::remove_dir_all(d);
    }

    #[cfg(unix)]
    #[test]
    fn balances_read_changes_no_file_or_permission() {
        use std::os::unix::fs::PermissionsExt;
        let d = temp_dir("money-bal-perms");
        let a = csv_account(&d);
        let l = account_add_in(&d, &ledger_draft("manual", "BTC")).unwrap();
        balance_set_manual_in(&d, &l.id, 1.0, "2026-11-10").unwrap();
        fx_set_manual_in(&d, 150.0, "2026-11-11").unwrap();
        let p = write_file(
            &d,
            "s.csv",
            format!("{SPLIT_HEADER}2026/11/02,\"1,000\",,x,\"9,000\"\n").as_bytes(),
        );
        import_statement_in(&d, &a.id, &p).unwrap();
        let snap = || {
            let mut v: Vec<(PathBuf, u32, u64)> = Vec::new();
            let mut stack = vec![d.join("money")];
            while let Some(dir) = stack.pop() {
                v.push((
                    dir.clone(),
                    fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                    0,
                ));
                for e in fs::read_dir(&dir).unwrap() {
                    let e = e.unwrap().path();
                    if e.is_dir() {
                        stack.push(e);
                    } else {
                        let m = fs::metadata(&e).unwrap();
                        v.push((e, m.permissions().mode() & 0o777, m.len()));
                    }
                }
            }
            v.sort();
            v
        };
        let before = snap();
        assert!(before.iter().any(|(_, m, _)| *m == 0o600));
        balances_list_in(&d, day("2026-11-12")).unwrap();
        assert_eq!(before, snap());
        let _ = fs::remove_dir_all(d);
    }

    // ── source scans ──

    fn production_source() -> String {
        let src = include_str!("money.rs");
        let marker = concat!("#[cfg(test)]", "\nmod tests {");
        src.split(marker).next().unwrap().to_string()
    }

    #[test]
    fn no_command_is_named_like_a_money_mover() {
        let src = production_source();
        let mut names = Vec::new();
        let mut lines = src.lines();
        while let Some(l) = lines.next() {
            if l.trim() == "#[tauri::command]" {
                let f = lines.next().unwrap();
                let name = f
                    .split("fn ")
                    .nth(1)
                    .and_then(|r| r.split('(').next())
                    .unwrap()
                    .trim()
                    .to_string();
                names.push(name);
            }
        }
        assert_eq!(names.len(), 9, "{names:?}");
        for n in &names {
            for banned in [
                "transfer",
                "pay",
                "trade",
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
                "order",
                "deposit",
                "exchange",
            ] {
                assert!(!n.contains(banned), "command {n} contains {banned}");
            }
        }
    }

    #[test]
    fn ecb_is_the_only_url_and_refresh_checks_the_gates_first() {
        let src = production_source();
        let https = concat!("https", "://");
        let http = concat!("http", "://");
        assert_eq!(
            src.matches(https).count(),
            1,
            "exactly one URL in the module"
        );
        assert_eq!(src.matches(http).count(), 0);
        assert!(ECB_DAILY_URL.starts_with("https://www.ecb.europa.eu/"));
        let cmd = src.split("pub async fn fx_rate_refresh").nth(1).unwrap();
        assert!(cmd.contains("super::telegram::outbound_disabled()"));
        assert!(cmd.contains("super::telegram::local_only_on()"));
        // The gate sits before the fetch in `fx_refresh_in`, and only `fetch_ecb` builds a client.
        let gate = src.find("if outbound_disabled || local_only").unwrap();
        let body = src.find("let body = fetch().await?").unwrap();
        assert!(gate < body);
        assert_eq!(src.matches("reqwest::Client::builder()").count(), 1);
    }
}
