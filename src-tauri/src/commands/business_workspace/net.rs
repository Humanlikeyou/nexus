//! Outbound network for business connectors (decision 0064). The only door to the network for them.
//!
//! - **Approved hosts only** (Taisun, 2026-11-26): the TaiyoSkin site, the GitHub API, and the Supabase
//!   connection pooler for the Tokyo region (`supabase.rs`, not HTTP). Google hosts were approved too,
//!   but GA4 and Search Console are deferred, so none is used. HTTPS only; redirects are followed
//!   only to an approved host, at most 3.
//! - **Kill switch:** every request first checks `outbound_disabled()` (`NEXUS_OUTBOUND_DISABLED`, the
//!   session "local only" switch, and the Settings opt-in). Refused requests send nothing.
//! - **Rate limits:** 429 and 502/503/504 are retried (3 attempts in all), waiting for `Retry-After`
//!   (capped at 30 s) or 1 s, 2 s, 4 s. A GitHub rate limit that resets later than that stops the
//!   sync with the reset time instead of waiting.
//! - **Size:** a response body over the caller's cap is refused.
//! - Nothing here sends customer information: requests carry only paths and the connector's own key.

use std::time::Duration;

/// Hosts an HTTPS request may reach.
pub(crate) const APPROVED_HTTP_HOSTS: &[&str] =
    &["taiyoskin.com", "www.taiyoskin.com", "api.github.com"];

