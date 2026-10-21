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
                        .map(|u| urlencoding_decode(u))
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
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Nexus/Terrance Research Agent)")
        .send()
        .await
        .map_err(|e| format!("Fetch error: {}", e))?;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body = resp
        .text()
        .await
        .map_err(|e| format!("Read error: {}", e))?;

    // If HTML, strip tags to get readable text
    let text = if content_type.contains("html") {
        strip_html_tags(&body)
    } else {
        body
    };

    // Truncate to limit
    if text.len() > limit {
        Ok(format!(
            "{}...\n\n[Truncated at {} chars]",
            &text[..limit],
            limit
        ))
    } else {
        Ok(text)
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
