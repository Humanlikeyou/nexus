//! Terrance Research Commands
//!
//! Provides web search and URL fetching capabilities for background research.
//! Uses Brave Search API (or falls back to DuckDuckGo HTML scraping).

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Search the web using Brave Search API.
/// Falls back to a simple DuckDuckGo scrape if no API key configured.
#[tauri::command]
pub async fn web_search(
    query: String,
    max_results: Option<u32>,
) -> Result<Vec<SearchResult>, String> {
    let limit = max_results.unwrap_or(5).min(10);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    // Try Brave Search API first (if key is available in env)
    if let Ok(api_key) = std::env::var("BRAVE_SEARCH_API_KEY") {
        if !api_key.is_empty() {
            return brave_search(&client, &query, limit, &api_key).await;
        }
    }

    // Fallback: DuckDuckGo instant answers API (limited but free, no key needed)
    duckduckgo_search(&client, &query, limit).await
}

async fn brave_search(
    client: &reqwest::Client,
    query: &str,
    limit: u32,
    api_key: &str,
) -> Result<Vec<SearchResult>, String> {
    let resp = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("Accept", "application/json")
        .header("Accept-Encoding", "gzip")
        .header("X-Subscription-Token", api_key)
        .query(&[("q", query), ("count", &limit.to_string())])
        .send()
        .await
        .map_err(|e| format!("Brave Search error: {}", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Brave Search parse error: {}", e))?;

    let results = body["web"]["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(limit as usize)
                .map(|r| SearchResult {
                    title: r["title"].as_str().unwrap_or("").to_string(),
                    url: r["url"].as_str().unwrap_or("").to_string(),
                    snippet: r["description"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(results)
}

async fn duckduckgo_search(
    client: &reqwest::Client,
    query: &str,
    limit: u32,
) -> Result<Vec<SearchResult>, String> {
    // DuckDuckGo HTML lite search — parse basic results
    let resp = client
        .get("https://html.duckduckgo.com/html/")
        .query(&[("q", query)])
        .header("User-Agent", "Mozilla/5.0 (Nexus/Terrance Research Agent)")
        .send()
        .await
        .map_err(|e| format!("DuckDuckGo error: {}", e))?;

    let html = resp
        .text()
        .await
        .map_err(|e| format!("DuckDuckGo parse error: {}", e))?;

    let mut results = Vec::new();

    // Simple HTML parsing — extract result links and snippets
    // DuckDuckGo lite wraps results in <a class="result__a"> and <a class="result__snippet">
    for chunk in html.split("class=\"result__a\"") {
        if results.len() >= limit as usize {
            break;
        }
        // Extract href
        if let Some(href_start) = chunk.find("href=\"") {
            let href_rest = &chunk[href_start + 6..];
            if let Some(href_end) = href_rest.find('"') {
                let url = &href_rest[..href_end];
                // Decode DuckDuckGo redirect URL
                let decoded_url = if url.contains("uddg=") {
                    url.split("uddg=")
                        .nth(1)
                        .and_then(|u| u.split('&').next())
                        .map(urlencoding_decode)
                        .unwrap_or_else(|| url.to_string())
                } else {
                    url.to_string()
                };

                // Extract title (text between > and </a>)
                let title = href_rest
                    .find('>')
                    .and_then(|s| {
                        href_rest[s + 1..]
                            .find("</a>")
                            .map(|e| &href_rest[s + 1..s + 1 + e])
                    })
                    .unwrap_or("")
                    .replace("<b>", "")
                    .replace("</b>", "");

                // Extract snippet (look for result__snippet nearby)
                let snippet = chunk
                    .find("class=\"result__snippet\"")
                    .and_then(|s| chunk[s..].find('>').map(|gt| s + gt + 1))
                    .and_then(|s| chunk[s..].find("</").map(|e| &chunk[s..s + e]))
                    .unwrap_or("")
                    .replace("<b>", "")
                    .replace("</b>", "");

                if !decoded_url.is_empty() && !title.is_empty() {
                    results.push(SearchResult {
                        title: html_entities_decode(&title),
                        url: decoded_url,
                        snippet: html_entities_decode(&snippet),
                    });
                }
            }
        }
    }

    Ok(results)
}

/// Fetch the text content of a URL (strips HTML tags for readability).
/// Returns at most `max_chars` characters (default 10000).
#[tauri::command]
pub async fn fetch_url(url: String, max_chars: Option<usize>) -> Result<String, String> {
    let limit = max_chars.unwrap_or(10_000);
    let mut current = parse_public_url(&url)?;
    let mut hops = 0;
    let resp = loop {
        let host = current
            .host_str()
            .ok_or_else(|| "URL has no host".to_string())?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let port = current.port_or_known_default().unwrap_or(443);
        let addrs = resolve_public(&host, port).await?;
        // Pin the connection to the vetted addresses (prevents DNS rebinding) and
        // never auto-follow redirects; each hop is re-validated below.
        let mut builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none());
        if host.parse::<std::net::IpAddr>().is_err() {
            builder = builder.resolve_to_addrs(&host, &addrs);
        }
        let client = builder.build().map_err(|e| e.to_string())?;
        let resp = client
            .get(current.clone())
            .header("User-Agent", "Mozilla/5.0 (Nexus/Terrance Research Agent)")
            .send()
            .await
            .map_err(|e| format!("Fetch error: {}", e))?;
        if resp.status().is_redirection() {
            hops += 1;
            if hops > 5 {
                return Err("Too many redirects".to_string());
            }
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "Redirect without Location header".to_string())?;
            let next = current
                .join(location)
                .map_err(|e| format!("Invalid redirect: {}", e))?;
            current = parse_public_url(next.as_str())?;
            continue;
        }
        break resp;
    };

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if resp
        .content_length()
        .is_some_and(|len| len > MAX_FETCH_BYTES as u64)
    {
        return Err(format!(
            "Response is too large (limit {} MB)",
            MAX_FETCH_BYTES / (1024 * 1024)
        ));
    }
    let mut resp = resp;
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Read error: {}", e))?
    {
        push_capped(&mut bytes, &chunk, MAX_FETCH_BYTES)?;
    }
    let body = String::from_utf8_lossy(&bytes).into_owned();

    // If HTML, strip tags to get readable text
    let text = if content_type.contains("html") {
        strip_html_tags(&body)
    } else {
        body
    };

    // Truncate to limit (on a char boundary, so multi-byte text can't panic)
    if text.len() > limit {
        let mut cut = limit;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        Ok(format!(
            "{}...\n\n[Truncated at {} chars]",
            &text[..cut],
            limit
        ))
    } else {
        Ok(text)
    }
}

// ─── URL safety (SSRF guard) ────────────────────────────────────────────────

/// Hard cap on downloaded body bytes, enforced while streaming (Content-Length
/// can be missing or wrong).
const MAX_FETCH_BYTES: usize = 5 * 1024 * 1024;

fn push_capped(buf: &mut Vec<u8>, chunk: &[u8], cap: usize) -> Result<(), String> {
    if buf.len().saturating_add(chunk.len()) > cap {
        return Err(format!(
            "Response is too large (limit {} MB)",
            cap / (1024 * 1024)
        ));
    }
    buf.extend_from_slice(chunk);
    Ok(())
}

/// Parse a URL and require http/https with a host. Literal IPs must be public.
fn parse_public_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|e| format!("Invalid URL: {}", e))?;
    match url.scheme() {
        "http" | "https" => {}
        other => return Err(format!("URL scheme '{}' is not allowed", other)),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("URLs with embedded credentials are not allowed".to_string());
    }
    let host = url.host_str().ok_or_else(|| "URL has no host".to_string())?;
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare.eq_ignore_ascii_case("localhost") || bare.to_ascii_lowercase().ends_with(".localhost") {
        return Err("Local addresses are not allowed".to_string());
    }
    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        if !is_public_ip(&ip) {
            return Err("Private, loopback or link-local addresses are not allowed".to_string());
        }
    }
    Ok(url)
}

async fn resolve_public(host: &str, port: u16) -> Result<Vec<std::net::SocketAddr>, String> {
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("DNS lookup failed: {}", e))?
        .collect();
    if addrs.is_empty() {
        return Err("DNS lookup returned no addresses".to_string());
    }
    if addrs.iter().any(|a| !is_public_ip(&a.ip())) {
        return Err("Host resolves to a private, loopback or link-local address".to_string());
    }
    Ok(addrs)
}

