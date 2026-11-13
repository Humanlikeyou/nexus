//! Home windows backend (design-1111w, approved in decision 0014 #5).
//!
//! Native child webviews (Tauri multiwebview, `unstable`) sit over the stage slots the UI measures;
//! a pop-out `WebviewWindow` is for sites that refuse a framed view. `home_window_open` is the single
//! write path: a user asked for the page (chat or voice), it is http(s) only, and nothing browses on
//! its own. Amara never fetches a page herself and never injects anything that reads text: the only
//! script run on a page is a boolean check for a password field (sign-in pause).
//!
//! Profiles: macOS WKWebView has no `data_directory`, so each site gets a WebKit data store named by
//! a UUID derived from the site key (`profile_uuid`; macOS 14+). The store itself lives in WebKit's
//! own folder for this app; `<app_data>/webviews/<site>/profile.json` is a status record only (host,
//! "saw a sign-in page", "signed in"), never a cookie or token. Sign out removes the store and that
//! folder, after the native approval dialog. `webviews` is excluded from every backup.
//!
//! Remote pages get no IPC: the capability file names only the `main` window and has no `remote`
//! block (a test pins both).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};

/// At most this many windows (stage and popped out together).
pub const MAX_WINDOWS: usize = 3;
/// How long a replace can be undone.
pub const UNDO_MS: u64 = 8_000;
pub const STATE_EVENT: &str = "home-windows://state";
pub const CLOSED_EVENT: &str = "home-windows://closed";
const WEBVIEWS_DIR: &str = "webviews";
const PROFILE_FILE: &str = "profile.json";
const MAX_URL_LEN: usize = 2048;
const MAX_QUERY_LEN: usize = 512;
const MAX_PROFILE_BYTES: u64 = 4 * 1024;

/// Fixed copy when local-only (navy) or `NEXUS_OUTBOUND_DISABLED` is on. Same text everywhere.
pub const REFUSED_COPY: &str =
    "Web windows are off while Local only or outbound-disabled is on. Nothing was opened.";

// ---------------------------------------------------------------- pure logic

/// `Some(copy)` when opening a web page must be refused.
pub fn refusal(outbound_disabled: bool) -> Option<&'static str> {
    outbound_disabled.then_some(REFUSED_COPY)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub url: Option<String>,
    pub query: Option<String>,
    pub local_path: Option<String>,
}

/// Parse what the user asked to open. Only http(s) URLs with a real DNS host; no credentials in the
/// URL; no IP literals or local names (a page here must never be a way to poke local services).
/// `query` and `localPath` are never used to fetch or read anything.
pub fn parse_target(kind: &str, target: &Target) -> Result<Url, String> {
    if !matches!(kind, "web" | "video" | "chart" | "social") {
        return Err(format!(
            "unsupported: {kind} windows are not wired to a webview"
        ));
    }
    if target
        .query
        .as_ref()
        .is_some_and(|q| q.len() > MAX_QUERY_LEN)
    {
        return Err("refused: that request is too long".into());
    }
    let Some(raw) = target.url.as_deref() else {
        let what = if target.local_path.is_some() {
            "files"
        } else if target.query.is_some() {
            "searches"
        } else {
            "empty targets"
        };
        return Err(format!(
            "refused: only http(s) URLs can be opened, not {what}"
        ));
    };
    if raw.len() > MAX_URL_LEN {
        return Err("refused: that address is too long".into());
    }
    let url = Url::parse(raw.trim()).map_err(|_| "refused: not a valid address".to_string())?;
    check_url(&url)?;
    Ok(url)
}

/// Names that resolve (or are meant to) on this Mac or its LAN. Includes `tauri.localhost`.
const LOCAL_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "internal",
    "lan",
    "home.arpa",
    "corp",
    "home",
    "intranet",
];

/// `host` must already be lower-case with no trailing dot.
pub fn host_is_local(host: &str) -> bool {
    LOCAL_SUFFIXES
        .iter()
        .any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

/// The rules every URL a window may hold must pass (also applied to each navigation).
pub fn check_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("refused: only http(s) URLs can be opened".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("refused: addresses with a username or password are not opened".into());
    }
    // No odd ports: a page host must never be a way to reach a local service on another port.
    if let Some(port) = url.port() {
        if port != 80 && port != 443 {
            return Err("refused: only the standard web ports are opened".into());
        }
    }
    let host = match url.domain() {
        Some(d) => d.trim_end_matches('.').to_ascii_lowercase(),
        None if url.host().is_some() => return Err("refused: IP addresses can't be opened".into()),
        None => return Err("refused: that address has no site".into()),
    };
    if !host.contains('.') || host_is_local(&host) {
        return Err("refused: local names can't be opened".into());
    }
    Ok(())
}

/// Registrable-ish domain: last two labels, or three under `co.jp`-style suffixes.
fn registrable(host: &str) -> String {
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = h.split('.').collect();
    let n = labels.len();
    if n <= 2 {
        return h;
    }
    let generic = [
        "co", "com", "ne", "or", "ac", "go", "org", "net", "gov", "edu",
    ];
    if labels[n - 1].len() == 2 && generic.contains(&labels[n - 2]) {
        labels[n - 3..].join(".")
    } else {
        labels[n - 2..].join(".")
    }
}

/// Host shown to the user (no `www.`, no subdomain noise).
pub fn site_host(url: &Url) -> String {
    registrable(url.host_str().unwrap_or(""))
}

/// Sanitised per-site key used in paths and as the profile name: `[a-z0-9_-]`, dots become `_`.
pub fn site_key(url: &Url) -> Result<String, String> {
    key_from_host(&site_host(url))
}

fn key_from_host(host: &str) -> Result<String, String> {
    let key: String = host
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c == '.' { '_' } else { c })
        .collect();
    valid_site_key(&key)?;
    Ok(key)
}

/// A key is 1-63 chars of `[a-z0-9_-]`, starting with a letter or digit. Anything else (dots,
/// slashes, `..`, spaces, unicode) is refused, so a key can never traverse out of `webviews/`.
pub fn valid_site_key(key: &str) -> Result<(), String> {
    let ok = !key.is_empty()
        && key.len() <= 63
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        && key.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err("Invalid site name.".into())
    }
}

/// `<base>/webviews/<site>`; `base` is the app data dir.
pub fn profile_dir(base: &Path, key: &str) -> Result<PathBuf, String> {
    valid_site_key(key)?;
    Ok(base.join(WEBVIEWS_DIR).join(key))
}

/// The WebKit data-store id for a site: stable, derived from the key, UUID-shaped.
pub fn profile_uuid(key: &str) -> [u8; 16] {
    let digest = Sha256::digest(format!("amara-os/webview-profile/v1/{key}").as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

/// A URL for logs: scheme and host only (no credentials, port, path, query or fragment).
pub fn redact_url(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(u) => format!("{}://{}", u.scheme(), u.host_str().unwrap_or("?")),
        Err(_) => "(unparsable address)".to_string(),
    }
}

/// A window id from the UI: `[A-Za-z0-9_-]`, up to 64 chars.
pub fn valid_window_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err("Invalid window id.".into())
    }
}

const LOGIN_HOST_PREFIXES: &[&str] = &["accounts.", "login.", "auth.", "signin.", "sso.", "id."];
const LOGIN_HOSTS: &[&str] = &[
    "appleid.apple.com",
    "login.microsoftonline.com",
    "login.live.com",
];
const LOGIN_SEGMENTS: &[&str] = &[
    "login",
    "signin",
    "sign-in",
    "sign_in",
    "log-in",
    "logon",
    "auth",
    "oauth",
    "oauth2",
    "authorize",
    "sso",
    "sessions",
    "challenge",
    "checkpoint",
    "password",
];

