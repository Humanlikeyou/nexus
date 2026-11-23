//! The Ollama-registry path of `model_download(tier)` (Phase 9, decision 0031): models that are
//! `source: ollama` in the manifest are pulled by the bundled runtime's own `/api/pull`, never by
//! this app's file downloader. We only talk to the LOOPBACK "pull runtime" (a short-lived,
//! unsandboxed `ollama serve` on its own loopback port, run by `pull_window` only while
//! the sandboxed serve is stopped; its base URL is a parameter here). The runtime, not this app, reaches `registry.ollama.ai:443`.
//!
//! Gate order (no connection of any kind before the approval):
//! 1 manifest verifies, 2 tier exists and every model in it is pullable (a `local` model, made on
//! this Mac, is `not_published`), 3 outbound not disabled (env + navy), 4 `registry.ollama.ai:443`
//! is on `PROD_PULL_HOSTS` and the pull base is loopback, 5 free space vs the manifest size,
//! 6 native approval naming host + size. Then: open the window (see `pull_window`), probe the
//! runtime (`runtime_unavailable` if it does not answer), POST `/api/pull`, map the streamed status lines to `model://progress`, and finally
//! require `/api/tags` to list the model with exactly the manifest digest (`checksum_mismatch`).
//! Progress events and every error are fixed text: no URL, no raw runtime text, no digest.

use super::model_download::{
    approval_error, fmt_size, pct, DownloadSummary, Progress, CONNECT_TIMEOUT,
};
use super::model_manifest::{self, Manifest, ManifestModel, ManifestVerifier, SOURCE_OLLAMA};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Emitter;

/// The only host a pull may reach (via the runtime), as `host:port`.
pub(crate) const REGISTRY_HOST_PORT: &str = "registry.ollama.ai:443";
/// Hosts a model pull may reach in production. The raw-download list (`PROD_ALLOWED_HOSTS`) stays
/// empty.
pub(crate) const PROD_PULL_HOSTS: &[&str] = &[REGISTRY_HOST_PORT];
/// Where the pull window's runtime listens (the supervisor lane's spec must use the same port).
pub(crate) const DEFAULT_PULL_BASE: &str = "http://127.0.0.1:11435";

pub const RUNTIME_UNAVAILABLE: &str = "runtime_unavailable";

/// The runtime may sit silent while it verifies a big file, so this is generous.
const PULL_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const TAGS_CAP: usize = 1 << 20;
const LINE_CAP: usize = 64 * 1024;
const MAX_LAYERS: usize = 64;

pub(crate) struct PullEnv<'a> {
    pub pull_hosts: &'a [&'a str],
    pub outbound_disabled: bool,
    /// Base URL of the pull runtime. Must be `http://<loopback>[:port]`.
    pub pull_base: &'a str,
    /// A path on the volume that holds the shared model folder (free-space check).
    pub store_dir: &'a Path,
    pub free_space: fn(&Path) -> Option<u64>,
    /// The download-window lifecycle (stop serve, window up, window down, serve back).
    pub window: &'a dyn super::pull_window::PullHost,
}

/// True when any model of this tier is `ollama`- or `local`-sourced, so the pull path owns it.
pub(crate) fn tier_uses_pull(m: &Manifest, tier: &str) -> bool {
    m.tiers
        .iter()
        .find(|t| t.id == tier)
        .is_some_and(|t| t.models.iter().any(|x| x.source.is_some()))
}

fn host_listed(list: &[&str], host_port: &str) -> bool {
    list.iter().any(|a| a.eq_ignore_ascii_case(host_port))
}

fn norm_digest(d: &str) -> String {
    d.trim().trim_start_matches("sha256:").to_ascii_lowercase()
}

fn map_send_err(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timed_out"
    } else if e.is_connect() {
        RUNTIME_UNAVAILABLE
    } else {
        "upstream_error"
    }
}

/// The digest `/api/tags` lists for `name` (None when absent). Any failure to reach or read the
/// runtime is a fixed code.
async fn tag_digest(
    client: &reqwest::Client,
    base: &str,
    name: &str,
) -> Result<Option<String>, &'static str> {
    let resp = client
        .get(format!("{base}/api/tags"))
        .timeout(PROBE_TIMEOUT)
        .send()
        .await
        .map_err(|e| map_send_err(&e))?;
    if !resp.status().is_success() {
        return Err("upstream_error");
    }
    let body = super::loopback_http::read_capped(resp, TAGS_CAP)
        .await
        .map_err(|e| match e {
            super::loopback_http::BodyError::TooLarge => "too_large",
            super::loopback_http::BodyError::Read => "upstream_error",
        })?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| "upstream_error")?;
    let models = v
        .get("models")
        .and_then(|m| m.as_array())
        .ok_or("upstream_error")?;
    for m in models {
        let n = m
            .get("name")
            .or_else(|| m.get("model"))
            .and_then(|x| x.as_str());
        if n == Some(name) {
            return Ok(m.get("digest").and_then(|d| d.as_str()).map(norm_digest));
        }
    }
    Ok(None)
}

