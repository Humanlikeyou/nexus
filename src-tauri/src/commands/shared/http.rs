//! One reqwest client builder. Every setting is named by the caller, so moving a call site here
//! never changes its timeouts, proxy, redirect or TLS rules. Per-request timeouts stay with callers.

use std::time::Duration;

/// The settings a call site used to spell out on its own `Client::builder()`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HttpSpec {
    /// Whole-request timeout (`.timeout`).
    pub timeout: Option<Duration>,
    /// Connect timeout (`.connect_timeout`).
    pub connect_timeout: Option<Duration>,
    /// Ignore environment proxies (`.no_proxy()`): loopback calls must never leave the Mac.
    pub no_proxy: bool,
    /// Never follow a redirect (`Policy::none()`).
    pub no_redirects: bool,
    /// Refuse plain http (`.https_only(true)`), for the few vetted off-Mac hosts.
    pub https_only: bool,
    /// Opt in to the per-launch token header. Only for Amara-owned services (router, crew, voice).
    /// Applied by `builder_for_base` and only when the base is loopback.
    pub launch_token: bool,
}

impl HttpSpec {
    /// Loopback defaults: no proxy, no redirects. Add timeouts with the field setters.
    pub(crate) const fn loopback() -> Self {
        Self {
            timeout: None,
            connect_timeout: None,
            no_proxy: true,
            no_redirects: true,
            https_only: false,
            launch_token: false,
        }
    }

    /// Opt in to `X-Amara-Launch-Token` (Amara-owned loopback services only).
    pub(crate) const fn with_launch_token(mut self) -> Self {
        self.launch_token = true;
        self
    }

    pub(crate) const fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    pub(crate) const fn connect_timeout(mut self, d: Duration) -> Self {
        self.connect_timeout = Some(d);
        self
    }
}

/// A builder with exactly `spec` applied. Callers may add more (e.g. `resolve_to_addrs`) and keep
/// their own `.build()` error text.
pub(crate) fn builder(spec: HttpSpec) -> reqwest::ClientBuilder {
    let mut b = reqwest::Client::builder();
    if let Some(t) = spec.timeout {
        b = b.timeout(t);
    }
    if let Some(t) = spec.connect_timeout {
        b = b.connect_timeout(t);
    }
    if spec.no_proxy {
        b = b.no_proxy();
    }
    if spec.no_redirects {
        b = b.redirect(reqwest::redirect::Policy::none());
    }
    if spec.https_only {
        b = b.https_only(true);
    }
    b
}

/// The launch-token header for a request to `base`, or None. Requires the spec's opt-in, a loopback
/// base, a spec that cannot leave the Mac (no proxy, no redirects, not https-only) and a readable
/// token file. Absent file means no header (the server then fails closed).
pub(crate) fn launch_token_header(
    spec: &HttpSpec,
    base: &str,
    token: Option<String>,
) -> Option<(&'static str, String)> {
    if !(spec.launch_token
        && spec.no_proxy
        && spec.no_redirects
        && !spec.https_only
        && crate::commands::loopback_http::is_loopback_url(base))
    {
        return None;
    }
    token.map(|t| (crate::commands::launch_token::HEADER_NAME, t))
}

/// Like `builder`, plus the launch-token default header when `launch_token_header` allows it.
/// Servers that do not enforce the token ignore the header, so opted-in sites send it always.
pub(crate) fn builder_for_base(spec: HttpSpec, base: &str) -> reqwest::ClientBuilder {
    // Only read the token file for call sites that opted in.
    let token = spec
        .launch_token
        .then(crate::commands::launch_token::current)
        .flatten();
    builder_with_token(spec, base, token)
}

