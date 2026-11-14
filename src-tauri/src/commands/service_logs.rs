//! `service_logs(id)`: a redacted, capped, read-only tail of one service's log for Settings > Services.
//!
//! - `id` is a system-map node id. Only `router` has a log source: its own log file
//!   `~/Library/Logs/amara-router/router.log`. Every other known id (including the n8n and postgres
//!   containers, whose logs can hold workflow and SQL content that redaction cannot recognise) returns
//!   `no_log`; anything not a known id returns `unknown_service`. No path or file name comes from IPC,
//!   and this module never starts a process.
//! - Capped: at most `MAX_LINES` lines, `MAX_LINE_CHARS` chars per line, `MAX_BYTES` bytes read.
//! - Every line is redacted before it leaves Rust (`redact_line`): secrets and tokens, `key=value`
//!   secrets, `Authorization`/`Bearer` values, URL query strings, e-mail addresses, long opaque
//!   strings, home-folder user names, control characters, and any line that mentions health values.
//! - Read-only: the file is opened for reading (never a symlink).
//!
//! Errors (fixed text): `unknown_service`, `no_log`, `not_found`, `read_failed`.

use super::service_control::KNOWN_IDS;
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const MAX_LINES: usize = 200;
pub const MAX_LINE_CHARS: usize = 300;
pub const MAX_BYTES: u64 = 64 * 1024;

pub const NO_LOG: &str = "no_log";
const NOT_FOUND: &str = "not_found";
const READ_FAILED: &str = "read_failed";
const HEALTH_HIDDEN: &str = "[line hidden: health data]";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLogTail {
    pub id: String,
    /// Oldest first, already redacted.
    pub lines: Vec<String>,
    /// True when older lines were cut by a cap.
    pub truncated: bool,
}

fn source_for(id: &str, home: &Path) -> Result<PathBuf, String> {
    if !KNOWN_IDS.contains(&id) {
        return Err(super::service_control::UNKNOWN_SERVICE.to_string());
    }
    if id == "router" {
        return Ok(home.join("Library/Logs/amara-router/router.log"));
    }
    Err(NO_LOG.to_string())
}

// ---- redaction ----

const SENSITIVE_KEYS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "apikey",
    "api_key",
    "api-key",
    "key",
    "authorization",
    "auth",
    "cookie",
    "session",
    "credential",
    "bearer",
    "signature",
    "sig",
];
/// Short health terms: matched as whole words only ("hr" must not hide "chrome" or "through").
const HEALTH_SHORT: &[&str] = &[
    "hr", "ecg", "ekg", "spo2", "steps", "step", "temp", "hrv", "bpm",
];
fn mentions_health(lower: &str) -> bool {
    HEALTH_WORDS.iter().any(|w| lower.contains(w))
        || lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| HEALTH_SHORT.contains(&w))
}
const HEALTH_WORDS: &[&str] = &[
    "readiness",
    "temperature",
    "heartrate",
    "oxygen",
    "heart",
    "bpm",
    "hrv",
    "recovery",
    "sleep",
    "strain",
    "spo2",
    "calorie",
    "whoop",
    "glucose",
    "blood",
    "weight",
    "resting_hr",
    "rmssd",
];

fn is_sensitive_key(k: &str) -> bool {
    let k = k
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .to_lowercase();
    !k.is_empty() && SENSITIVE_KEYS.iter().any(|s| k.contains(s))
}

fn trim_punct(t: &str) -> &str {
    t.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']' | '<' | '>' | '{' | '}'
        )
    })
}

fn looks_like_email(core: &str) -> bool {
    let Some((local, domain)) = core.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._+-%".contains(c))
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
}

fn looks_opaque(core: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "sk-", "sk_", "ghp_", "gho_", "xox", "akia", "eyj", "pk_", "rk_", "bot",
    ];
    let lower = core.to_lowercase();
    if PREFIXES.iter().any(|p| lower.starts_with(p)) && core.len() >= 12 {
        return true;
    }
    let body_ok = core
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_-+=/.".contains(c));
    if !body_ok || core.len() < 20 {
        return false;
    }
    let digit = core.chars().any(|c| c.is_ascii_digit());
    let alpha = core.chars().any(|c| c.is_ascii_alphabetic());
    // Paths and dotted names are not secrets: require no '/' run and mixed digits+letters,
    // or a long hex string.
    let hex = core.len() >= 32 && core.chars().all(|c| c.is_ascii_hexdigit());
    hex || (digit && alpha && !core.contains('/') && !core.contains('.'))
}