/// Streams `/api/pull` for one model and maps its status lines to progress. Raw runtime text is
/// never forwarded and never returned in an error.
async fn pull_one(
    client: &reqwest::Client,
    base: &str,
    m: &ManifestModel,
    total: u64,
    progress: &mut (dyn FnMut(&Progress) + Send),
) -> Result<(), &'static str> {
    let ev = |state, done: u64| Progress {
        role: m.role.clone(),
        state,
        pct: pct(done, total),
        bytes_done: done.min(total),
        bytes_total: total,
    };
    let body = serde_json::json!({ "model": m.name, "stream": true }).to_string();
    let mut resp = client
        .post(format!("{base}/api/pull"))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| map_send_err(&e))?;
    if !resp.status().is_success() {
        return Err("upstream_error");
    }
    let mut layers: HashMap<String, u64> = HashMap::new();
    let mut best = 0u64;
    let mut state: &'static str = "downloading";
    let mut success = false;
    let mut buf: Vec<u8> = Vec::new();
    progress(&ev("downloading", 0));
    loop {
        let chunk = tokio::time::timeout(PULL_IDLE_TIMEOUT, resp.chunk())
            .await
            .map_err(|_| "timed_out")?
            .map_err(|_| "upstream_error")?;
        let at_end = chunk.is_none();
        if let Some(c) = chunk {
            buf.extend_from_slice(&c);
        }
        while let Some(i) = buf.iter().position(|b| *b == b'\n').or_else(|| {
            // The final line may arrive without a newline.
            (at_end && !buf.is_empty()).then_some(buf.len())
        }) {
            let line: Vec<u8> = buf.drain(..i.min(buf.len())).collect();
            if buf.first() == Some(&b'\n') {
                buf.remove(0);
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let v: serde_json::Value =
                serde_json::from_slice(&line).map_err(|_| "upstream_error")?;
            if v.get("error").is_some() {
                return Err("upstream_error");
            }
            let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
            if status == "success" {
                success = true;
            }
            let mut next = state;
            if status.starts_with("verifying") || status.starts_with("writing") {
                next = "verifying";
            }
            if let (Some(d), Some(done)) = (
                v.get("digest").and_then(|d| d.as_str()),
                v.get("completed").and_then(|c| c.as_u64()),
            ) {
                if d.len() <= 100 && (layers.len() < MAX_LAYERS || layers.contains_key(d)) {
                    let e = layers.entry(d.to_string()).or_insert(0);
                    *e = (*e).max(done);
                }
            }
            let sum = layers
                .values()
                .fold(0u64, |a, b| a.saturating_add(*b))
                .min(total);
            let moved = sum > best;
            best = best.max(sum);
            if moved || next != state {
                state = next;
                progress(&ev(state, best));
            }
        }
        if buf.len() > LINE_CAP {
            return Err("upstream_error");
        }
        if at_end {
            break;
        }
    }
    if success {
        Ok(())
    } else {
        Err("upstream_error")
    }
}

fn failed(role: &str, total: u64) -> Progress {
    Progress {
        role: role.to_string(),
        state: "failed",
        pct: 0,
        bytes_done: 0,
        bytes_total: total,
    }
}

/// The testable core. `approve` gets the dialog text (host + size) and returns a guard held for the
/// whole pull.
pub(crate) async fn run_pull<G, F, Fut>(
    env: &PullEnv<'_>,
    manifest_bytes: &[u8],
    verifier: &dyn ManifestVerifier,
    tier: &str,
    approve: F,
    progress: &mut (dyn FnMut(&Progress) + Send),
) -> Result<DownloadSummary, &'static str>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<G, String>>,
{
    let m = model_manifest::load_with(manifest_bytes, verifier)?;
    let t = m
        .tiers
        .iter()
        .find(|t| t.id == tier)
        .ok_or("tier_not_found")?;
    // A model made on this Mac is not on the registry: say so, never pretend.
    if t.models
        .iter()
        .any(|x| x.source.as_deref() != Some(SOURCE_OLLAMA))
    {
        return Err("not_published");
    }
    if env.outbound_disabled {
        return Err("outbound_blocked");
    }
    if !host_listed(env.pull_hosts, REGISTRY_HOST_PORT) {
        return Err("host_not_allowed");
    }
    let base = super::loopback_http::loopback_base(env.pull_base).ok_or(RUNTIME_UNAVAILABLE)?;
    let mut jobs: Vec<(&ManifestModel, u64, String)> = Vec::new();
    for x in &t.models {
        let (Some(bytes), Some(sha)) = (x.bytes, x.sha256.as_deref()) else {
            return Err("not_published");
        };
        jobs.push((x, bytes, norm_digest(sha)));
    }
    let total: u64 = jobs.iter().fold(0u64, |a, j| a.saturating_add(j.1));
    if (env.free_space)(env.store_dir).is_none_or(|f| f < total) {
        return Err("no_space");
    }
    let host = REGISTRY_HOST_PORT.split(':').next().unwrap_or_default();
    let text = super::approval_gate::approval_text(
        &format!("Download {} model(s) ({})", jobs.len(), fmt_size(total)),
        &format!("From: {host}\nThe local model stops during the download and restarts after."),
    )
    .map_err(|_| "approval_failed")?;
    let _guard = approve(text).await.map_err(|e| approval_error(&e))?;

    // Mutual exclusion (handoff 25a M1): the unsandboxed window runs only while the sandboxed serve
    // is stopped. `begin` undoes everything it did on failure; `finish` ends the window on every
    // path below, and the guard inside the session covers a cancel.
    let session = super::pull_window::begin(env.window).await?;
    let r = pull_models(env.window, &base, &jobs, tier, progress).await;
    session.finish().await;
    r
}

