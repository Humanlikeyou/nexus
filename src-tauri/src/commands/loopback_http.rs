//! The one builder for loopback-only HTTP clients (turn-pipeline plan, lane L0).
//!
//! Every client built here has no proxy, no redirects and a named connect timeout. The parsed
//! loopback check (`loopback_base`, moved from `llm_router.rs`) and a body cap helper live here too,
//! so later lanes do not copy them. `ha.rs` uses this `loopback_base` too (its private copy is gone).

use std::time::Duration;

/// Connect timeout for loopback services. A loopback connect is instant or refused, so 3 s is generous.
pub(crate) const LOOPBACK_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// A loopback client: no redirects (one off the Mac is never legitimate) and no environment proxy
/// (a proxy must never carry a loopback call off the Mac). Per-request timeouts stay with the caller.
pub(crate) fn loopback_client(connect_timeout: Duration) -> reqwest::Client {
    super::shared::http::builder(
        super::shared::http::HttpSpec::loopback().connect_timeout(connect_timeout),
    )
    .build()
    .unwrap_or_default()
}

/// A loopback client for an Amara-owned service (router): as `loopback_client`, plus the per-launch
/// token header. Servers that do not enforce the token ignore it.
pub(crate) fn amara_service_client(connect_timeout: Duration, base: &str) -> reqwest::Client {
    super::shared::http::builder_for_base(
        super::shared::http::HttpSpec::loopback()
            .connect_timeout(connect_timeout)
            .with_launch_token(),
        base,
    )
    .build()
    .unwrap_or_default()
}

/// True when the parsed URL's host is `localhost` or a loopback IP. Parsing (not string splitting)
/// closes userinfo tricks like `http://127.0.0.1@evil.example`.
pub(crate) fn is_loopback_url(raw: &str) -> bool {
    reqwest::Url::parse(raw.trim())
        .ok()
        .is_some_and(|u| host_is_loopback(&u))
}

fn host_is_loopback(u: &reqwest::Url) -> bool {
    let Some(host) = u.host_str() else {
        return false;
    };
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// `http://<loopback host>[:port]`, rebuilt from the parsed URL, or None. Parsing (rather than
/// splitting the string) closes userinfo tricks like `http://127.0.0.1@evil.example` and
/// `http://localhost:1@evil.example:8787`; any path, query or fragment is refused too.
pub(crate) fn loopback_base(raw: &str) -> Option<String> {
    let u = reqwest::Url::parse(raw.trim()).ok()?;
    if !host_is_loopback(&u) {
        return None;
    }
    let host = u.host_str()?.to_string();
    let clean = u.scheme() == "http"
        && u.username().is_empty()
        && u.password().is_none()
        && matches!(u.path(), "" | "/")
        && u.query().is_none()
        && u.fragment().is_none();
    clean.then(|| match u.port() {
        Some(port) => format!("http://{host}:{port}"),
        None => format!("http://{host}"),
    })
}

/// The body exceeded the cap (by `Content-Length` or while streaming).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BodyError {
    TooLarge,
    Read,
}