fn is_public_ip(ip: &std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xC0) == 64) // 100.64/10 CGNAT
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0/24
                || (o[0] == 198 && (o[1] & 0xFE) == 18) // 198.18/15
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(&IpAddr::V4(v4));
            }
            let seg = v6.segments();
            let seg0 = seg[0];
            // IPv4-compatible ::a.b.c.d (deprecated) — judge by the embedded IPv4.
            if seg[..6].iter().all(|s| *s == 0) && !v6.is_loopback() && !v6.is_unspecified() {
                let o = v6.octets();
                return is_public_ip(&IpAddr::V4(std::net::Ipv4Addr::new(o[12], o[13], o[14], o[15])));
            }
            !(v6.is_loopback()
                || seg0 == 0x2002 // 2002::/16 6to4 (embeds arbitrary IPv4)
                || (seg0 == 0x2001 && seg[1] == 0x0000) // 2001::/32 Teredo
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg0 & 0xFE00) == 0xFC00 // fc00::/7 unique local
                || (seg0 & 0xFFC0) == 0xFE80 // fe80::/10 link-local
                || (seg0 & 0xFFC0) == 0xFEC0 // fec0::/10 site-local
                || (seg0 == 0x2001 && v6.segments()[1] == 0x0db8) // documentation
                || (seg0 == 0x0064 && v6.segments()[1] == 0xff9b)) // NAT64 (may map to private v4)
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let lower = html.to_lowercase();

    let chars: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        if !in_tag && chars[i] == '<' {
            in_tag = true;
            // Check for script/style opening
            let rest: String = lower_chars[i..].iter().take(10).collect();
            if rest.starts_with("<script") {
                in_script = true;
            } else if rest.starts_with("<style") {
                in_style = true;
            }
            // Check for script/style closing
            if rest.starts_with("</script") {
                in_script = false;
            } else if rest.starts_with("</style") {
                in_style = false;
            }
        } else if in_tag && chars[i] == '>' {
            in_tag = false;
        } else if !in_tag && !in_script && !in_style {
            result.push(chars[i]);
        }
        i += 1;
    }

    // Clean up whitespace: collapse runs of whitespace, trim lines
    let cleaned: String = result
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<&str>>()
        .join("\n");

    html_entities_decode(&cleaned)
}