/// Local paths that name a person or a machine: `/Users/<name>` and `/home/<name>` become `~`,
/// temp folders (`/private/var/folders/...`, `/var/folders/...`, `/tmp/...`) become `[tmp]`.
fn scrub_home(t: &str) -> String {
    let mut cur = t.to_string();
    for prefix in ["/Users/", "/home/"] {
        let mut out = String::with_capacity(cur.len());
        let mut rest = cur.as_str();
        while let Some(i) = rest.find(prefix) {
            out.push_str(&rest[..i]);
            let after = &rest[i + prefix.len()..];
            // The name ends at the next '/', whitespace or quote; the rest of the text is kept.
            let end = after
                .find(|c: char| c == '/' || c.is_whitespace() || matches!(c, '"' | '\''))
                .unwrap_or(after.len());
            out.push('~');
            rest = &after[end..];
        }
        out.push_str(rest);
        cur = out;
    }
    for prefix in [
        "/private/var/folders/",
        "/var/folders/",
        "/private/tmp/",
        "/tmp/",
    ] {
        let mut out = String::with_capacity(cur.len());
        let mut rest = cur.as_str();
        while let Some(i) = rest.find(prefix) {
            out.push_str(&rest[..i]);
            out.push_str("[tmp]");
            let after = &rest[i..];
            let end = after
                .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ')' | ']'))
                .unwrap_or(after.len());
            rest = &after[end..];
        }
        out.push_str(rest);
        cur = out;
    }
    cur
}

/// `~name` / `~name/x` (another user's home) -> `~` / `~/x`.
fn scrub_tilde_user(tok: &str) -> Option<String> {
    let core = trim_punct(tok);
    let rest = core.strip_prefix('~')?;
    if !rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return None;
    }
    let tail = rest.find('/').map(|j| &rest[j..]).unwrap_or("");
    Some(tok.replacen(core, &format!("~{tail}"), 1))
}

fn redact_token(tok: &str, redact_next: &mut bool) -> String {
    if *redact_next {
        let scheme = trim_punct(tok).to_lowercase();
        if matches!(scheme.as_str(), "bearer" | "basic" | "token") {
            // `Authorization: Bearer <value>`: the scheme word stays, the value after it goes.
            return tok.to_string();
        }
        *redact_next = false;
        if !tok.is_empty() {
            return "[redacted]".to_string();
        }
    }
    if let Some(t) = scrub_tilde_user(tok) {
        return t;
    }
    let core = trim_punct(tok);
    if core.is_empty() {
        return tok.to_string();
    }
    let lower = core.to_lowercase();
    // Scheme words that introduce a credential in the next token.
    if matches!(lower.as_str(), "bearer" | "basic" | "token") {
        *redact_next = true;
        return tok.to_string();
    }
    // URL / path with a query string or fragment.
    if (core.contains("://") || core.starts_with('/')) && (core.contains('?') || core.contains('#'))
    {
        let cut = core.find(['?', '#']).unwrap_or(core.len());
        let userinfo_free = core[..cut].to_string();
        return format!("{}?[redacted]", strip_userinfo(&userinfo_free));
    }
    if core.contains("://") {
        return strip_userinfo(core);
    }
    // key=value / key:value / "key":"value"
    if let Some(pos) = core.find(['=', ':']) {
        let (k, v) = (&core[..pos], &core[pos + 1..]);
        if is_sensitive_key(k) {
            if v.trim_matches(|c: char| c == '"' || c == '\'').is_empty() {
                *redact_next = true;
                return tok.to_string();
            }
            return format!("{}{}[redacted]", &core[..pos], &core[pos..pos + 1]);
        }
        if looks_like_email(trim_punct(v)) || looks_opaque(trim_punct(v)) {
            return format!("{}{}[redacted]", k, &core[pos..pos + 1]);
        }
    }
    if looks_like_email(core) {
        return "[email]".to_string();
    }
    if looks_opaque(core) {
        return "[redacted]".to_string();
    }
    tok.to_string()
}

/// `https://user:pw@host/path` -> `https://host/path`
fn strip_userinfo(url: &str) -> String {
    if let Some(i) = url.find("://") {
        let (scheme, rest) = url.split_at(i + 3);
        let end = rest.find('/').unwrap_or(rest.len());
        if let Some(at) = rest[..end].rfind('@') {
            return format!("{scheme}{}", &rest[at + 1..]);
        }
    }
    url.to_string()
}

pub(crate) fn redact_line(line: &str) -> String {
    let clean: String = line
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let lower = clean.to_lowercase();
    if mentions_health(&lower) {
        return HEALTH_HIDDEN.to_string();
    }
    let clean = scrub_home(&clean);
    let mut redact_next = false;
    let joined = clean
        .split(' ')
        .map(|t| redact_token(t, &mut redact_next))
        .collect::<Vec<_>>()
        .join(" ");
    let mut out: String = joined.trim().chars().take(MAX_LINE_CHARS).collect();
    if joined.trim().chars().count() > MAX_LINE_CHARS {
        out.push('…');
    }
    out
}