/// Read a response body, refusing as soon as more than `cap` bytes have arrived: the cap is
/// enforced chunk by chunk, so an oversized or endless body is never held in memory.
pub(crate) async fn read_capped(
    mut resp: reqwest::Response,
    cap: usize,
) -> Result<Vec<u8>, BodyError> {
    if resp.content_length().is_some_and(|n| n > cap as u64) {
        return Err(BodyError::TooLarge);
    }
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|_| BodyError::Read)? {
        if body.len() + chunk.len() > cap {
            return Err(BodyError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn builder_sets_no_proxy_and_no_redirects_source_pin() {
        // loopback_client must build from the shared loopback spec with the caller's connect timeout...
        let src = include_str!("loopback_http.rs");
        let f = &src[src.find("pub(crate) fn loopback_client").unwrap()..];
        let f = &f[..f.find(".build()").unwrap()];
        assert!(f.contains("HttpSpec::loopback()"));
        assert!(f.contains(".connect_timeout(connect_timeout)"));
        // ...and that spec must be no-proxy + no-redirects, applied by the shared builder.
        let http = include_str!("shared/http.rs");
        let spec = &http[http.find("pub(crate) const fn loopback()").unwrap()..];
        let spec = &spec[..spec.find("\n    }\n").unwrap()];
        assert!(spec.contains("no_proxy: true"));
        assert!(spec.contains("no_redirects: true"));
        let b = &http[http.find("pub(crate) fn builder(").unwrap()..];
        assert!(b.contains("b = b.no_proxy();"));
        assert!(b.contains("redirect(reqwest::redirect::Policy::none())"));
    }

    #[test]
    fn non_loopback_hosts_are_refused() {
        for bad in [
            "http://example.com",
            "http://192.168.1.5:8787",
            "http://127.0.0.1@evil.example",
            "http://localhost:1@evil.example:8787",
            "http://127.0.0.1:8787/v1",
            "https://127.0.0.1:8787",
            "not a url",
        ] {
            assert_eq!(loopback_base(bad), None, "{bad}");
        }
        assert!(!is_loopback_url("http://example.com/x"));
        assert!(!is_loopback_url("http://127.0.0.1@evil.example/x"));
        assert!(is_loopback_url("http://127.0.0.1:8787/v1/x"));
        assert!(is_loopback_url("http://[::1]:8787/"));
        assert_eq!(
            loopback_base("http://localhost:8787/").as_deref(),
            Some("http://localhost:8787")
        );
    }

    #[test]
    fn odd_host_spellings_are_normalised_or_refused() {
        // Decimal, hex and short IPv4 forms are normalised by the URL parser to 127.0.0.1, and the
        // base is rebuilt from the parsed host, so the request goes to the canonical address.
        for ok in [
            "http://2130706433:8787",
            "http://0x7f000001:8787",
            "http://0x7f.0.0.1:8787",
            "http://127.1:8787",
            "http://017700000001:8787",
        ] {
            assert_eq!(
                loopback_base(ok).as_deref(),
                Some("http://127.0.0.1:8787"),
                "{ok}"
            );
        }
        // Not loopback (or not provably so): refused.
        for bad in [
            "http://localhost.:8787",
            "http://0.0.0.0:8787",
            "http://[::ffff:127.0.0.1]:8787",
            "http://[::]:8787",
            "http://localhost.evil.example:8787",
            "http://127.0.0.1.evil.example:8787",
        ] {
            assert_eq!(loopback_base(bad), None, "{bad}");
        }
    }

    /// Serve `head` then `chunks` forever-ish; returns the base URL. `declare` sets Content-Length.
    async fn serve(chunks: usize, chunk_len: usize, declare: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let head = if declare {
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n",
                    chunks * chunk_len
                )
            } else {
                "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n".to_string()
            };
            let _ = s.write_all(head.as_bytes()).await;
            for _ in 0..chunks {
                let data = vec![b'a'; chunk_len];
                let r = if declare {
                    s.write_all(&data).await
                } else {
                    let mut framed = format!("{chunk_len:x}\r\n").into_bytes();
                    framed.extend_from_slice(&data);
                    framed.extend_from_slice(b"\r\n");
                    s.write_all(&framed).await
                };
                if r.is_err() {
                    return;
                }
            }
            let _ = s.write_all(b"0\r\n\r\n").await;
        });
        base
    }

    #[tokio::test]
    async fn body_cap_is_enforced_while_streaming() {
        // No Content-Length: only the streaming check can catch it (4 x 100 > 250).
        let base = serve(4, 100, false).await;
        let resp = loopback_client(LOOPBACK_CONNECT_TIMEOUT)
            .get(&base)
            .send()
            .await
            .unwrap();
        assert_eq!(read_capped(resp, 250).await, Err(BodyError::TooLarge));
        // Exactly at the cap is fine.
        let base = serve(2, 100, false).await;
        let resp = loopback_client(LOOPBACK_CONNECT_TIMEOUT)
            .get(&base)
            .send()
            .await
            .unwrap();
        assert_eq!(read_capped(resp, 200).await.unwrap().len(), 200);
        // A declared length over the cap is refused up front.
        let base = serve(3, 100, true).await;
        let resp = loopback_client(LOOPBACK_CONNECT_TIMEOUT)
            .get(&base)
            .send()
            .await
            .unwrap();
        assert_eq!(read_capped(resp, 250).await, Err(BodyError::TooLarge));
    }
}