/// The pull proper, against the window on the loopback base. Verifies every pinned digest.
async fn pull_models(
    window: &dyn super::pull_window::PullHost,
    base: &str,
    jobs: &[(&ManifestModel, u64, String)],
    tier: &str,
    progress: &mut (dyn FnMut(&Progress) + Send),
) -> Result<DownloadSummary, &'static str> {
    let client = super::shared::http::builder(
        super::shared::http::HttpSpec::loopback().connect_timeout(CONNECT_TIMEOUT),
    )
    .build()
    .map_err(|_| "storage_error")?;
    // The window process must still be ours and alive before anything is read from or sent to
    // its port (a child that lost the bind race to another listener has exited).
    if !window.window_alive() {
        return Err(RUNTIME_UNAVAILABLE);
    }
    // The runtime must answer before anything is announced; this is the first connection.
    let mut installed: HashMap<&str, Option<String>> = HashMap::new();
    for (x, _, _) in jobs {
        installed.insert(&x.name, tag_digest(&client, base, &x.name).await?);
    }
    for (x, bytes, _) in jobs {
        progress(&Progress {
            role: x.role.clone(),
            state: "waiting",
            pct: 0,
            bytes_done: 0,
            bytes_total: *bytes,
        });
    }
    let mut completed = 0u32;
    for (x, bytes, want) in jobs {
        let r = async {
            // Already installed with the pinned digest: nothing to pull.
            if installed.get(x.name.as_str()).and_then(|d| d.as_deref()) != Some(want.as_str()) {
                if !window.window_alive() {
                    return Err(RUNTIME_UNAVAILABLE);
                }
                pull_one(&client, base, x, *bytes, progress).await?;
                progress(&Progress {
                    role: x.role.clone(),
                    state: "verifying",
                    pct: 100,
                    bytes_done: *bytes,
                    bytes_total: *bytes,
                });
                // Never trust a digest read from a port whose window has died.
                if !window.window_alive() {
                    return Err(RUNTIME_UNAVAILABLE);
                }
                let got = tag_digest(&client, base, &x.name).await?;
                if got.as_deref() != Some(want.as_str()) {
                    return Err("checksum_mismatch");
                }
            }
            Ok::<(), &'static str>(())
        }
        .await;
        if let Err(e) = r {
            progress(&failed(&x.role, *bytes));
            return Err(e);
        }
        progress(&Progress {
            role: x.role.clone(),
            state: "done",
            pct: 100,
            bytes_done: *bytes,
            bytes_total: *bytes,
        });
        completed += 1;
    }
    Ok(DownloadSummary {
        tier: tier.to_string(),
        completed,
    })
}