/// A known sign-in address (host or path). Path segments are matched whole, so `/blog` is not `/log`.
pub fn is_login_url(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    if LOGIN_HOSTS.contains(&host.as_str())
        || LOGIN_HOST_PREFIXES.iter().any(|p| host.starts_with(p))
    {
        return true;
    }
    url.path_segments()
        .map(|mut s| s.any(|seg| LOGIN_SEGMENTS.contains(&seg.to_ascii_lowercase().as_str())))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PageVerdict {
    /// UI content state: `signing_in` or `ready`.
    pub state: &'static str,
    /// Whether anything may read this page's content. Fails closed: an unknown password-field
    /// answer (`None`) or a login URL means no.
    pub read_allowed: bool,
}

pub fn judge_page(url: &Url, has_password_field: Option<bool>) -> PageVerdict {
    let login = is_login_url(url) || has_password_field == Some(true);
    PageVerdict {
        state: if login { "signing_in" } else { "ready" },
        read_allowed: !login && has_password_field == Some(false),
    }
}

/// Fixed copy when a public-looking name resolves to this Mac or a private network.
pub const PRIVATE_ADDR_COPY: &str =
    "That address points to this Mac or a private network, so it was not opened.";
const RESOLVE_FAIL_COPY: &str = "That address could not be checked, so it was not opened.";

/// Loopback, private, link-local, CGNAT, unspecified, multicast, ULA, and IPv4-mapped (or
/// NAT64-embedded) forms of those.
pub fn ip_is_blocked(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast()
                || o[0] == 0
                || (o[0] == 100 && (64..=127).contains(&o[1]))
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return true;
            }
            let seg = v6.segments();
            if (seg[0] & 0xffc0) == 0xfe80 || (seg[0] & 0xfe00) == 0xfc00 {
                return true; // link-local fe80::/10, unique local fc00::/7
            }
            // ::ffff:a.b.c.d, ::a.b.c.d and the NAT64 prefix 64:ff9b::/96 carry an IPv4 address.
            let embedded = v6.to_ipv4_mapped().or_else(|| v6.to_ipv4()).or_else(|| {
                (seg[0] == 0x64 && seg[1] == 0xff9b && seg[2..6] == [0, 0, 0, 0]).then(|| {
                    std::net::Ipv4Addr::new(
                        (seg[6] >> 8) as u8,
                        seg[6] as u8,
                        (seg[7] >> 8) as u8,
                        seg[7] as u8,
                    )
                })
            });
            embedded.is_some_and(|v4| ip_is_blocked(IpAddr::V4(v4)))
        }
    }
}

/// Resolve `host` off the async thread (2 s cap) and refuse if ANY address is blocked, or if it
/// cannot be resolved. Names are also checked in `check_url` (that is all `on_navigation`, which is
/// synchronous, can do; `on_load` re-checks after each navigation starts).
/// KNOWN RESIDUAL: DNS rebinding after this check (the page can resolve differently later). There
/// are no WebKit content rules offline to pin it.
pub async fn resolve_public(host: &str) -> Result<(), String> {
    let target = format!("{host}:443");
    let lookup = tauri::async_runtime::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        target.to_socket_addrs().map(|a| a.collect::<Vec<_>>())
    });
    let addrs = match tokio::time::timeout(std::time::Duration::from_secs(2), lookup).await {
        Ok(Ok(Ok(a))) if !a.is_empty() => a,
        _ => return Err(format!("refused: {RESOLVE_FAIL_COPY}")),
    };
    if addrs.iter().any(|a| ip_is_blocked(a.ip())) {
        return Err(format!("refused: {PRIVATE_ADDR_COPY}"));
    }
    Ok(())
}

/// Fixed copy when the Mac is too old for per-site WebKit data stores.
pub const MACOS_COPY: &str = "Home windows need macOS 14 or later. Nothing was opened.";

/// `version` like "15.1" or "14.0.1".
pub fn macos_version_ok(version: &str) -> bool {
    version
        .trim()
        .split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .is_some_and(|m| m >= 14)
}

/// Checked at run time (the bundle's minimumSystemVersion is untouched).
fn macos_ok() -> bool {
    #[cfg(target_os = "macos")]
    {
        static OK: LazyLock<bool> = LazyLock::new(|| {
            std::process::Command::new("sw_vers")
                .arg("-productVersion")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .is_some_and(|v| macos_version_ok(&v))
        });
        *OK
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Both gates every open, undo and pop-out must pass, in order: outbound/navy, then macOS.
fn gate() -> Result<(), String> {
    if let Some(copy) = refusal(super::telegram::outbound_disabled()) {
        return Err(format!("refused: {copy}"));
    }
    if !macos_ok() {
        return Err(format!("refused: {MACOS_COPY}"));
    }
    Ok(())
}

/// Sites a user may open without the first-time approval dialog.
const CATALOG_SITES: &[&str] = &["instagram_com", "tradingview_com", "youtube_com"];

/// A site never opened before (no profile record) that is not one of the catalog sites needs one
/// native approval that shows the host.
pub fn needs_first_open_approval(base: &Path, key: &str) -> bool {
    !CATALOG_SITES.contains(&key) && read_profile(base, key).is_none()
}

/// Clamp a UI rect to the main window's inner size (logical px). `None` when too little would show.
pub fn clamp_rect(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    inner_w: f64,
    inner_h: f64,
) -> Option<(f64, f64, f64, f64)> {
    const MIN_VISIBLE: f64 = 48.0;
    let (x0, y0) = (x.max(0.0), y.max(0.0));
    let (x1, y1) = ((x + w).min(inner_w), (y + h).min(inner_h));
    (x1 - x0 >= MIN_VISIBLE && y1 - y0 >= MIN_VISIBLE).then_some((x0, y0, x1 - x0, y1 - y0))
}

/// Not-found from a data-store removal means there was nothing to remove.
pub fn store_error_is_not_found(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("not found")
        || m.contains("no such")
        || m.contains("notfound")
        || m.contains("unknown identifier")
}

/// Close every window of a site, without holding the list lock while the caller closes natives.
pub fn close_ids_for_site(set: &Mutex<WindowSet>, site: &str) -> Vec<String> {
    let ids = set.lock().map(|s| s.ids_for_site(site)).unwrap_or_default();
    for id in &ids {
        if let Ok(mut s) = set.lock() {
            s.close(id);
        }
    }
    ids
}

pub fn close_all_ids(set: &Mutex<WindowSet>) -> Vec<String> {
    let ids = set.lock().map(|s| s.ids()).unwrap_or_default();
    for id in &ids {
        if let Ok(mut s) = set.lock() {
            s.close(id);
        }
    }
    ids
}

// ------------------------------------------------------------- window set

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub site: String,
    pub host: String,
    pub url: String,
    pub kind: String,
    pub popped: bool,
    pub opened_at: u64,
    pub last_looked: u64,
}

#[derive(Debug, Clone)]
struct UndoPack {
    opened_id: String,
    replaced: Entry,
    until: u64,
}

#[derive(Debug, Default)]
pub struct WindowSet {
    entries: Vec<Entry>,
    undo: Option<UndoPack>,
}

impl WindowSet {
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }
    pub fn ids(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.id.clone()).collect()
    }

    /// The window looked at least recently (ties: opened earliest).
    pub fn lru_id(&self) -> Option<String> {
        self.entries
            .iter()
            .min_by_key(|e| (e.last_looked, e.opened_at))
            .map(|e| e.id.clone())
    }

    /// Add a window. With 3 already open, the least-recently-looked one is closed to make room
    /// (popped-out windows count) and returned, and an 8 s Undo is armed.
    pub fn open(&mut self, entry: Entry, now: u64) -> Result<Option<Entry>, String> {
        if self.get(&entry.id).is_some() {
            return Err("That window is already open.".into());
        }
        let mut replaced = None;
        if self.entries.len() >= MAX_WINDOWS {
            let lru = self.lru_id().expect("non-empty at the cap");
            let at = self
                .entries
                .iter()
                .position(|e| e.id == lru)
                .expect("present");
            replaced = Some(self.entries.remove(at));
        }
        self.undo = replaced.clone().map(|r| UndoPack {
            opened_id: entry.id.clone(),
            replaced: r,
            until: now.saturating_add(UNDO_MS),
        });
        self.entries.push(entry);
        Ok(replaced)
    }

    /// Looking at a window makes it the most recently looked at.
    pub fn focus(&mut self, id: &str, now: u64) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) => {
                e.last_looked = now;
                true
            }
            None => false,
        }
    }

    pub fn close(&mut self, id: &str) -> Option<Entry> {
        let at = self.entries.iter().position(|e| e.id == id)?;
        self.undo = None;
        Some(self.entries.remove(at))
    }

    pub fn set_popped(&mut self, id: &str, popped: bool) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) => {
                e.popped = popped;
                true
            }
            None => false,
        }
    }

    /// The rebuild after an undo failed: put the set back as it was and keep the Undo armed.
    pub fn undo_failed(&mut self, restored: &Entry, closed: Entry, until: u64) {
        self.entries.retain(|e| e.id != restored.id);
        let pack = UndoPack {
            opened_id: closed.id.clone(),
            replaced: restored.clone(),
            until,
        };
        self.entries.push(closed);
        self.undo = Some(pack);
    }

    pub fn ids_for_site(&self, site: &str) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| e.site == site)
            .map(|e| e.id.clone())
            .collect()
    }

    /// Undo the last replace: close the window that took the slot, bring the replaced one back.
    /// Returns `(restored, closed)`.
    pub fn undo(&mut self, now: u64) -> Result<(Entry, Entry, u64), String> {
        let pack = self.undo.take().ok_or("Nothing to undo.")?;
        if now > pack.until {
            return Err("Undo expired.".into());
        }
        let at = self
            .entries
            .iter()
            .position(|e| e.id == pack.opened_id)
            .ok_or("Nothing to undo.")?;
        let closed = self.entries.remove(at);
        let until = pack.until;
        let mut restored = pack.replaced;
        restored.last_looked = now;
        self.entries.push(restored.clone());
        Ok((restored, closed, until))
    }
}