/// The Supabase pooler for the Tokyo region: `aws-<n>-ap-northeast-1.pooler.supabase.com`.
pub(crate) fn supabase_pooler_ok(host: &str) -> bool {
    host.strip_suffix("-ap-northeast-1.pooler.supabase.com")
        .and_then(|p| p.strip_prefix("aws-"))
        .is_some_and(|n| (1..=2).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
}

pub(crate) fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    if host.contains('@') || host.contains(':') || host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

pub(crate) fn url_allowed(url: &str) -> bool {
    host_of(url).is_some_and(|h| APPROVED_HTTP_HOSTS.contains(&h.as_str()))
}

/// Refuses when network use is off on this Mac. Nothing is sent.
pub(crate) fn gate() -> Result<(), String> {
    #[cfg(test)]
    if TEST_ALLOW_NET.with(|a| a.get()) {
        return Ok(());
    }
    if crate::commands::telegram::outbound_disabled() {
        return Err(
            "Network use is off on this Mac (outbound is disabled), so nothing was sent.".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub(crate) static TEST_ALLOW_NET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Request {
    pub url: String,
    /// Header name and value. Values may hold a key; requests are never logged.
    pub headers: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Response {
    pub status: u16,
    /// Lowercase names.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// One HTTP GET. The real one uses reqwest with redirects off; tests use a fake.
pub(crate) trait Transport {
    fn get(&self, req: &Request, max_bytes: usize) -> Result<Response, String>;
}

pub(crate) struct Live;

impl Transport for Live {
    fn get(&self, req: &Request, max_bytes: usize) -> Result<Response, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .user_agent("amara-os-enterprise/1 (read-only)")
            .https_only(true)
            .build()
            .map_err(|e| format!("network setup failed: {e}"))?;
        let req = req.clone();
        tauri::async_runtime::block_on(async move {
            let mut b = client.get(&req.url);
            for (k, v) in &req.headers {
                b = b.header(k, v);
            }
            let r = b.send().await.map_err(|e| {
                if e.is_timeout() {
                    "the request timed out".to_string()
                } else if e.is_connect() {
                    "couldn't connect".to_string()
                } else {
                    "the request failed".to_string()
                }
            })?;
            let status = r.status().as_u16();
            let headers = r
                .headers()
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_ascii_lowercase(),
                        v.to_str().unwrap_or("").to_string(),
                    )
                })
                .collect();
            if r.content_length().is_some_and(|n| n as usize > max_bytes) {
                return Err(format!(
                    "the response is over {} KB; not read",
                    max_bytes / 1024
                ));
            }
            let mut body = Vec::new();
            let mut stream = r;
            while let Some(chunk) = stream
                .chunk()
                .await
                .map_err(|_| "the response was cut off".to_string())?
            {
                body.extend_from_slice(&chunk);
                if body.len() > max_bytes {
                    return Err(format!(
                        "the response is over {} KB; not read",
                        max_bytes / 1024
                    ));
                }
            }
            Ok(Response {
                status,
                headers,
                body,
            })
        })
    }
}

/// How the caller waits between retries (tests record instead of sleeping).
pub(crate) type Sleep = fn(Duration);
pub(crate) fn real_sleep(d: Duration) {
    std::thread::sleep(d);
}

/// GET with the gate, the host allowlist, approved-only redirects and retries.
pub(crate) fn fetch(
    t: &dyn Transport,
    sleep: Sleep,
    req: &Request,
    max_bytes: usize,
) -> Result<Response, String> {
    gate()?;
    let mut url = req.url.clone();
    let mut redirects = 0;
    let mut attempt = 0;
    loop {
        if !url_allowed(&url) {
            return Err(format!(
                "{} is not an approved host; nothing was sent",
                host_of(&url).unwrap_or_else(|| "that address".into())
            ));
        }
        attempt += 1;
        let r = t.get(
            &Request {
                url: url.clone(),
                headers: req.headers.clone(),
            },
            max_bytes,
        );
        match r {
            Ok(resp) if (300..400).contains(&resp.status) => {
                let loc = resp
                    .header("location")
                    .ok_or("a redirect without a location")?;
                let next = if loc.starts_with('/') {
                    format!("https://{}{loc}", host_of(&url).unwrap_or_default())
                } else {
                    loc.to_string()
                };
                redirects += 1;
                if redirects > 3 {
                    return Err("too many redirects".into());
                }
                url = next;
                attempt = 0;
            }
            Ok(resp) if matches!(resp.status, 429 | 502 | 503 | 504) && attempt < 3 => {
                sleep(retry_wait(&resp, attempt)?);
            }
            Ok(resp) => return Ok(resp),
            Err(e)
                if attempt < 3 && (e.contains("timed out") || e.contains("couldn't connect")) =>
            {
                sleep(Duration::from_secs(1 << (attempt - 1)));
            }
            Err(e) => return Err(e),
        }
    }
}

/// `Retry-After` seconds (capped at 30), else 1 s, 2 s, 4 s. A GitHub reset further away stops.
pub(crate) fn retry_wait(resp: &Response, attempt: u32) -> Result<Duration, String> {
    if let Some(s) = resp
        .header("retry-after")
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        if s > 30 {
            return Err(format!(
                "rate limited; the service asks to wait {s} s, so this sync stopped"
            ));
        }
        return Ok(Duration::from_secs(s));
    }
    if resp.header("x-ratelimit-remaining") == Some("0") {
        if let Some(reset) = resp
            .header("x-ratelimit-reset")
            .and_then(|v| v.parse::<i64>().ok())
        {
            let wait = reset - chrono::Utc::now().timestamp();
            if wait > 30 {
                let at = chrono::DateTime::from_timestamp(reset, 0)
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_default();
                return Err(format!(
                    "rate limited until {at}; this sync stopped and can be run again then"
                ));
            }
            return Ok(Duration::from_secs(wait.max(1) as u64));
        }
    }
    Ok(Duration::from_secs(1 << (attempt.saturating_sub(1)).min(3)))
}

/// The next page from a `Link` header (`<url>; rel="next"`), only on an approved host.
pub(crate) fn next_link(link: Option<&str>) -> Option<String> {
    link?
        .split(',')
        .find_map(|part| {
            let (u, rel) = part.split_once(';')?;
            rel.contains("rel=\"next\"").then(|| {
                u.trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string()
            })
        })
        .filter(|u| url_allowed(u))
}