/// `~/.ollama/models` (the shared model folder today); only used to find the volume for the
/// free-space check, so a missing folder falls back to its nearest existing parent.
fn store_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".ollama").join("models"))
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// The pull path of the click-only `model_download` command. After the gates and the approval, the
/// window lifecycle (`pull_window`) stops the sandboxed serve if the app started it,
/// runs the unsandboxed window on `DEFAULT_PULL_BASE`, stops it again and restores the serve. A
/// runtime the app did not start (Ollama.app) is `runtime_busy`, a pull or start-all in flight is
/// `pull_in_progress` / `busy`; a window that never answers is `runtime_unavailable`.
pub(crate) async fn pull_command(
    app: tauri::AppHandle,
    tier: String,
) -> Result<DownloadSummary, String> {
    let store = store_dir();
    let host = super::pull_window::RunnerHost {
        runner: super::services_start_all::shared_runner(),
        window: super::services_start_all::shared_window(),
    };
    let env = PullEnv {
        pull_hosts: PROD_PULL_HOSTS,
        outbound_disabled: super::telegram::outbound_disabled(),
        pull_base: DEFAULT_PULL_BASE,
        store_dir: &store,
        free_space: super::model_download::statvfs_free,
        window: &host,
    };
    let handle = app.clone();
    let mut emit = move |p: &Progress| {
        let _ = handle.emit(super::model_download::PROGRESS_EVENT, p);
    };
    run_pull(
        &env,
        model_manifest::shipped_bytes(),
        &model_manifest::PinnedSha256(model_manifest::MANIFEST_SHA256),
        &tier,
        |text| async move {
            super::approval_gate::require_approval(&app, &["model_download".to_string()], text)
                .await
        },
        &mut emit,
    )
    .await
    .map_err(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::super::pull_window::fakes::FakeHost;
    use super::super::pull_window::ServeState;
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct AcceptAll;
    impl ManifestVerifier for AcceptAll {
        fn verify(&self, _: &[u8]) -> bool {
            true
        }
    }

    #[derive(Default)]
    struct State {
        /// name -> digest currently "installed"
        tags: HashMap<String, String>,
        /// What a pull installs (name -> digest); a missing name installs nothing.
        installs: HashMap<String, String>,
        /// Lines a pull streams back (per model; `{}` is replaced by nothing, lines are verbatim).
        lines: Vec<String>,
        pulls: Vec<String>,
    }

    struct Fixture {
        base: String,
        conns: Arc<AtomicUsize>,
        state: Arc<Mutex<State>>,
    }

    async fn serve(state: State) -> Fixture {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let conns = Arc::new(AtomicUsize::new(0));
        let state = Arc::new(Mutex::new(state));
        let (c, st) = (conns.clone(), state.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                c.fetch_add(1, Ordering::SeqCst);
                let st = st.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    let (head_end, clen) = loop {
                        let n = s.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                            let cl = head
                                .lines()
                                .find_map(|l| l.strip_prefix("content-length:"))
                                .and_then(|v| v.trim().parse::<usize>().ok())
                                .unwrap_or(0);
                            break (i + 4, cl);
                        }
                    };
                    while buf.len() < head_end + clen {
                        let n = s.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let first = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let reqline = first.lines().next().unwrap_or("").to_string();
                    if reqline.starts_with("GET /api/tags") {
                        let models: Vec<serde_json::Value> = st
                            .lock()
                            .unwrap()
                            .tags
                            .iter()
                            .map(|(n, d)| serde_json::json!({"name": n, "digest": d}))
                            .collect();
                        let body = serde_json::json!({ "models": models }).to_string();
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = s.write_all(head.as_bytes()).await;
                        let _ = s.write_all(body.as_bytes()).await;
                    } else if reqline.starts_with("POST /api/pull") {
                        let req: serde_json::Value =
                            serde_json::from_slice(&buf[head_end..head_end + clen])
                                .unwrap_or_default();
                        let name = req["model"].as_str().unwrap_or("").to_string();
                        assert_eq!(req["stream"], true);
                        let lines = {
                            let mut g = st.lock().unwrap();
                            g.pulls.push(name.clone());
                            g.lines.clone()
                        };
                        let _ = s
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
                            )
                            .await;
                        for l in lines {
                            let _ = s.write_all(format!("{l}\n").as_bytes()).await;
                            let _ = s.flush().await;
                            tokio::task::yield_now().await;
                        }
                        let mut g = st.lock().unwrap();
                        if let Some(d) = g.installs.get(&name).cloned() {
                            g.tags.insert(name, d);
                        }
                    } else {
                        let _ = s
                            .write_all(
                                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .await;
                    }
                    let _ = s.shutdown().await;
                });
            }
        });
        Fixture { base, conns, state }
    }

    fn shipped() -> Manifest {
        model_manifest::load().unwrap()
    }

    fn compact_digests() -> HashMap<String, (u64, String)> {
        shipped().tiers[0]
            .models
            .iter()
            .map(|x| {
                (
                    x.name.clone(),
                    (x.bytes.unwrap(), x.sha256.clone().unwrap()),
                )
            })
            .collect()
    }

    /// A realistic pull stream: manifest, two layers with growing `completed`, verify, success.
    /// Includes a repeated (non-monotonic) layer value that must not move progress backwards.
    fn stream_lines(total: u64) -> Vec<String> {
        let a = total - 500;
        let l = |d: &str, t: u64, c: u64| {
            format!(
                r#"{{"status":"pulling {d}","digest":"sha256:{d}","total":{t},"completed":{c}}}"#
            )
        };
        vec![
            r#"{"status":"pulling manifest"}"#.into(),
            l("aaaaaaaaaaaa", a, 0),
            l("aaaaaaaaaaaa", a, a / 4),
            l("aaaaaaaaaaaa", a, a / 2),
            l("aaaaaaaaaaaa", a, a / 3),
            l("aaaaaaaaaaaa", a, a),
            l("bbbbbbbbbbbb", 500, 500),
            r#"{"status":"verifying sha256 digest"}"#.into(),
            r#"{"status":"writing manifest"}"#.into(),
            r#"{"status":"success"}"#.into(),
        ]
    }

    fn tmp() -> PathBuf {
        std::env::temp_dir()
    }

    fn env<'a>(hosts: &'a [&'a str], base: &'a str, store: &'a Path) -> PullEnv<'a> {
        // A fresh Absent host per call, leaked (test-only) so the env can borrow it.
        let host: &'static FakeHost = Box::leak(Box::new(FakeHost::new(ServeState::Absent)));
        env_with(hosts, base, store, host)
    }

    fn env_with<'a>(
        hosts: &'a [&'a str],
        base: &'a str,
        store: &'a Path,
        host: &'a FakeHost,
    ) -> PullEnv<'a> {
        PullEnv {
            window: host,
            pull_hosts: hosts,
            outbound_disabled: false,
            pull_base: base,
            store_dir: store,
            free_space: |_| Some(u64::MAX),
        }
    }

    const HOSTS: &[&str] = &[REGISTRY_HOST_PORT];

    async fn go(
        env: &PullEnv<'_>,
        tier: &str,
        approved: &AtomicUsize,
        events: &mut Vec<Progress>,
    ) -> Result<DownloadSummary, &'static str> {
        let mut cb = |p: &Progress| events.push(p.clone());
        run_pull(
            env,
            model_manifest::shipped_bytes(),
            &model_manifest::PinnedSha256(model_manifest::MANIFEST_SHA256),
            tier,
            |_t| async {
                approved.fetch_add(1, Ordering::SeqCst);
                Ok::<(), String>(())
            },
            &mut cb,
        )
        .await
    }

    /// A fixture that pulls the compact tier correctly (right digests installed on pull).
    async fn good_fixture() -> Fixture {
        let d = compact_digests();
        let total = d["gemma4:latest"].0;
        serve(State {
            installs: d.iter().map(|(n, (_, s))| (n.clone(), s.clone())).collect(),
            lines: stream_lines(total),
            ..State::default()
        })
        .await
    }

    #[tokio::test]
    async fn pull_maps_progress_monotonically_and_verifies_the_digest() {
        let f = good_fixture().await;
        let store = tmp();
        let e = env(HOSTS, &f.base, &store);
        let approved = AtomicUsize::new(0);
        let mut ev = Vec::new();
        let r = go(&e, "compact", &approved, &mut ev).await.unwrap();
        assert_eq!(r.completed, 2);
        assert_eq!(approved.load(Ordering::SeqCst), 1);
        let mut pulls = f.state.lock().unwrap().pulls.clone();
        pulls.sort();
        assert_eq!(pulls, ["gemma4:latest", "nomic-embed-text:latest"]);
        for role in ["fast", "embed"] {
            let mine: Vec<&Progress> = ev.iter().filter(|e| e.role == role).collect();
            assert_eq!(mine.first().unwrap().state, "waiting");
            assert_eq!(mine.last().unwrap().state, "done");
            assert_eq!(mine.last().unwrap().pct, 100);
            let done: Vec<u64> = mine.iter().map(|e| e.bytes_done).collect();
            assert!(done.windows(2).all(|w| w[0] <= w[1]), "{role} {done:?}");
            assert!(mine
                .iter()
                .any(|e| e.state == "downloading" && e.bytes_done > 0));
            assert!(mine.iter().all(|e| e.bytes_done <= e.bytes_total));
        }
        let json = serde_json::to_string(&ev).unwrap();
        assert!(!json.contains("127.0.0.1") && !json.contains("http"));
        assert!(!json.contains("pulling") && !json.contains("sha256"));
        assert!(json.contains("bytesDone"));
    }

    #[tokio::test]
    async fn digest_mismatch_after_pull_is_checksum_mismatch() {
        let d = compact_digests();
        let total = d["gemma4:latest"].0;
        let f = serve(State {
            installs: d.keys().map(|n| (n.clone(), "0".repeat(64))).collect(),
            lines: stream_lines(total),
            ..State::default()
        })
        .await;
        let store = tmp();
        let e = env(HOSTS, &f.base, &store);
        let mut ev = Vec::new();
        assert_eq!(
            go(&e, "compact", &AtomicUsize::new(0), &mut ev)
                .await
                .unwrap_err(),
            "checksum_mismatch"
        );
        assert_eq!(ev.last().unwrap().state, "failed");
        assert!(ev.iter().all(|e| e.state != "done"));
    }

    #[tokio::test]
    async fn model_missing_after_a_pull_that_reports_success_is_checksum_mismatch() {
        let d = compact_digests();
        let f = serve(State {
            lines: stream_lines(d["gemma4:latest"].0),
            ..State::default()
        })
        .await;
        let store = tmp();
        let e = env(HOSTS, &f.base, &store);
        assert_eq!(
            go(&e, "compact", &AtomicUsize::new(0), &mut Vec::new())
                .await
                .unwrap_err(),
            "checksum_mismatch"
        );
    }

    #[tokio::test]
    async fn already_installed_with_the_pinned_digest_skips_the_pull() {
        let d = compact_digests();
        let f = serve(State {
            tags: d.iter().map(|(n, (_, s))| (n.clone(), s.clone())).collect(),
            ..State::default()
        })
        .await;
        let store = tmp();
        let e = env(HOSTS, &f.base, &store);
        let mut ev = Vec::new();
        let r = go(&e, "compact", &AtomicUsize::new(0), &mut ev)
            .await
            .unwrap();
        assert_eq!(r.completed, 2);
        assert!(f.state.lock().unwrap().pulls.is_empty());
        assert_eq!(ev.last().unwrap().state, "done");
    }

    #[tokio::test]
    async fn outbound_disabled_or_navy_opens_zero_connections() {
        let f = good_fixture().await;
        let store = tmp();
        let mut e = env(HOSTS, &f.base, &store);
        e.outbound_disabled = true;
        let approved = AtomicUsize::new(0);
        let mut ev = Vec::new();
        assert_eq!(
            go(&e, "compact", &approved, &mut ev).await.unwrap_err(),
            "outbound_blocked"
        );
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
        assert_eq!(approved.load(Ordering::SeqCst), 0);
        assert!(ev.is_empty());
    }

    #[tokio::test]
    async fn declined_approval_opens_zero_connections_and_names_host_and_size() {
        let f = good_fixture().await;
        let store = tmp();
        let e = env(HOSTS, &f.base, &store);
        let mut cb = |_: &Progress| {};
        let r = run_pull(
            &e,
            model_manifest::shipped_bytes(),
            &model_manifest::PinnedSha256(model_manifest::MANIFEST_SHA256),
            "compact",
            |t| async move {
                assert!(t.contains("registry.ollama.ai") && t.contains("9.9 GB"));
                assert!(!t.contains("127.0.0.1") && !t.contains("11435"));
                Err::<(), String>(super::super::approval_gate::denied_message())
            },
            &mut cb,
        )
        .await;
        assert_eq!(r.unwrap_err(), "declined");
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn registry_must_be_on_the_pull_list_before_any_connection() {
        let f = good_fixture().await;
        let store = tmp();
        for hosts in [
            vec![],
            vec!["registry.ollama.ai"],
            vec!["registry.ollama.ai:8443"],
            vec!["example.com:443"],
        ] {
            let e = env(&hosts, &f.base, &store);
            let approved = AtomicUsize::new(0);
            assert_eq!(
                go(&e, "compact", &approved, &mut Vec::new())
                    .await
                    .unwrap_err(),
                "host_not_allowed"
            );
            assert_eq!(approved.load(Ordering::SeqCst), 0);
        }
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn non_loopback_pull_base_is_refused_before_dialog_and_socket() {
        let store = tmp();
        for base in [
            "http://example.com:11435",
            "https://127.0.0.1:11435",
            "http://127.0.0.1@evil.example:11435",
            "http://127.0.0.1:11435/api",
            "http://192.168.1.5:11435",
            "",
        ] {
            let e = env(HOSTS, base, &store);
            let approved = AtomicUsize::new(0);
            assert_eq!(
                go(&e, "compact", &approved, &mut Vec::new())
                    .await
                    .unwrap_err(),
                RUNTIME_UNAVAILABLE,
                "{base}"
            );
            assert_eq!(approved.load(Ordering::SeqCst), 0, "{base}");
        }
    }

    #[tokio::test]
    async fn local_source_model_is_not_published_with_zero_connections() {
        let f = good_fixture().await;
        let store = tmp();
        let e = env(HOSTS, &f.base, &store);
        for tier in ["standard", "large"] {
            let approved = AtomicUsize::new(0);
            let mut ev = Vec::new();
            assert_eq!(
                go(&e, tier, &approved, &mut ev).await.unwrap_err(),
                "not_published"
            );
            assert_eq!(approved.load(Ordering::SeqCst), 0);
            assert!(ev.is_empty());
        }
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
        // Not-published wins even with outbound off (a property of the manifest, checked first).
        let mut e2 = env(HOSTS, &f.base, &store);
        e2.outbound_disabled = true;
        assert_eq!(
            go(&e2, "standard", &AtomicUsize::new(0), &mut Vec::new())
                .await
                .unwrap_err(),
            "not_published"
        );
    }

    #[tokio::test]
    async fn unknown_tier_and_bad_manifest_are_fixed_codes() {
        let store = tmp();
        let e = env(HOSTS, "http://127.0.0.1:1", &store);
        assert_eq!(
            go(&e, "huge", &AtomicUsize::new(0), &mut Vec::new())
                .await
                .unwrap_err(),
            "tier_not_found"
        );
        let mut cb = |_: &Progress| {};
        let r = run_pull(
            &e,
            b"{}",
            &AcceptAll,
            "compact",
            |_t| async { Ok::<(), String>(()) },
            &mut cb,
        )
        .await;
        assert_eq!(r.unwrap_err(), "manifest_invalid");
    }

    #[tokio::test]
    async fn no_space_refuses_before_dialog_and_socket() {
        let f = good_fixture().await;
        let store = tmp();
        for fs in [
            (|_: &Path| Some(10u64)) as fn(&Path) -> Option<u64>,
            |_: &Path| None,
        ] {
            let mut e = env(HOSTS, &f.base, &store);
            e.free_space = fs;
            let approved = AtomicUsize::new(0);
            assert_eq!(
                go(&e, "compact", &approved, &mut Vec::new())
                    .await
                    .unwrap_err(),
                "no_space"
            );
            assert_eq!(approved.load(Ordering::SeqCst), 0);
        }
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn runtime_not_answering_is_runtime_unavailable_after_approval() {
        // A port nothing listens on.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        drop(l);
        let store = tmp();
        let e = env(HOSTS, &base, &store);
        let approved = AtomicUsize::new(0);
        let mut ev = Vec::new();
        assert_eq!(
            go(&e, "compact", &approved, &mut ev).await.unwrap_err(),
            RUNTIME_UNAVAILABLE
        );
        assert_eq!(approved.load(Ordering::SeqCst), 1);
        assert!(ev.is_empty());
    }

    #[tokio::test]
    async fn a_pull_runs_inside_the_window_and_restores_the_serve_never_overlapping() {
        let f = good_fixture().await;
        let store = tmp();
        let host = FakeHost::new(ServeState::Ours);
        let e = env_with(HOSTS, &f.base, &store, &host);
        let mut ev = Vec::new();
        let r = go(&e, "compact", &AtomicUsize::new(0), &mut ev).await;
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(
            host.log(),
            [
                "stop_serve",
                "spawn_window",
                "window_ready",
                "kill_window",
                "window_gone",
                "start_serve"
            ]
        );
        // The pull itself happened between window_ready and kill_window.
        assert!(!f.state.lock().unwrap().pulls.is_empty());
        assert!(!host.overlap.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn the_window_is_stopped_and_the_serve_restored_on_every_error_path() {
        // Runtime never answers, a bad stream, a digest mismatch.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dead = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        drop(l);
        let d = compact_digests();
        let bad_digest = serve(State {
            installs: d.keys().map(|n| (n.clone(), "0".repeat(64))).collect(),
            lines: stream_lines(d["gemma4:latest"].0),
            ..State::default()
        })
        .await;
        let bad_stream = serve(State {
            lines: vec!["not json".into()],
            ..State::default()
        })
        .await;
        for (base, want) in [
            (dead, RUNTIME_UNAVAILABLE),
            (bad_digest.base.clone(), "checksum_mismatch"),
            (bad_stream.base.clone(), "upstream_error"),
        ] {
            let store = tmp();
            let host = FakeHost::new(ServeState::Ours);
            let e = env_with(HOSTS, &base, &store, &host);
            let mut ev = Vec::new();
            let err = go(&e, "compact", &AtomicUsize::new(0), &mut ev)
                .await
                .unwrap_err();
            assert_eq!(err, want);
            assert!(host
                .log()
                .ends_with(&["kill_window", "window_gone", "start_serve"]));
            assert!(!host.overlap.load(Ordering::SeqCst));
            assert!(host.slot.held_by().is_none());
        }
    }

    #[tokio::test]
    async fn a_window_that_died_after_ready_is_runtime_unavailable_and_never_pulled_from() {
        // A squatter took :11435, our child lost the bind race and exited: nothing may be read from
        // or sent to that port.
        let f = good_fixture().await;
        let store = tmp();
        let mut host = FakeHost::new(ServeState::Ours);
        host.window_dies = true;
        let e = env_with(HOSTS, &f.base, &store, &host);
        let mut ev = Vec::new();
        assert_eq!(
            go(&e, "compact", &AtomicUsize::new(0), &mut ev)
                .await
                .unwrap_err(),
            RUNTIME_UNAVAILABLE
        );
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
        assert!(f.state.lock().unwrap().pulls.is_empty());
        assert!(host
            .log()
            .ends_with(&["kill_window", "window_gone", "start_serve"]));
    }

    #[test]
    fn the_approval_text_says_the_local_model_stops_during_the_download() {
        let src = include_str!("model_pull.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap();
        assert!(prod.contains("The local model stops during the download and restarts after."));
    }

    #[tokio::test]
    async fn a_cancelled_pull_kills_the_window_and_frees_the_slot() {
        let f = good_fixture().await;
        let store = tmp();
        let host = FakeHost::new(ServeState::Ours);
        let e = env_with(HOSTS, &f.base, &store, &host);
        let mut ev = Vec::new();
        let n = AtomicUsize::new(0);
        // Dropped right after the window opens (the first await inside the pull).
        let fut = go(&e, "compact", &n, &mut ev);
        let _ = tokio::time::timeout(Duration::from_millis(0), fut).await;
        assert!(!host.window_up.load(Ordering::SeqCst));
        assert!(host.slot.held_by().is_none());
    }

    #[tokio::test]
    async fn a_foreign_runtime_is_runtime_busy_after_approval_with_nothing_touched() {
        let f = good_fixture().await;
        let store = tmp();
        let host = FakeHost::new(ServeState::Foreign);
        let e = env_with(HOSTS, &f.base, &store, &host);
        let approved = AtomicUsize::new(0);
        let mut ev = Vec::new();
        assert_eq!(
            go(&e, "compact", &approved, &mut ev).await.unwrap_err(),
            "runtime_busy"
        );
        assert_eq!(approved.load(Ordering::SeqCst), 1);
        assert!(host.log().is_empty());
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_concurrent_pull_is_pull_in_progress_and_gates_run_before_the_lifecycle() {
        let f = good_fixture().await;
        let store = tmp();
        let host = FakeHost::new(ServeState::Absent);
        let _held = host
            .slot
            .try_acquire(super::super::pull_window::Holder::Download)
            .unwrap();
        let e = env_with(HOSTS, &f.base, &store, &host);
        let approved = AtomicUsize::new(0);
        let mut ev = Vec::new();
        assert_eq!(
            go(&e, "compact", &approved, &mut ev).await.unwrap_err(),
            "pull_in_progress"
        );
        assert!(host.log().is_empty());
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
        // Outbound off still refuses first, without touching the slot or the host.
        let mut e2 = env_with(HOSTS, &f.base, &store, &host);
        e2.outbound_disabled = true;
        let n = AtomicUsize::new(0);
        assert_eq!(
            go(&e2, "compact", &n, &mut ev).await.unwrap_err(),
            "outbound_blocked"
        );
        assert_eq!(n.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn runtime_error_lines_and_bad_streams_never_echo_text() {
        let d = compact_digests();
        let total = d["gemma4:latest"].0;
        let store = tmp();
        let secret = r#"{"error":"pull failed: http://secret.example/x sha256:deadbeef"}"#;
        let cases: Vec<Vec<String>> = vec![
            vec![secret.into()],
            // Stream ends without "success".
            stream_lines(total)[..4].to_vec(),
            // Not JSON.
            vec!["<html>http://secret.example</html>".into()],
        ];
        for lines in cases {
            let f = serve(State {
                installs: d.iter().map(|(n, (_, s))| (n.clone(), s.clone())).collect(),
                lines,
                ..State::default()
            })
            .await;
            let e = env(HOSTS, &f.base, &store);
            let mut ev = Vec::new();
            let err = go(&e, "compact", &AtomicUsize::new(0), &mut ev)
                .await
                .unwrap_err();
            assert_eq!(err, "upstream_error");
            let json = serde_json::to_string(&ev).unwrap();
            assert!(!json.contains("secret") && !json.contains("deadbeef"));
            assert_eq!(ev.last().unwrap().state, "failed");
        }
    }

    #[test]
    fn production_lists_and_command_uses_the_kill_switch() {
        assert_eq!(PROD_PULL_HOSTS, ["registry.ollama.ai:443"]);
        assert!(super::super::model_download::PROD_ALLOWED_HOSTS.is_empty());
        assert_eq!(
            super::super::loopback_http::loopback_base(DEFAULT_PULL_BASE).as_deref(),
            Some(DEFAULT_PULL_BASE)
        );
        let src = include_str!("model_pull.rs");
        let cmd = &src[src.find("pub(crate) async fn pull_command").unwrap()
            ..src.find("#[cfg(test)]").unwrap()];
        assert!(cmd.contains("super::telegram::outbound_disabled()"));
        assert!(cmd.contains("PROD_PULL_HOSTS") && cmd.contains("DEFAULT_PULL_BASE"));
    }

    #[test]
    fn tier_dispatch_follows_the_manifest_source() {
        let m = shipped();
        for t in ["compact", "standard", "large"] {
            assert!(tier_uses_pull(&m, t), "{t}");
        }
        assert!(!tier_uses_pull(&m, "nope"));
    }
}