/// Last `MAX_LINES` lines of `bytes`, redacted. `cut_front` = the bytes began mid-file.
pub(crate) fn tail_from_bytes(bytes: &[u8], cut_front: bool) -> (Vec<String>, bool) {
    let text = String::from_utf8_lossy(bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    let mut truncated = cut_front;
    if cut_front && !lines.is_empty() {
        lines.remove(0); // probably a partial line
    }
    if lines.len() > MAX_LINES {
        lines.drain(..lines.len() - MAX_LINES);
        truncated = true;
    }
    (
        lines
            .into_iter()
            .map(redact_line)
            .filter(|l| !l.is_empty())
            .collect(),
        truncated,
    )
}

/// Read the end of a regular file (not a symlink), at most `MAX_BYTES`.
pub(crate) fn read_file_tail(path: &Path) -> Result<(Vec<String>, bool), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| NOT_FOUND.to_string())?;
    if !meta.file_type().is_file() {
        return Err(NOT_FOUND.to_string());
    }
    let mut f = std::fs::File::open(path).map_err(|_| READ_FAILED.to_string())?;
    let len = meta.len();
    let start = len.saturating_sub(MAX_BYTES);
    f.seek(SeekFrom::Start(start))
        .map_err(|_| READ_FAILED.to_string())?;
    let mut buf = Vec::new();
    f.take(MAX_BYTES)
        .read_to_end(&mut buf)
        .map_err(|_| READ_FAILED.to_string())?;
    Ok(tail_from_bytes(&buf, start > 0))
}

