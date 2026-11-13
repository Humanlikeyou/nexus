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
        }
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
}