/// `builder_for_base` with the token supplied by the caller (tests use a temp dir, never the real one).
pub(crate) fn builder_with_token(
    spec: HttpSpec,
    base: &str,
    token: Option<String>,
) -> reqwest::ClientBuilder {
    let b = builder(spec);
    match launch_token_header(&spec, base, token) {
        Some((name, value)) => {
            let mut hv = match reqwest::header::HeaderValue::from_str(&value) {
                Ok(v) => v,
                Err(_) => return b,
            };
            hv.set_sensitive(true);
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::HeaderName::from_static("x-amara-launch-token"),
                hv,
            );
            let _ = name;
            b.default_headers(h)
        }
        None => b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_spec_is_no_proxy_no_redirects_and_no_timeouts_until_asked() {
        let s = HttpSpec::loopback();
        assert!(s.no_proxy && s.no_redirects && !s.https_only);
        assert!(s.timeout.is_none() && s.connect_timeout.is_none());
        let s = s
            .timeout(Duration::from_secs(4))
            .connect_timeout(Duration::from_secs(3));
        assert_eq!(s.timeout, Some(Duration::from_secs(4)));
        assert_eq!(s.connect_timeout, Some(Duration::from_secs(3)));
    }

    #[tokio::test]
    async fn a_loopback_client_does_not_follow_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/x\r\nContent-Length: 0\r\n\r\n",
                )
                .await;
        });
        let client = builder(HttpSpec::loopback().timeout(Duration::from_secs(5)))
            .build()
            .unwrap();
        let resp = client.get(format!("http://{addr}/")).send().await.unwrap();
        assert_eq!(resp.status().as_u16(), 302);
    }

    const TOK: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn header_only_on_loopback_and_opted_in() {
        let on = HttpSpec::loopback().with_launch_token();
        let off = HttpSpec::loopback();
        let t = || Some(TOK.to_string());
        let got = launch_token_header(&on, "http://127.0.0.1:8787", t()).unwrap();
        assert_eq!(got, ("X-Amara-Launch-Token", TOK.to_string()));
        assert!(launch_token_header(&on, "http://localhost:9000/x", t()).is_some());
        assert!(launch_token_header(&on, "http://[::1]:9000", t()).is_some());
        // not opted in
        assert!(launch_token_header(&off, "http://127.0.0.1:8787", t()).is_none());
        // off-Mac and userinfo tricks
        assert!(launch_token_header(&on, "https://example.com", t()).is_none());
        assert!(launch_token_header(&on, "http://127.0.0.1@evil.example", t()).is_none());
        // absent token file: no header
        assert!(launch_token_header(&on, "http://127.0.0.1:8787", None).is_none());
    }

    #[test]
    fn header_never_for_off_mac_style_specs() {
        let t = || Some(TOK.to_string());
        let base = "http://127.0.0.1:8787";
        let mut s = HttpSpec::loopback().with_launch_token();
        s.https_only = true;
        assert!(launch_token_header(&s, base, t()).is_none());
        let mut s = HttpSpec::loopback().with_launch_token();
        s.no_redirects = false;
        assert!(launch_token_header(&s, base, t()).is_none());
        let mut s = HttpSpec::loopback().with_launch_token();
        s.no_proxy = false;
        assert!(launch_token_header(&s, base, t()).is_none());
        assert!(!HttpSpec::loopback().launch_token);
    }

    async fn seen_request(client: reqwest::Client) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).await.unwrap();
            let _ = s
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await;
            String::from_utf8_lossy(&buf[..n]).to_lowercase()
        });
        client.get(format!("http://{addr}/")).send().await.unwrap();
        srv.await.unwrap()
    }

    #[tokio::test]
    async fn builder_with_token_sends_header_only_when_opted_in() {
        let d = std::env::temp_dir().join(format!("http-tok-{}", uuid::Uuid::new_v4()));
        let tok = crate::commands::launch_token::mint_in(&d).unwrap();
        let read = || crate::commands::launch_token::read_in(&d);
        let base = "http://127.0.0.1:1";
        let on = builder_with_token(HttpSpec::loopback().with_launch_token(), base, read())
            .build()
            .unwrap();
        let req = seen_request(on).await;
        assert!(
            req.contains(&format!("x-amara-launch-token: {tok}")),
            "{req}"
        );
        let off = builder_with_token(HttpSpec::loopback(), base, read())
            .build()
            .unwrap();
        assert!(!seen_request(off).await.contains("x-amara-launch-token"));
        // opted in but no token file: no header
        let none = builder_with_token(HttpSpec::loopback().with_launch_token(), base, None)
            .build()
            .unwrap();
        assert!(!seen_request(none).await.contains("x-amara-launch-token"));
        // opted in, off-Mac base: no header
        let off_mac = builder_with_token(
            HttpSpec::loopback().with_launch_token(),
            "https://example.com",
            read(),
        )
        .build()
        .unwrap();
        assert!(!seen_request(off_mac).await.contains("x-amara-launch-token"));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn builder_for_base_does_not_read_the_token_file_unless_opted_in() {
        let src = include_str!("http.rs");
        let f = &src[src.find("pub(crate) fn builder_for_base").unwrap()..];
        assert!(f[..f.find("\n}\n").unwrap()].contains("spec\n        .launch_token"));
    }
}