static SET: LazyLock<Mutex<WindowSet>> = LazyLock::new(|| Mutex::new(WindowSet::default()));
/// Windows whose last verdict allows reading (none is read yet; this is the gate a reader must use).
static READABLE: LazyLock<Mutex<HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn lock_set() -> Result<std::sync::MutexGuard<'static, WindowSet>, String> {
    SET.lock()
        .map_err(|_| "Window list is unavailable.".to_string())
}

/// Whether page content of this window may be read right now (never during sign-in).
#[allow(dead_code)] // the gate for the on-device comment reader, which is not wired yet
pub fn read_allowed(id: &str) -> bool {
    READABLE
        .lock()
        .map(|m| m.get(id).copied().unwrap_or(false))
        .unwrap_or(false)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// --------------------------------------------------------- profile records

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRecord {
    pub site: String,
    pub host: String,
    /// A sign-in page was shown in this profile.
    pub saw_login: bool,
    /// A normal page loaded after a sign-in page. A hint, not proof: Amara never reads cookies.
    pub signed_in: bool,
}

/// Next record after a page finished loading.
pub fn next_profile(
    prev: Option<ProfileRecord>,
    site: &str,
    host: &str,
    login_page: bool,
) -> ProfileRecord {
    let mut rec = prev.unwrap_or(ProfileRecord {
        site: site.to_string(),
        host: host.to_string(),
        saw_login: false,
        signed_in: false,
    });
    if login_page {
        rec.saw_login = true;
        rec.signed_in = false;
    } else if rec.saw_login {
        rec.signed_in = true;
    }
    rec
}

fn read_profile(base: &Path, key: &str) -> Option<ProfileRecord> {
    let path = profile_dir(base, key).ok()?.join(PROFILE_FILE);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_PROFILE_BYTES {
        return None;
    }
    let rec: ProfileRecord = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    (rec.site == key).then_some(rec)
}

fn write_profile(base: &Path, rec: &ProfileRecord) -> Result<(), String> {
    let dir = profile_dir(base, &rec.site)?;
    crate::app_paths::create_private_dir_all(&dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(rec).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&dir.join(PROFILE_FILE), bytes).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SiteRow {
    pub id: String,
    pub host: String,
    pub label: String,
    /// `in`, `out` (a sign-in page was shown, not passed) or `none`.
    pub signed: &'static str,
}

fn site_label(host: &str) -> String {
    match host {
        "instagram.com" => "Instagram".into(),
        "tradingview.com" => "TradingView".into(),
        "youtube.com" => "YouTube".into(),
        other => other.to_string(),
    }
}

/// Status only: never a cookie, token or password.
pub fn list_sites(base: &Path) -> Vec<SiteRow> {
    let mut rows = Vec::new();
    let Ok(rd) = std::fs::read_dir(base.join(WEBVIEWS_DIR)) else {
        return rows;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if valid_site_key(&name).is_err() {
            continue;
        }
        let Some(rec) = read_profile(base, &name) else {
            continue;
        };
        let signed = if rec.signed_in {
            "in"
        } else if rec.saw_login {
            "out"
        } else {
            "none"
        };
        rows.push(SiteRow {
            id: name,
            label: site_label(&rec.host),
            host: rec.host,
            signed,
        });
    }
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    rows
}

/// Delete ONE site's profile folder. Refuses a symlink or anything that resolves outside `webviews/`.
pub fn delete_profile_dir(base: &Path, key: &str) -> Result<bool, String> {
    let dir = profile_dir(base, key)?;
    let meta = match std::fs::symlink_metadata(&dir) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("refused: that profile is not a plain folder".into());
    }
    let root = std::fs::canonicalize(base.join(WEBVIEWS_DIR)).map_err(|e| e.to_string())?;
    let real = std::fs::canonicalize(&dir).map_err(|e| e.to_string())?;
    if real.parent() != Some(root.as_path()) {
        return Err("refused: that profile is outside the webviews folder".into());
    }
    std::fs::remove_dir_all(&real).map_err(|e| e.to_string())?;
    Ok(true)
}

// ----------------------------------------------------------- tauri commands

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub id: String,
    pub site: String,
    pub host: String,
    /// The window closed to make room (looked at least recently); the UI offers Undo.
    pub replaced: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StatePayload {
    id: String,
    state: &'static str,
    host: String,
    signed_in: Option<bool>,
    /// Fixed copy when a window was closed for a reason (never a URL).
    note: Option<&'static str>,
}

#[derive(Debug, Serialize, Clone)]
struct ClosedPayload {
    id: String,
}

fn child_label(id: &str) -> String {
    format!("hw-{id}")
}
fn popout_label(id: &str) -> String {
    format!("hwp-{id}")
}

static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

fn remember(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// Close every Home window. Called when local-only (navy) turns on, so no page stays open.
pub fn close_all() {
    let Some(app) = APP.get() else { return };
    for id in close_all_ids(&SET) {
        close_native(app, &id);
        let _ = app.emit(CLOSED_EVENT, ClosedPayload { id });
    }
}

fn log_line(msg: &str) {
    eprintln!("[home-windows] {msg}");
}

fn close_native(app: &AppHandle, id: &str) {
    if let Some(wv) = app.get_webview(&child_label(id)) {
        let _ = wv.close();
    }
    if let Some(w) = app.get_webview_window(&popout_label(id)) {
        let _ = w.destroy();
    }
    if let Ok(mut m) = READABLE.lock() {
        m.remove(id);
    }
}

#[tauri::command]
pub async fn home_window_preflight() -> Result<(), String> {
    gate()
}

#[tauri::command]
pub async fn home_window_open(
    app: AppHandle,
    id: String,
    kind: String,
    target: Target,
) -> Result<OpenResult, String> {
    gate()?;
    remember(&app);
    valid_window_id(&id)?;
    let url = parse_target(&kind, &target)?;
    let site = site_key(&url)?;
    let host = site_host(&url);
    // Before anything is built (and before a dialog): a public name must not point at a private address.
    resolve_public(url.host_str().unwrap_or("")).await?;
    if needs_first_open_approval(&crate::app_paths::data_dir(), &site) {
        let text = super::approval_gate::approval_text(
            &format!("Open {host} inside Amara OS"),
            "You have not opened this site here before. It gets its own private browser profile \
             on this Mac. Amara reads nothing on it.",
        )?;
        let _g = super::approval_gate::require_approval(&app, &[format!("webview:{site}")], text)
            .await?;
    }
    let main = app
        .get_window("main")
        .ok_or("The main window is not available.")?;

    let replaced = {
        let mut set = lock_set()?;
        set.open(
            Entry {
                id: id.clone(),
                site: site.clone(),
                host: host.clone(),
                url: url.to_string(),
                kind: kind.clone(),
                popped: false,
                opened_at: now_ms(),
                last_looked: now_ms(),
            },
            now_ms(),
        )?
    };
    if let Some(r) = &replaced {
        close_native(&app, &r.id);
    }

    let built = build_child(&app, &main, &id, &site, &host, url.clone());
    if let Err(e) = built {
        if let Ok(mut set) = lock_set() {
            set.close(&id);
        }
        log_line(&format!(
            "open failed for {}: {e}",
            redact_url(url.as_str())
        ));
        return Err(format!("failed: could not open {host}"));
    }
    log_line(&format!("opened {}", redact_url(url.as_str())));
    Ok(OpenResult {
        id,
        site,
        host,
        replaced: replaced.map(|r| r.id),
    })
}

fn build_child(
    app: &AppHandle,
    main: &tauri::Window,
    id: &str,
    site: &str,
    host: &str,
    url: Url,
) -> Result<(), String> {
    let (app_load, wid_nav) = (app.clone(), id.to_string());
    let (wid, site_s, host_s) = (id.to_string(), site.to_string(), host.to_string());
    let builder = WebviewBuilder::new(child_label(id), WebviewUrl::External(url))
        .data_store_identifier(profile_uuid(site))
        .on_navigation(move |u| {
            let ok = check_url(u).is_ok();
            if !ok {
                log_line(&format!("blocked navigation to {}", redact_url(u.as_str())));
            }
            // A new navigation: whatever was allowed to be read before no longer is.
            clear_readable(&wid_nav);
            ok
        })
        // No pop-up windows from a page: the user opens windows, pages don't.
        .on_new_window(|u, _| {
            log_line(&format!("denied new window for {}", redact_url(u.as_str())));
            NewWindowResponse::Deny
        })
        .on_page_load(move |wv, payload| on_load(&app_load, &wid, &site_s, &host_s, &wv, &payload));
    let webview = main
        .add_child(
            builder,
            LogicalPosition::new(-10_000.0, -10_000.0),
            LogicalSize::new(10.0, 10.0),
        )
        .map_err(|e| e.to_string())?;
    let _ = webview.hide();
    Ok(())
}

fn clear_readable(id: &str) {
    if let Ok(mut m) = READABLE.lock() {
        m.remove(id);
    }
}

/// Page-load handler shared by stage webviews and pop-outs. The password check runs once the page
/// finished; until then (and after any navigation start) the window is not readable.
/// NOTE for the future reader: re-check `read_allowed(id)` AT READ TIME, never cache it.
fn on_load(
    app: &AppHandle,
    id: &str,
    site: &str,
    host: &str,
    wv: &tauri::Webview,
    payload: &tauri::webview::PageLoadPayload<'_>,
) {
    if payload.event() != PageLoadEvent::Finished {
        clear_readable(id);
        if payload.event() == PageLoadEvent::Started {
            // Off-thread: resolve the new host; a private address closes the window.
            let (app, id, host) = (
                app.clone(),
                id.to_string(),
                payload.url().host_str().unwrap_or("").to_string(),
            );
            tauri::async_runtime::spawn(async move {
                if resolve_public(&host).await.is_err() {
                    log_line("closed a window whose address resolves to a private network");
                    if let Ok(mut set) = SET.lock() {
                        set.close(&id);
                    }
                    close_native(&app, &id);
                    let _ = app.emit(
                        STATE_EVENT,
                        StatePayload {
                            id,
                            state: "blocked",
                            host: String::new(),
                            signed_in: None,
                            note: Some(PRIVATE_ADDR_COPY),
                        },
                    );
                }
            });
        }
        return;
    }
    let (app, wid, site_s, host_s, url) = (
        app.clone(),
        id.to_string(),
        site.to_string(),
        host.to_string(),
        payload.url().clone(),
    );
    let _ = wv.eval_with_callback(
        "document.querySelector('input[type=\"password\"]') !== null",
        move |answer| {
            let has_password = match answer.trim() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
            finish_load(&app, &wid, &site_s, &host_s, &url, has_password);
        },
    );
}

fn finish_load(
    app: &AppHandle,
    id: &str,
    site: &str,
    host: &str,
    url: &Url,
    has_password: Option<bool>,
) {
    let verdict = judge_page(url, has_password);
    if let Ok(mut m) = READABLE.lock() {
        m.insert(id.to_string(), verdict.read_allowed);
    }
    let base = crate::app_paths::data_dir();
    let rec = next_profile(
        read_profile(&base, site),
        site,
        host,
        verdict.state == "signing_in",
    );
    if let Err(e) = write_profile(&base, &rec) {
        log_line(&format!("profile record not saved: {e}"));
    }
    log_line(&format!(
        "loaded {} -> {}",
        redact_url(url.as_str()),
        verdict.state
    ));
    let _ = app.emit(
        STATE_EVENT,
        StatePayload {
            id: id.to_string(),
            state: verdict.state,
            host: host.to_string(),
            signed_in: Some(rec.signed_in),
            note: None,
        },
    );
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotRect {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub visible: bool,
}

fn sane(v: f64) -> bool {
    v.is_finite() && (-1.0..=20_000.0).contains(&v)
}

/// The UI sends the stage slot rects (CSS px in the main webview) on layout and resize.
#[tauri::command]
pub async fn home_window_layout(app: AppHandle, rects: Vec<SlotRect>) -> Result<(), String> {
    if rects.len() > MAX_WINDOWS {
        return Err("Too many slots.".into());
    }
    remember(&app);
    let live = lock_set()?.ids();
    let mut shown = Vec::new();
    let main = app
        .get_window("main")
        .ok_or("The main window is not available.")?;
    let inner: Option<(f64, f64)> = main.inner_size().ok().map(|s| {
        let l = s.to_logical::<f64>(main.scale_factor().unwrap_or(1.0));
        (l.width, l.height)
    });
    // While Local only / outbound-disabled is on nothing is shown (the windows are also closed).
    let disabled = super::telegram::outbound_disabled();
    for r in &rects {
        valid_window_id(&r.id)?;
        if !live.contains(&r.id) {
            continue;
        }
        let Some(wv) = app.get_webview(&child_label(&r.id)) else {
            continue;
        };
        let clamped = match (r.visible && !disabled, inner) {
            (true, Some((iw, ih))) if sane(r.x) && sane(r.y) && sane(r.width) && sane(r.height) => {
                clamp_rect(r.x, r.y, r.width, r.height, iw, ih)
            }
            _ => None,
        };
        if let Some((x, y, w, h)) = clamped {
            wv.set_position(LogicalPosition::new(x, y))
                .map_err(|e| e.to_string())?;
            wv.set_size(LogicalSize::new(w, h))
                .map_err(|e| e.to_string())?;
            wv.show().map_err(|e| e.to_string())?;
            shown.push(r.id.clone());
        } else {
            let _ = wv.hide();
        }
    }
    // Anything not mentioned stays hidden (a window on its way out, or popped out).
    for id in live {
        if !shown.contains(&id) {
            if let Some(wv) = app.get_webview(&child_label(&id)) {
                let _ = wv.hide();
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn home_window_focus(
    app: AppHandle,
    id: String,
    grab: Option<bool>,
) -> Result<(), String> {
    valid_window_id(&id)?;
    if !lock_set()?.focus(&id, now_ms()) {
        return Err("No such window.".into());
    }
    if grab.unwrap_or(false) {
        if let Some(wv) = app.get_webview(&child_label(&id)) {
            let _ = wv.set_focus();
        } else if let Some(w) = app.get_webview_window(&popout_label(&id)) {
            let _ = w.set_focus();
        }
    }
    Ok(())
}

/// `id` is a window id, or `"all"`. Idempotent: closing something already gone is fine.
#[tauri::command]
pub async fn home_window_close(app: AppHandle, id: String) -> Result<(), String> {
    let ids = if id == "all" {
        lock_set()?.ids()
    } else {
        valid_window_id(&id)?;
        vec![id]
    };
    for id in ids {
        lock_set()?.close(&id);
        close_native(&app, &id);
    }
    Ok(())
}

/// Undo the last replace (8 s): the replaced window comes back, the new one closes.
#[tauri::command]
pub async fn home_window_undo(app: AppHandle) -> Result<String, String> {
    gate()?;
    remember(&app);
    let main = app
        .get_window("main")
        .ok_or("The main window is not available.")?;
    let (restored, closed, until) = lock_set()?.undo(now_ms())?;
    // Check the URL that will actually load (it may have navigated since `host` was recorded).
    let checked = match Url::parse(&restored.url) {
        Ok(u) => match check_url(&u) {
            Ok(()) => match resolve_public(u.host_str().unwrap_or("")).await {
                Ok(()) => Ok(u),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        },
        Err(_) => Err("refused: that address can't be opened".to_string()),
    };
    let url = match checked {
        Ok(u) => u,
        Err(e) => {
            if let Ok(mut set) = lock_set() {
                set.undo_failed(&restored, closed, until);
            }
            return Err(e);
        }
    };
    close_native(&app, &closed.id);
    if let Err(e) = build_child(
        &app,
        &main,
        &restored.id,
        &restored.site,
        &restored.host,
        url,
    ) {
        // Keep the Undo: the set goes back as it was.
        if let Ok(mut set) = lock_set() {
            set.undo_failed(&restored, closed, until);
        }
        log_line(&format!("undo could not reopen: {e}"));
        return Err(format!("failed: could not reopen {}", restored.host));
    }
    Ok(restored.id)
}

/// Move a window into its own native window (counts toward the 3). Closing that window tells the UI.
#[tauri::command]
pub async fn home_window_pop_out(app: AppHandle, id: String) -> Result<(), String> {
    gate()?;
    remember(&app);
    valid_window_id(&id)?;
    let entry = lock_set()?.get(&id).cloned().ok_or("No such window.")?;
    if entry.popped {
        return Ok(());
    }
    let current = app
        .get_webview(&child_label(&id))
        .and_then(|w| w.url().ok())
        .filter(|u| check_url(u).is_ok());
    let url = match current {
        Some(u) => u,
        None => Url::parse(&entry.url).map_err(|e| e.to_string())?,
    };
    resolve_public(url.host_str().unwrap_or("")).await?;
    let (nav_id, load_app, load_id, load_site, load_host) = (
        id.clone(),
        app.clone(),
        id.clone(),
        entry.site.clone(),
        entry.host.clone(),
    );
    let win = WebviewWindowBuilder::new(&app, popout_label(&id), WebviewUrl::External(url))
        .title(format!("{} · Amara OS", entry.host))
        .inner_size(1100.0, 760.0)
        .center()
        .data_store_identifier(profile_uuid(&entry.site))
        .on_navigation(move |u| {
            clear_readable(&nav_id);
            check_url(u).is_ok()
        })
        .on_new_window(|_, _| NewWindowResponse::Deny)
        // The same sign-in pause and state events as a stage window.
        .on_page_load(move |wv, payload| {
            on_load(
                &load_app,
                &load_id,
                &load_site,
                &load_host,
                AsRef::<tauri::Webview>::as_ref(&wv),
                &payload,
            )
        })
        .build()
        .map_err(|e| {
            log_line(&format!("pop-out failed: {e}"));
            "failed: could not open a pop-out window".to_string()
        })?;
    let (handle, wid) = (app.clone(), id.clone());
    win.on_window_event(move |ev| {
        if matches!(ev, WindowEvent::Destroyed) {
            // Closing the pop-out closes the window; Home returns when it was the last one.
            if let Ok(mut set) = SET.lock() {
                set.close(&wid);
            }
            clear_readable(&wid);
            let _ = handle.emit(CLOSED_EVENT, ClosedPayload { id: wid.clone() });
        }
    });
    lock_set()?.set_popped(&id, true);
    clear_readable(&id);
    if let Some(wv) = app.get_webview(&child_label(&id)) {
        let _ = wv.close();
    }
    Ok(())
}

#[tauri::command]
pub async fn home_window_sites() -> Result<Vec<SiteRow>, String> {
    Ok(list_sites(&crate::app_paths::data_dir()))
}

/// Sign out of one site: native approval, close its windows, remove its WebKit store, delete only
/// that site's folder. Other sites are untouched.
#[tauri::command]
pub async fn home_window_sign_out(app: AppHandle, site: String) -> Result<(), String> {
    remember(&app);
    valid_site_key(&site)?;
    let base = crate::app_paths::data_dir();
    let host = read_profile(&base, &site)
        .map(|r| r.host)
        .unwrap_or_else(|| site.replace('_', "."));
    let text = super::approval_gate::approval_text(
        &format!("Sign out of {host} inside Amara OS"),
        "This deletes the private browser profile for this one site on this Mac: its cookies and \
         saved sessions. Windows open on this site will close. Other sites are not touched.",
    )?;
    let _guard =
        super::approval_gate::require_approval(&app, &[format!("webview:{site}")], text).await?;
    // The list lock is NOT held while natives close (holding it across close() deadlocked).
    let closed = close_ids_for_site(&SET, &site);
    for id in &closed {
        close_native(&app, id);
        let _ = app.emit(CLOSED_EVENT, ClosedPayload { id: id.clone() });
    }
    // A data store can't be removed while a webview still uses it: wait (bounded) for them to go.
    for _ in 0..40 {
        let still = app.webviews().keys().any(|l| {
            closed
                .iter()
                .any(|id| *l == child_label(id) || *l == popout_label(id))
        });
        if !still {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    if let Err(e) = app.remove_data_store(profile_uuid(&site)).await {
        let msg = e.to_string();
        if !store_error_is_not_found(&msg) {
            // Keep the folder: the browser data may still exist, so don't claim it is gone.
            log_line(&format!("data store for {site} not removed: {msg}"));
            return Err("failed: could not remove browser data".into());
        }
    }
    delete_profile_dir(&base, &site)?;
    log_line(&format!("signed out of {host}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }
    fn entry(id: &str, opened: u64, looked: u64) -> Entry {
        Entry {
            id: id.into(),
            site: "a_com".into(),
            host: "a.com".into(),
            url: "https://a.com/".into(),
            kind: "web".into(),
            popped: false,
            opened_at: opened,
            last_looked: looked,
        }
    }
    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("hw-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn cap_is_three_and_a_fourth_replaces_the_one_looked_at_least_recently() {
        let mut s = WindowSet::default();
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            assert_eq!(
                s.open(entry(id, i as u64, i as u64), 100 + i as u64)
                    .unwrap(),
                None
            );
        }
        // "a" was opened first but looked at most recently; "b" is the least recently looked at.
        assert!(s.focus("a", 500));
        let replaced = s
            .open(entry("d", 10, 10), 600)
            .unwrap()
            .expect("a 4th replaces");
        assert_eq!(replaced.id, "b");
        assert_eq!(s.len(), 3);
        assert!(s.get("b").is_none() && s.get("a").is_some() && s.get("d").is_some());
    }

    #[test]
    fn popped_out_windows_count_toward_the_cap() {
        let mut s = WindowSet::default();
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            s.open(entry(id, i as u64, i as u64), 0).unwrap();
        }
        s.set_popped("a", true);
        assert_eq!(s.len(), 3);
        let replaced = s.open(entry("d", 9, 9), 10).unwrap().unwrap();
        assert_eq!(
            replaced.id, "a",
            "a popped-out window is still the least recently looked at"
        );
    }

    #[test]
    fn undo_restores_the_replaced_window_within_8s_and_not_after() {
        let mut s = WindowSet::default();
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            s.open(entry(id, i as u64, i as u64), 0).unwrap();
        }
        s.open(entry("d", 5, 5), 1_000).unwrap();
        let (restored, closed, _) = s.undo(1_000 + UNDO_MS).unwrap();
        assert_eq!((restored.id.as_str(), closed.id.as_str()), ("a", "d"));
        assert_eq!(s.len(), 3);
        assert!(s.undo(1_001).is_err(), "undo is single use");

        let mut s = WindowSet::default();
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            s.open(entry(id, i as u64, i as u64), 0).unwrap();
        }
        s.open(entry("d", 5, 5), 1_000).unwrap();
        assert_eq!(s.undo(1_000 + UNDO_MS + 1).unwrap_err(), "Undo expired.");
        assert!(s.get("d").is_some(), "an expired undo changes nothing");
    }

    #[test]
    fn closing_or_opening_without_a_replace_clears_undo_and_duplicates_are_refused() {
        let mut s = WindowSet::default();
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            s.open(entry(id, i as u64, i as u64), 0).unwrap();
        }
        s.open(entry("d", 5, 5), 10).unwrap();
        s.close("c");
        assert!(s.undo(11).is_err());
        assert!(s.open(entry("d", 6, 6), 12).is_err());
    }

    #[test]
    fn site_keys_are_sanitised_and_cannot_traverse() {
        assert_eq!(
            site_key(&u("https://www.instagram.com/accounts/login/?next=/x")).unwrap(),
            "instagram_com"
        );
        assert_eq!(
            site_key(&u("https://uk.tradingview.com/chart/")).unwrap(),
            "tradingview_com"
        );
        assert_eq!(
            site_key(&u("https://www.example.co.jp/")).unwrap(),
            "example_co_jp"
        );
        for bad in [
            "",
            "..",
            "../x",
            "a/b",
            "a\\b",
            "a.b",
            ".hidden",
            "-x",
            "A_com",
            "é",
            "a b",
            &"a".repeat(64),
        ] {
            assert!(valid_site_key(bad).is_err(), "{bad:?}");
        }
        let base = Path::new("/data");
        assert_eq!(
            profile_dir(base, "instagram_com").unwrap(),
            base.join("webviews/instagram_com")
        );
        assert!(profile_dir(base, "../config").is_err());
        assert!(profile_dir(base, "a/../../b").is_err());
    }

    fn fn_body<'a>(src: &'a str, name: &str) -> &'a str {
        let at = src.find(&format!("pub async fn {name}")).unwrap();
        let rest = &src[at..];
        &rest[..rest.find("\n}\n").unwrap()]
    }

    #[test]
    fn only_standard_ports_and_no_local_suffix_names_open() {
        let ok = |s: &str| check_url(&u(s)).is_ok();
        assert!(
            ok("https://example.com/")
                && ok("https://example.com:443/")
                && ok("http://example.com:80/")
        );
        assert!(ok("https://notlocalhost.com/") && ok("https://homey.example.com/"));
        for bad in [
            "https://example.com:8788/",
            "http://example.com:5678/",
            "https://example.com:8283/",
            "http://example.com:8080/",
            "https://tauri.localhost/",
            "http://ipc.localhost/",
            "https://a.b.localhost/",
            "http://localhost./",
            "https://printer.local/",
            "https://x.internal/",
            "https://nas.lan/",
            "https://router.home.arpa/",
            "https://files.corp/",
            "https://tv.home/",
            "https://wiki.intranet/",
            "https://LOCALHOST.example.localhost/",
        ] {
            assert!(!ok(bad), "{bad}");
        }
        assert!(
            host_is_local("localhost")
                && host_is_local("a.home.arpa")
                && !host_is_local("example.com")
        );
    }

    #[test]
    fn signing_out_of_a_site_with_an_open_window_does_not_deadlock() {
        let set = std::sync::Arc::new(Mutex::new(WindowSet::default()));
        set.lock().unwrap().open(entry("w1", 1, 1), 1).unwrap();
        let mut other = entry("w2", 2, 2);
        other.site = "b_com".into();
        set.lock().unwrap().open(other, 2).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let s2 = set.clone();
        std::thread::spawn(move || tx.send(close_ids_for_site(&s2, "a_com")).unwrap());
        let closed = rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("closing a site's windows must not deadlock");
        assert_eq!(closed, vec!["w1".to_string()]);
        assert_eq!(set.lock().unwrap().ids(), vec!["w2".to_string()]);
        // the command never holds the lock across close(): no `lock_set()?.` temporary in a loop head
        let src = include_str!("home_windows.rs");
        let body = fn_body(src, "home_window_sign_out");
        assert!(body.contains("close_ids_for_site(&SET, &site)"));
        assert!(!body.contains("for id in lock_set()"));
    }

    #[test]
    fn old_macos_refuses_with_fixed_copy() {
        assert!(
            macos_version_ok("14.0") && macos_version_ok("15.1.2\n") && macos_version_ok("26.0")
        );
        assert!(
            !macos_version_ok("13.6.1")
                && !macos_version_ok("12")
                && !macos_version_ok("garbage")
                && !macos_version_ok("")
        );
        assert!(MACOS_COPY.starts_with("Home windows need macOS 14"));
        let src = include_str!("home_windows.rs");
        assert!(src.contains("macos_ok()") && fn_body(src, "home_window_open").contains("gate()?"));
        assert!(fn_body(src, "home_window_pop_out").contains("gate()?"));
    }

    #[test]
    fn navy_closes_every_window_and_undo_popout_layout_are_gated() {
        let set = Mutex::new(WindowSet::default());
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            set.lock()
                .unwrap()
                .open(entry(id, i as u64, i as u64), 0)
                .unwrap();
        }
        let mut closed = close_all_ids(&set);
        closed.sort();
        assert_eq!(closed, vec!["a", "b", "c"]);
        assert!(set.lock().unwrap().ids().is_empty());
        let src = include_str!("home_windows.rs");
        assert!(fn_body(src, "home_window_undo").contains("gate()?"));
        assert!(fn_body(src, "home_window_pop_out").contains("gate()?"));
        assert!(fn_body(src, "home_window_layout").contains("super::telegram::outbound_disabled()"));
        // turning local-only on closes the windows (the one-line hook in set_local_only)
        let tg = include_str!("telegram.rs");
        let set_fn = &tg[tg.find("pub fn set_local_only").unwrap()..];
        let set_fn = &set_fn[..set_fn.find("\n}\n").unwrap()];
        assert!(set_fn.contains("super::home_windows::close_all()"));
        assert!(set_fn.find("LOCAL_ONLY.store").unwrap() < set_fn.find("close_all()").unwrap());
    }

    #[test]
    fn sign_out_waits_for_webviews_and_keeps_the_folder_when_the_store_wont_go() {
        assert!(store_error_is_not_found("Data store Not Found"));
        assert!(!store_error_is_not_found("store is in use"));
        let src = include_str!("home_windows.rs");
        let body = fn_body(src, "home_window_sign_out");
        assert!(body.contains("app.webviews()") && body.contains("could not remove browser data"));
        assert!(body.find("remove_data_store").unwrap() < body.find("delete_profile_dir").unwrap());
        assert!(body.find("app.webviews()").unwrap() < body.find("remove_data_store").unwrap());
    }

    #[test]
    fn first_open_of_a_new_site_needs_approval_but_known_ones_do_not() {
        let base = tmp("first-open");
        assert!(needs_first_open_approval(&base, "example_org"));
        assert!(!needs_first_open_approval(&base, "instagram_com"));
        assert!(!needs_first_open_approval(&base, "tradingview_com"));
        write_profile(
            &base,
            &ProfileRecord {
                site: "example_org".into(),
                host: "example.org".into(),
                saw_login: false,
                signed_in: false,
            },
        )
        .unwrap();
        assert!(!needs_first_open_approval(&base, "example_org"));
        let src = include_str!("home_windows.rs");
        let open = fn_body(src, "home_window_open");
        assert!(open.find("needs_first_open_approval").unwrap() < open.find("set.open(").unwrap());
        assert!(open.contains("require_approval") && open.contains("Open {host} inside Amara OS"));
        let long = Target {
            query: Some("q".repeat(513)),
            url: Some("https://example.com/".into()),
            ..Default::default()
        };
        assert!(parse_target("web", &long).unwrap_err().contains("too long"));
        let ok = Target {
            query: Some("q".repeat(512)),
            url: Some("https://example.com/".into()),
            ..Default::default()
        };
        assert!(parse_target("web", &ok).is_ok());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rects_are_clamped_to_the_window_and_need_a_minimum_visible_size() {
        assert_eq!(
            clamp_rect(10.0, 20.0, 300.0, 200.0, 1400.0, 900.0),
            Some((10.0, 20.0, 300.0, 200.0))
        );
        assert_eq!(
            clamp_rect(-50.0, -50.0, 300.0, 300.0, 1400.0, 900.0),
            Some((0.0, 0.0, 250.0, 250.0))
        );
        assert_eq!(
            clamp_rect(1000.0, 500.0, 2000.0, 2000.0, 1400.0, 900.0),
            Some((1000.0, 500.0, 400.0, 400.0))
        );
        assert_eq!(
            clamp_rect(1390.0, 10.0, 300.0, 300.0, 1400.0, 900.0),
            None,
            "10 px visible is too little"
        );
        assert_eq!(clamp_rect(0.0, 0.0, 47.0, 500.0, 1400.0, 900.0), None);
        assert_eq!(
            clamp_rect(5000.0, 5000.0, 100.0, 100.0, 1400.0, 900.0),
            None
        );
    }

    #[test]
    fn readability_is_cleared_on_navigation_and_popout_and_destroy_and_popouts_get_the_load_handler(
    ) {
        let src = include_str!("home_windows.rs");
        let prod = src.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(
            fn_body(prod, "home_window_pop_out")
                .matches("clear_readable(")
                .count()
                >= 3
        );
        assert!(fn_body(prod, "home_window_pop_out").contains(".on_page_load("));
        let build = &prod[prod.find("fn build_child").unwrap()..];
        assert!(build[..build.find("add_child").unwrap()].contains("clear_readable(&wid_nav)"));
        assert!(
            prod.contains("PageLoadEvent::Finished")
                && prod.contains("re-check `read_allowed(id)` AT READ TIME")
        );
        READABLE.lock().unwrap().insert("stale".into(), true);
        assert!(read_allowed("stale"));
        clear_readable("stale");
        assert!(!read_allowed("stale"));
    }

    #[test]
    fn a_failed_undo_rebuild_keeps_the_undo_armed() {
        let mut s = WindowSet::default();
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            s.open(entry(id, i as u64, i as u64), 0).unwrap();
        }
        s.open(entry("d", 5, 5), 1_000).unwrap();
        let (restored, closed, until) = s.undo(1_001).unwrap();
        s.undo_failed(&restored, closed, until);
        assert!(s.get("d").is_some() && s.get("a").is_none());
        let (again, _, _) = s.undo(1_002).expect("the pack was kept");
        assert_eq!(again.id, "a");
        assert!(include_str!("home_windows.rs").contains("set.undo_failed("));
    }

    #[test]
    fn logs_carry_scheme_and_host_only_and_popout_errors_use_fixed_copy() {
        assert_eq!(
            redact_url("https://www.example.com:8443/a/b?x=1#y"),
            "https://www.example.com"
        );
        let src = include_str!("home_windows.rs");
        let prod = src.split("#[cfg(test)]\nmod tests").next().unwrap();
        assert!(prod.contains("\"failed: could not open a pop-out window\""));
        assert!(!prod.contains("could not open a pop-out window ({e})"));
    }

    #[test]
    fn ip_classifier_blocks_every_private_range_and_passes_public_addresses() {
        let ip = |s: &str| ip_is_blocked(s.parse().unwrap());
        for bad in [
            "127.0.0.1",
            "127.8.9.10",
            "10.0.0.1",
            "10.255.255.255",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "0.0.0.0",
            "0.1.2.3",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "ff02::1",
            "fe80::1",
            "febf::1",
            "fc00::1",
            "fd12:3456::1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:192.168.0.9",
            "::ffff:169.254.1.1",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "64:ff9b::a00:1",
        ] {
            assert!(ip(bad), "{bad} should be blocked");
        }
        for good in [
            "8.8.8.8",
            "1.1.1.1",
            "172.15.255.255",
            "172.32.0.1",
            "100.63.255.255",
            "100.128.0.1",
            "192.169.0.1",
            "169.253.0.1",
            "93.184.216.34",
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
            "fec0::1",
            "64:ff9b::808:808",
        ] {
            assert!(!ip(good), "{good} should pass");
        }
    }

    #[tokio::test]
    async fn resolution_refuses_private_literals_and_passes_public_ones() {
        // IP literals resolve without any DNS.
        for bad in [
            "127.0.0.1",
            "10.0.0.5",
            "192.168.1.9",
            "169.254.169.254",
            "100.64.1.1",
        ] {
            let e = resolve_public(bad).await.unwrap_err();
            assert!(
                e.starts_with("refused:") && e.contains("private network"),
                "{bad}: {e}"
            );
        }
        assert!(resolve_public("8.8.8.8").await.is_ok());
        // a name that cannot be resolved is refused too (reserved .invalid TLD never resolves)
        let e = resolve_public("host.invalid").await.unwrap_err();
        assert!(e.contains("could not be checked"), "{e}");
    }

    #[test]
    fn resolution_runs_before_anything_is_built_and_navigation_rechecks_off_thread() {
        let src = include_str!("home_windows.rs");
        let prod = src.split("#[cfg(test)]\nmod tests").next().unwrap();
        let open = fn_body(prod, "home_window_open");
        assert!(open.find("resolve_public(").unwrap() < open.find("set.open(").unwrap());
        assert!(open.find("resolve_public(").unwrap() < open.find("build_child(").unwrap());
        let undo = fn_body(prod, "home_window_undo");
        assert!(undo.find("resolve_public(").unwrap() < undo.find("build_child(").unwrap());
        // Undo checks the URL it will load, not the recorded host.
        assert!(!undo.contains("resolve_public(&restored.host)"));
        assert!(undo.find("check_url(&u)").unwrap() < undo.find("build_child(").unwrap());
        assert!(undo.find("u.host_str()").unwrap() < undo.find("build_child(").unwrap());
        let pop = fn_body(prod, "home_window_pop_out");
        assert!(
            pop.find("resolve_public(").unwrap() < pop.find("WebviewWindowBuilder::new").unwrap()
        );
        // the lookup is off the async thread and capped at 2 s
        assert!(prod.contains("spawn_blocking") && prod.contains("from_secs(2)"));
        // page-load Started re-resolves in a spawned task, not in the synchronous callback
        let on_load = &prod[prod.find("fn on_load").unwrap()..];
        let started = &on_load[..on_load.find("let _ = wv.eval_with_callback").unwrap()];
        assert!(
            started.contains("PageLoadEvent::Started") && started.contains("async_runtime::spawn")
        );
        assert!(prod.contains("DNS rebinding") && prod.contains("KNOWN RESIDUAL"));
    }

    #[test]
    fn each_site_gets_its_own_stable_uuid_shaped_store_id() {
        let a = profile_uuid("instagram_com");
        assert_eq!(a, profile_uuid("instagram_com"));
        assert_ne!(a, profile_uuid("tradingview_com"));
        assert_eq!(a[6] >> 4, 4);
        assert_eq!(a[8] >> 6, 0b10);
    }

    #[test]
    fn only_http_and_https_urls_with_a_real_host_open() {
        let t = |s: &str| Target {
            url: Some(s.into()),
            ..Default::default()
        };
        assert!(parse_target("chart", &t("https://www.tradingview.com/chart/")).is_ok());
        assert!(parse_target("web", &t("http://example.org/a")).is_ok());
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "tauri://localhost/",
            "ftp://example.com/",
            "about:blank",
            "https://user:pw@example.com/",
            "https://127.0.0.1:8642/",
            "http://[::1]/",
            "http://localhost/",
            "http://printer.local/",
            "http://intranet/",
            "not a url",
        ] {
            assert!(parse_target("web", &t(bad)).is_err(), "{bad}");
        }
        // a query or a local path never opens anything on its own
        let q = Target {
            query: Some("bitcoin".into()),
            ..Default::default()
        };
        assert!(parse_target("web", &q).is_err());
        let p = Target {
            local_path: Some("/Users/x/a.pdf".into()),
            ..Default::default()
        };
        assert!(parse_target("pdf", &p)
            .unwrap_err()
            .starts_with("unsupported:"));
        assert!(parse_target("web", &p).is_err());
    }

    #[test]
    fn logs_never_carry_query_strings_fragments_or_credentials() {
        let r = redact_url(
            "https://user:pw@www.example.com/path/x?token=abc123&next=/y#access_token=zzz",
        );
        assert_eq!(r, "https://www.example.com");
        assert!(
            !r.contains("abc123") && !r.contains("zzz") && !r.contains("pw") && !r.contains("path")
        );
        assert_eq!(redact_url("garbage"), "(unparsable address)");
    }

    #[test]
    fn sign_in_pages_are_detected_by_url_or_password_field_and_reading_pauses() {
        for login in [
            "https://www.instagram.com/accounts/login/",
            "https://accounts.google.com/v3/signin/identifier",
            "https://www.tradingview.com/accounts/signin/",
            "https://example.com/oauth2/authorize?client_id=1",
            "https://login.microsoftonline.com/common",
            "https://example.com/Login",
        ] {
            let v = judge_page(&u(login), Some(false));
            assert_eq!((v.state, v.read_allowed), ("signing_in", false), "{login}");
        }
        // a password field on an ordinary-looking URL
        let v = judge_page(&u("https://example.com/home"), Some(true));
        assert_eq!((v.state, v.read_allowed), ("signing_in", false));
        // a normal page, password check answered no
        let v = judge_page(&u("https://example.com/blog/post"), Some(false));
        assert_eq!((v.state, v.read_allowed), ("ready", true));
        // fails closed: unknown answer means never read
        let v = judge_page(&u("https://example.com/blog/post"), None);
        assert_eq!((v.state, v.read_allowed), ("ready", false));
        assert!(!read_allowed("no-such-window"));
    }

    #[test]
    fn profile_record_tracks_sign_in_without_any_secret() {
        let r = next_profile(None, "a_com", "a.com", false);
        assert!(!r.saw_login && !r.signed_in);
        let r = next_profile(Some(r), "a_com", "a.com", true);
        assert!(r.saw_login && !r.signed_in);
        let r = next_profile(Some(r), "a_com", "a.com", false);
        assert!(r.signed_in);
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.to_lowercase().contains("cookie") && !json.to_lowercase().contains("token"));
    }

    #[test]
    fn sites_list_and_sign_out_touch_only_that_site() {
        let base = tmp("signout");
        for (site, host, login, signed) in [
            ("a_com", "a.com", true, true),
            ("b_com", "b.com", false, false),
        ] {
            write_profile(
                &base,
                &ProfileRecord {
                    site: site.into(),
                    host: host.into(),
                    saw_login: login,
                    signed_in: signed,
                },
            )
            .unwrap();
        }
        std::fs::write(base.join("webviews/a_com/extra.bin"), b"x").unwrap();
        std::fs::write(base.join("config.json"), b"{}").unwrap();
        let rows = list_sites(&base);
        assert_eq!(
            rows.iter()
                .map(|r| (r.id.as_str(), r.signed))
                .collect::<Vec<_>>(),
            vec![("a_com", "in"), ("b_com", "none")]
        );

        assert!(delete_profile_dir(&base, "a_com").unwrap());
        assert!(!base.join("webviews/a_com").exists());
        assert!(
            base.join("webviews/b_com/profile.json").exists(),
            "other sites are untouched"
        );
        assert!(base.join("config.json").exists());
        assert!(!delete_profile_dir(&base, "a_com").unwrap(), "already gone");
        assert!(delete_profile_dir(&base, "../b_com").is_err());
        assert!(delete_profile_dir(&base, "..").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn sign_out_refuses_a_symlinked_profile() {
        let base = tmp("symlink");
        let outside = tmp("symlink-outside");
        std::fs::write(outside.join("keep.txt"), b"keep").unwrap();
        std::fs::create_dir_all(base.join("webviews")).unwrap();
        std::os::unix::fs::symlink(&outside, base.join("webviews/evil_com")).unwrap();
        assert!(delete_profile_dir(&base, "evil_com").is_err());
        assert!(outside.join("keep.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn navy_or_outbound_disabled_refuses_with_fixed_copy() {
        assert_eq!(refusal(true), Some(REFUSED_COPY));
        assert_eq!(refusal(false), None);
        assert!(REFUSED_COPY.starts_with("Web windows are off"));
        // both commands that can open a page check the same gate, which already includes navy
        let src = include_str!("home_windows.rs");
        let open = src.split("pub async fn home_window_open").nth(1).unwrap();
        assert!(open.find("gate()?").unwrap() < open.find("parse_target").unwrap());
        assert!(src.contains("super::telegram::outbound_disabled()"));
        assert!(src.contains("pub async fn home_window_preflight"));
        assert!(include_str!("telegram.rs").contains("local_only_on() || parse_outbound_disabled"));
    }

    #[test]
    fn webviews_is_left_out_of_every_backup() {
        use super::super::backups::is_excluded_name;
        assert!(is_excluded_name("webviews", 0));
        assert!(is_excluded_name("webviews", 1));
        assert!(is_excluded_name("Webviews", 0));
        assert!(!is_excluded_name("goals.json", 0));
    }

    #[test]
    fn remote_pages_get_no_ipc_and_nothing_is_read_or_fetched_here() {
        let caps = include_str!("../../capabilities/default.json");
        assert!(caps.contains("\"windows\": [\"main\"]"));
        assert!(
            !caps.contains("\"remote\""),
            "a remote block would give web pages IPC"
        );
        let src = include_str!("home_windows.rs");
        let prod = src.split("#[cfg(test)]\nmod tests").next().unwrap();
        // the only script run on a page is the password-field yes/no
        assert_eq!(prod.matches(".eval_with_callback").count(), 1);
        assert!(
            !prod.contains("reqwest")
                && !prod.contains("innerText")
                && !prod.contains("document.cookie")
        );
        // a user action drives every navigation: no navigate() call anywhere
        assert!(!prod.contains(".navigate("));
    }
}