async fn tail_for(id: &str, home: PathBuf) -> Result<ServiceLogTail, String> {
    let path = source_for(id, &home)?;
    let (lines, truncated) = tokio::task::spawn_blocking(move || read_file_tail(&path))
        .await
        .map_err(|_| READ_FAILED.to_string())??;
    Ok(ServiceLogTail {
        id: id.to_string(),
        lines,
        truncated,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn service_logs(id: String) -> Result<ServiceLogTail, String> {
    let home = dirs::home_dir().ok_or_else(|| READ_FAILED.to_string())?;
    tail_for(&id, home).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-svclogs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn redacts_secrets_tokens_queries_emails_and_health() {
        let cases: &[(&str, &str)] = &[
            (
                "GET /webhook/abc?token=hunter2&x=1 200",
                "GET /webhook/abc?[redacted] 200",
            ),
            (
                "fetch https://api.example.com/v1/x?key=abcd done",
                "fetch https://api.example.com/v1/x?[redacted] done",
            ),
            (
                "url https://bob:pw123@host.local/p ok",
                "url https://host.local/p ok",
            ),
            (
                "Authorization: Bearer abc.def.ghi next",
                "Authorization: Bearer [redacted] next",
            ),
            ("api_key=sk-live-123456789012 set", "api_key=[redacted] set"),
            (
                "{\"password\":\"p4ss\",\"a\":1}",
                "{\"password\":[redacted]",
            ),
            (
                "mail from taisun@example.com failed",
                "mail from [email] failed",
            ),
            (
                "id 3f9a8c7d6e5b4a3f2e1d0c9b8a7f6e5d here",
                "id [redacted] here",
            ),
            (
                "saved /Users/someone/Library/x.json",
                "saved ~/Library/x.json",
            ),
            ("resting heart rate 51 bpm", HEALTH_HIDDEN),
            ("HRV 66 ms", HEALTH_HIDDEN),
            ("plain line stays plain", "plain line stays plain"),
        ];
        for (input, want) in cases {
            let got = redact_line(input);
            assert!(
                !got.contains("hunter2") && !got.contains("pw123") && !got.contains("p4ss"),
                "{got}"
            );
            if !input.starts_with('{') {
                assert_eq!(&got, want, "{input}");
            } else {
                assert!(got.contains("[redacted]") && !got.contains("p4ss"), "{got}");
            }
        }
        assert!(!redact_line("ESC \u{1b}[31mred\u{1b}[0m").contains('\u{1b}'));
    }

    #[test]
    fn a_secret_never_survives_in_any_position() {
        let secret = "sk-ant-api03-FAKE_ZZZZZZZZZZZZZZZZ_NOT_REAL";
        for line in [
            format!("token={secret}"),
            format!("Bearer {secret}"),
            format!("header x-api-key: {secret}"),
            format!("\"apiKey\": \"{secret}\""),
            format!("plain {secret} tail"),
            format!("https://h/p?a={secret}"),
        ] {
            assert!(!redact_line(&line).contains("ZZZZ"), "{line}");
        }
    }

    #[test]
    fn caps_lines_chars_and_bytes() {
        let long = "x ".repeat(500);
        let r = redact_line(&long);
        assert!(
            r.chars().count() <= MAX_LINE_CHARS + 1,
            "{}",
            r.chars().count()
        );
        let many: String = (0..1000).map(|i| format!("line {i}\n")).collect();
        let (lines, truncated) = tail_from_bytes(many.as_bytes(), false);
        assert_eq!(lines.len(), MAX_LINES);
        assert!(truncated);
        assert_eq!(lines.last().unwrap(), "line 999");
        // multibyte text near the cap never panics
        let multi = "é".repeat(1000);
        assert!(redact_line(&multi).chars().count() <= MAX_LINE_CHARS + 1);
    }

    #[test]
    fn file_tail_reads_only_the_end_and_refuses_symlinks() {
        let d = tmp("file");
        let f = d.join("router.log");
        let big: String = (0..20000).map(|i| format!("INFO req {i}\n")).collect();
        std::fs::write(&f, &big).unwrap();
        assert!(big.len() as u64 > MAX_BYTES);
        let (lines, truncated) = read_file_tail(&f).unwrap();
        assert!(truncated);
        assert_eq!(lines.len(), MAX_LINES);
        assert_eq!(lines.last().unwrap(), "INFO req 19999");
        let link = d.join("link.log");
        std::os::unix::fs::symlink(&f, &link).unwrap();
        assert_eq!(read_file_tail(&link).unwrap_err(), NOT_FOUND);
        assert_eq!(
            read_file_tail(&d.join("missing.log")).unwrap_err(),
            NOT_FOUND
        );
        assert_eq!(read_file_tail(&d).unwrap_err(), NOT_FOUND);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn only_known_ids_and_known_paths() {
        let home = tmp("home");
        let logs = home.join("Library/Logs/amara-router");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("router.log"), "ok 1\nGET /x?token=abc\n").unwrap();
        let t = tail_for("router", home.clone()).await.unwrap();
        assert_eq!(t.id, "router");
        assert_eq!(
            t.lines,
            vec!["ok 1".to_string(), "GET /x?[redacted]".to_string()]
        );
        for (id, want) in [
            ("", "unknown_service"),
            ("../../etc/passwd", "unknown_service"),
            ("/etc/passwd", "unknown_service"),
            ("docker:n8n", "unknown_service"),
            ("voice", NO_LOG),
            ("n8n", NO_LOG),
            ("postgres", NO_LOG),
            ("hermes", NO_LOG),
            ("letta", NO_LOG),
        ] {
            assert_eq!(
                tail_for(id, home.clone()).await.unwrap_err(),
                want,
                "{id:?}"
            );
        }
        // No log file yet: fixed text, no path in it.
        let empty = tmp("empty");
        let err = tail_for("router", empty.clone()).await.unwrap_err();
        assert_eq!(err, NOT_FOUND);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn more_health_words_and_paths_are_scrubbed() {
        for line in [
            "resting hr 52",
            "HR=61 logged",
            "readiness score 80",
            "ecg strip saved",
            "skin temperature 36.1",
            "spo2 97",
            "steps 8120 today",
            "weight 70.2 kg",
            "heart rate sample",
        ] {
            assert_eq!(redact_line(line), HEALTH_HIDDEN, "{line}");
        }
        // Whole-word matching keeps ordinary lines.
        for line in ["chrome through pool ready", "stepper motor", "template ok"] {
            assert_ne!(redact_line(line), HEALTH_HIDDEN, "{line}");
        }
        assert_eq!(
            redact_line("open /home/alice/x/y.log ok"),
            "open ~/x/y.log ok"
        );
        assert_eq!(redact_line("open /Users/alice ok"), "open ~ ok");
        assert_eq!(
            redact_line("tmp /private/var/folders/ab/cd12/T/x.sock end"),
            "tmp [tmp] end"
        );
        assert_eq!(redact_line("tmp /var/folders/ab/T/x end"), "tmp [tmp] end");
        assert_eq!(redact_line("see ~bob/notes.txt"), "see ~/notes.txt");
        assert_eq!(redact_line("see ~/Library/x"), "see ~/Library/x");
        for line in [
            "/home/alice/x",
            "/private/var/folders/q/r/T/z",
            "~alice/z",
            "/Users/alice/z",
        ] {
            assert!(!redact_line(line).contains("alice"), "{line}");
        }
    }

    #[test]
    fn command_takes_only_an_id() {
        let code = include_str!("service_logs.rs");
        let code = code.split("#[cfg(test)]").next().unwrap();
        assert!(code.contains("pub async fn service_logs(id: String)"));
        // No process of any kind, so no docker logs: n8n and postgres logs can carry workflow and SQL content.
        let code: String = code
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for banned in [
            "Command",
            "std::process",
            "tokio::process",
            "docker",
            "reqwest",
        ] {
            assert!(!code.contains(banned), "{banned}");
        }
    }
}