fn html_entities_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

fn urlencoding_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                result.push(byte as char);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

#[cfg(test)]
mod ssrf_tests {
    use super::*;

    #[test]
    fn rejects_non_http_and_local_urls() {
        for bad in [
            "file:///etc/passwd",
            "ftp://example.com/x",
            "http://localhost:5678/",
            "http://api.localhost/",
            "http://127.0.0.1/",
            "http://10.0.0.5/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
            "http://[fe80::1]/",
            "http://[fd00::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://0.0.0.0/",
            "http://100.64.0.1/",
            "http://user:pw@example.com/",
            "http://[2002:7f00:1::1]/",
            "http://[2001:0:4136:e378::1]/",
            "http://[::127.0.0.1]/",
            "http://[::10.0.0.1]/",
            "http://[64:ff9b::7f00:1]/",
            "http://[64:ff9b::808:808]/",
            "not a url",
        ] {
            assert!(parse_public_url(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn accepts_public_urls() {
        assert!(parse_public_url("https://example.com/path?q=1").is_ok());
        assert!(parse_public_url("http://93.184.216.34/").is_ok());
        assert!(parse_public_url("https://[2606:4700::1111]/").is_ok());
    }

    #[test]
    fn body_cap_is_enforced_while_streaming() {
        let mut buf = Vec::new();
        push_capped(&mut buf, &[0u8; 6], 10).unwrap();
        push_capped(&mut buf, &[0u8; 4], 10).unwrap();
        assert!(push_capped(&mut buf, &[0u8; 1], 10).is_err());
        assert_eq!(buf.len(), 10);
    }

    #[tokio::test]
    async fn resolve_rejects_loopback_hostname_literal() {
        assert!(resolve_public("127.0.0.1", 80).await.is_err());
    }
}
