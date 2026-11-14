//! `model_download(tier)`: click-only, resumable download of a tier's models into
//! `<data_dir>/models/`. Gate order (nothing touches a socket before step 5):
//! 1 manifest verifies, 2 tier exists, 3 outbound not disabled (env + navy), 4 every URL's host is
//! on the allow-list (EMPTY in production until Taisun logs a host: every URL is refused with
//! `host_not_allowed`) and the manifest has size + SHA-256, 5 native approval naming host + size.
//! Data goes to `models/.partial/<sha256>` (HTTP Range resume, 0600, no symlinks), is fully
//! SHA-256 verified, then atomically renamed to `models/<sha256>`. A mismatch deletes only the
//! partial. Events `model://progress` and every error are fixed text with no URL.
//! Store layout vs Ollama `/api/pull` is still open (plan Q1): files are content-addressed.

use super::model_manifest::{self, ManifestModel, ManifestVerifier};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Emitter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const PROGRESS_EVENT: &str = "model://progress";
/// Hosts the app may download from, as `host:port` entries (for example `host.example:443`; a bare
/// host never matches). EMPTY in production until Taisun logs a host (a [GATE]).
pub(crate) const PROD_ALLOWED_HOSTS: &[&str] = &[];

const CHUNK_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Progress {
    pub role: String,
    /// waiting | downloading | verifying | done | failed
    pub state: &'static str,
    pub pct: u8,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSummary {
    pub tier: String,
    pub completed: u32,
}

/// Everything the core needs from outside, so tests inject their own.
pub(crate) struct Env<'a> {
    pub allowed_hosts: &'a [&'a str],
    pub outbound_disabled: bool,
    pub base: &'a Path,
    /// Tests only: allow plain http (production keeps https_only).
    pub allow_http: bool,
    /// Free bytes on the volume holding the given path (None = unknown, treated as no space).
    pub free_space: fn(&Path) -> Option<u64>,
}

/// Free bytes via statvfs on the nearest existing ancestor of `p`.
pub(crate) fn statvfs_free(p: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let mut cur = p;
    while !cur.exists() {
        cur = cur.parent()?;
    }
    let c = std::ffi::CString::new(cur.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: valid NUL-terminated path and a zeroed out-struct of the right type.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    (st.f_bavail as u64).checked_mul(st.f_frsize)
}

struct Job {
    role: String,
    url: reqwest::Url,
    sha: String,
    bytes: u64,
}

/// Entries are `host:port` (e.g. `host:443`); the URL's port (explicit or the scheme default) must
/// match. A bare host entry never matches.
fn host_allowed(allowed: &[&str], u: &reqwest::Url) -> bool {
    let (Some(h), Some(port)) = (u.host_str(), u.port_or_known_default()) else {
        return false;
    };
    let hp = format!("{h}:{port}");
    allowed.iter().any(|a| a.eq_ignore_ascii_case(&hp))
}

fn plan(models: &[ManifestModel], env: &Env) -> Result<Vec<Job>, &'static str> {
    let mut urls = Vec::new();
    for m in models {
        let u = reqwest::Url::parse(&m.url).map_err(|_| "host_not_allowed")?;
        let scheme_ok = u.scheme() == "https" || (env.allow_http && u.scheme() == "http");
        if !scheme_ok
            || !u.username().is_empty()
            || u.password().is_some()
            || !host_allowed(env.allowed_hosts, &u)
        {
            return Err("host_not_allowed");
        }
        urls.push(u);
    }
    let mut jobs = Vec::new();
    for (m, url) in models.iter().zip(urls) {
        let (Some(bytes), Some(sha)) = (m.bytes, m.sha256.clone()) else {
            return Err("not_published");
        };
        jobs.push(Job {
            role: m.role.clone(),
            url,
            sha,
            bytes,
        });
    }
    Ok(jobs)
}

/// Create (0700) or reuse a directory. Never a symlink. With `harden`, a reused directory must be
/// ours and is chmod 0700 if group/world accessible; someone else's is `unsafe_path`.
fn ensure_dir(p: &Path, harden: bool) -> Result<(), &'static str> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match p.symlink_metadata() {
        Ok(md) if md.is_dir() => {
            if harden {
                // SAFETY: geteuid has no preconditions.
                if md.uid() != unsafe { libc::geteuid() } {
                    return Err("unsafe_path");
                }
                if md.mode() & 0o077 != 0 {
                    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))
                        .map_err(|_| "unsafe_path")?;
                }
            }
            Ok(())
        }
        Ok(_) => Err("unsafe_path"),
        Err(_) => std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(p)
            .map_err(|_| "storage_error"),
    }
}

/// Hash through an O_NOFOLLOW handle (must be a regular file). Returns (sha, dev, ino).
async fn sha_of(p: &Path) -> Result<(String, u64, u64), &'static str> {
    use std::os::unix::fs::MetadataExt;
    let mut f = tokio::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(p)
        .await
        .map_err(|_| "unsafe_path")?;
    let md = f.metadata().await.map_err(|_| "storage_error")?;
    if !md.is_file() {
        return Err("unsafe_path");
    }
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).await.map_err(|_| "storage_error")?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok((hex::encode(h.finalize()), md.dev(), md.ino()))
}

fn pct(done: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    ((done.min(total) as u128 * 100) / total as u128) as u8
}

async fn fetch_one(
    client: &reqwest::Client,
    j: &Job,
    models_dir: &Path,
    partial_dir: &Path,
    free_space: fn(&Path) -> Option<u64>,
    progress: &mut (dyn FnMut(&Progress) + Send),
) -> Result<(), &'static str> {
    let p = |state, done| Progress {
        role: j.role.clone(),
        state,
        pct: pct(done, j.bytes),
        bytes_done: done,
        bytes_total: j.bytes,
    };
    let part = super::path_safety::safe_join(partial_dir, &j.sha).map_err(|_| "unsafe_path")?;
    let fin = super::path_safety::safe_join(models_dir, &j.sha).map_err(|_| "unsafe_path")?;
    if let Ok(md) = part.symlink_metadata() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: geteuid has no preconditions.
        if !md.is_file() || md.uid() != unsafe { libc::geteuid() } {
            return Err("unsafe_path");
        }
        if md.mode() & 0o077 != 0 {
            std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| "unsafe_path")?;
        }
    }
    let mut have = part.metadata().map(|m| m.len()).unwrap_or(0);
    if have > j.bytes {
        let _ = tokio::fs::remove_file(&part).await;
        have = 0;
    }
    if have < j.bytes {
        if free_space(models_dir).is_none_or(|f| f < j.bytes - have) {
            return Err("no_space");
        }
        let mut req = client.get(j.url.clone());
        if have > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let mut resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                "timed_out"
            } else {
                "upstream_error"
            }
        })?;
        let append = match resp.status().as_u16() {
            206 if have > 0 => true,
            200 => false,
            416 => {
                let _ = tokio::fs::remove_file(&part).await;
                return Err("upstream_error");
            }
            _ => return Err("upstream_error"),
        };
        let mut opts = tokio::fs::OpenOptions::new();
        opts.write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW);
        if append {
            opts.append(true);
        } else {
            opts.truncate(true);
            have = 0;
        }
        let mut f = opts.open(&part).await.map_err(|_| "unsafe_path")?;
        let mut done = have;
        let mut last = u8::MAX;
        progress(&p("downloading", done));
        loop {
            let chunk = tokio::time::timeout(CHUNK_TIMEOUT, resp.chunk())
                .await
                .map_err(|_| "timed_out")?
                .map_err(|_| "upstream_error")?;
            let Some(chunk) = chunk else { break };
            done += chunk.len() as u64;
            if done > j.bytes {
                drop(f);
                let _ = tokio::fs::remove_file(&part).await;
                return Err("too_large");
            }
            f.write_all(&chunk).await.map_err(|_| "storage_error")?;
            let now = pct(done, j.bytes);
            if now != last {
                last = now;
                progress(&p("downloading", done));
            }
        }
        f.flush().await.map_err(|_| "storage_error")?;
        f.sync_all().await.map_err(|_| "storage_error")?;
    }
    progress(&p(
        "verifying",
        j.bytes.min(part.metadata().map(|m| m.len()).unwrap_or(0)),
    ));
    let size = part.metadata().map(|m| m.len()).unwrap_or(0);
    let (digest, dev, ino) = sha_of(&part).await?;
    if size != j.bytes || digest != j.sha {
        // Only the partial goes; nothing else in models/ is touched.
        let _ = tokio::fs::remove_file(&part).await;
        return Err("checksum_mismatch");
    }
    {
        // The path must still be the exact file that was hashed.
        use std::os::unix::fs::MetadataExt;
        let md = part.symlink_metadata().map_err(|_| "unsafe_path")?;
        if !md.is_file() || md.dev() != dev || md.ino() != ino {
            return Err("unsafe_path");
        }
    }
    tokio::fs::rename(&part, &fin)
        .await
        .map_err(|_| "storage_error")?;
    progress(&p("done", j.bytes));
    Ok(())
}

fn approval_error(e: &str) -> &'static str {
    if e.starts_with(super::approval_gate::DECLINED_PREFIX) {
        "declined"
    } else if e.starts_with(super::approval_gate::BUSY_PREFIX) {
        "busy"
    } else {
        "approval_failed"
    }
}

fn fmt_size(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

/// The testable core. `approve` receives the dialog text (host + size) and returns a guard that is
/// held until the download ends.
pub(crate) async fn run<G, F, Fut>(
    env: &Env<'_>,
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
    if env.outbound_disabled {
        return Err("outbound_blocked");
    }
    let jobs = plan(&t.models, env)?;
    let mut hosts: Vec<String> = jobs
        .iter()
        .filter_map(|j| j.url.host_str().map(str::to_string))
        .collect();
    hosts.sort();
    hosts.dedup();
    let total: u64 = jobs.iter().fold(0u64, |a, j| a.saturating_add(j.bytes));
    if (env.free_space)(env.base).is_none_or(|f| f < total) {
        return Err("no_space");
    }
    let text = super::approval_gate::approval_text(
        &format!(
            "Download {} model file(s) ({})",
            jobs.len(),
            fmt_size(total)
        ),
        &format!("From: {}", hosts.join(", ")),
    )
    .map_err(|_| "approval_failed")?;
    let _guard = approve(text).await.map_err(|e| approval_error(&e))?;

    let models_dir = env.base.join("models");
    let partial_dir = models_dir.join(".partial");
    ensure_dir(env.base, false)?;
    ensure_dir(&models_dir, true)?;
    ensure_dir(&partial_dir, true)?;
    let client = super::shared::http::builder(super::shared::http::HttpSpec {
        https_only: !env.allow_http,
        ..super::shared::http::HttpSpec::loopback().connect_timeout(CONNECT_TIMEOUT)
    })
    .build()
    .map_err(|_| "storage_error")?;

    for j in &jobs {
        progress(&Progress {
            role: j.role.clone(),
            state: "waiting",
            pct: 0,
            bytes_done: 0,
            bytes_total: j.bytes,
        });
    }
    let mut completed = 0u32;
    for j in &jobs {
        if let Err(e) = fetch_one(
            &client,
            j,
            &models_dir,
            &partial_dir,
            env.free_space,
            progress,
        )
        .await
        {
            progress(&Progress {
                role: j.role.clone(),
                state: "failed",
                pct: 0,
                bytes_done: 0,
                bytes_total: j.bytes,
            });
            return Err(e);
        }
        completed += 1;
    }
    Ok(DownloadSummary {
        tier: tier.to_string(),
        completed,
    })
}

/// Click-only. The only error strings are the fixed codes above.
#[tauri::command]
#[specta::specta]
pub async fn model_download(
    app: tauri::AppHandle,
    tier: String,
) -> Result<DownloadSummary, String> {
    let base: PathBuf = crate::app_paths::data_dir();
    let env = Env {
        allowed_hosts: PROD_ALLOWED_HOSTS,
        outbound_disabled: super::telegram::outbound_disabled(),
        base: &base,
        allow_http: false,
        free_space: statvfs_free,
    };
    let handle = app.clone();
    let mut emit = move |p: &Progress| {
        let _ = handle.emit(PROGRESS_EVENT, p);
    };
    run(
        &env,
        model_manifest_bytes(),
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

fn model_manifest_bytes() -> &'static [u8] {
    model_manifest::shipped_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::net::TcpListener;

    struct AcceptAll;
    impl ManifestVerifier for AcceptAll {
        fn verify(&self, _: &[u8]) -> bool {
            true
        }
    }

    struct Fixture {
        port: u16,
        conns: Arc<AtomicUsize>,
        ranges: Arc<Mutex<Vec<String>>>,
    }

    async fn serve(body: Vec<u8>) -> Fixture {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let conns = Arc::new(AtomicUsize::new(0));
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let (c, r) = (conns.clone(), ranges.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                c.fetch_add(1, Ordering::SeqCst);
                let (body, r) = (body.clone(), r.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                    let start = req
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|v| v.trim_end_matches('-').parse::<usize>().ok());
                    if let Some(s0) = start {
                        r.lock().unwrap().push(format!("{s0}-"));
                    }
                    let (status, part) = match start {
                        Some(s0) => ("206 Partial Content", &body[s0.min(body.len())..]),
                        None => ("200 OK", &body[..]),
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        part.len()
                    );
                    let _ = s.write_all(head.as_bytes()).await;
                    let _ = s.write_all(part).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        Fixture {
            port,
            conns,
            ranges,
        }
    }

    fn manifest_for(port: u16, body: &[u8], sha: &str) -> Vec<u8> {
        let one = |role: &str| {
            format!(
                "{{\"role\":\"{role}\",\"name\":\"gemma4:latest\",\"bytes\":{},\"sha256\":\"{sha}\",\"url\":\"http://127.0.0.1:{port}/m.bin\"}}",
                body.len()
            )
        };
        format!(
            "{{\"schema\":1,\"issued\":\"t\",\"tiers\":[{{\"id\":\"compact\",\"name\":\"Compact\",\"minRamGb\":8,\"models\":[{}]}}]}}",
            one("fast")
        )
        .into_bytes()
    }

    fn body() -> Vec<u8> {
        (0..200_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha256::digest(b))
    }

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-mdl-{tag}-{}-{}",
            std::process::id(),
            &sha(tag.as_bytes())[..8]
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    async fn go(
        env: &Env<'_>,
        m: &[u8],
        approved: &AtomicUsize,
        events: &mut Vec<Progress>,
    ) -> Result<DownloadSummary, &'static str> {
        let mut cb = |p: &Progress| events.push(p.clone());
        run(
            env,
            m,
            &AcceptAll,
            "compact",
            |_t| async {
                approved.fetch_add(1, Ordering::SeqCst);
                Ok::<(), String>(())
            },
            &mut cb,
        )
        .await
    }

    #[tokio::test]
    async fn downloads_verifies_and_reports_monotonic_progress_without_url() {
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let base = tmp("ok");
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        let env = Env {
            allowed_hosts: &hosts,
            outbound_disabled: false,
            base: &base,
            allow_http: true,
            free_space: |_| Some(u64::MAX),
        };
        let approved = AtomicUsize::new(0);
        let mut ev = Vec::new();
        let r = go(&env, &m, &approved, &mut ev).await.unwrap();
        assert_eq!(r.completed, 1);
        assert_eq!(approved.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(base.join("models").join(sha(&b))).unwrap(), b);
        assert!(!base.join("models/.partial").join(sha(&b)).exists());
        let dl: Vec<u64> = ev
            .iter()
            .filter(|e| e.state == "downloading")
            .map(|e| e.bytes_done)
            .collect();
        assert!(dl.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(ev.last().unwrap().state, "done");
        assert_eq!(ev.last().unwrap().pct, 100);
        let json = serde_json::to_string(&ev).unwrap();
        assert!(
            !json.contains("127.0.0.1") && !json.contains("http") && json.contains("bytesDone")
        );
    }

    #[tokio::test]
    async fn refused_when_outbound_disabled_with_zero_connections() {
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let base = tmp("nav");
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        let env = Env {
            allowed_hosts: &hosts,
            outbound_disabled: true,
            base: &base,
            allow_http: true,
            free_space: |_| Some(u64::MAX),
        };
        let approved = AtomicUsize::new(0);
        let mut ev = Vec::new();
        assert_eq!(
            go(&env, &m, &approved, &mut ev).await.unwrap_err(),
            "outbound_blocked"
        );
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
        assert_eq!(approved.load(Ordering::SeqCst), 0);
        assert!(ev.is_empty());
        assert!(!base.join("models").exists());
    }

    #[tokio::test]
    async fn invalid_manifest_wins_over_outbound_and_unknown_tier_is_reported() {
        let base = tmp("ord");
        let env = Env {
            allowed_hosts: &[],
            outbound_disabled: true,
            base: &base,
            allow_http: true,
            free_space: |_| Some(u64::MAX),
        };
        let mut cb = |_: &Progress| {};
        let r = run(
            &env,
            b"{}",
            &AcceptAll,
            "compact",
            |_t| async { Ok::<(), String>(()) },
            &mut cb,
        )
        .await;
        assert_eq!(r.unwrap_err(), "manifest_invalid");
        let m = manifest_for(1, b"x", &sha(b"x"));
        let r = run(
            &env,
            &m,
            &AcceptAll,
            "large",
            |_t| async { Ok::<(), String>(()) },
            &mut cb,
        )
        .await;
        assert_eq!(r.unwrap_err(), "tier_not_found");
    }

    #[tokio::test]
    async fn host_not_on_list_is_refused_before_any_socket_or_dialog() {
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let base = tmp("host");
        for hosts in [vec![], vec!["example.invalid"]] {
            let env = Env {
                allowed_hosts: &hosts,
                outbound_disabled: false,
                base: &base,
                allow_http: true,
                free_space: |_| Some(u64::MAX),
            };
            let approved = AtomicUsize::new(0);
            let mut ev = Vec::new();
            assert_eq!(
                go(&env, &m, &approved, &mut ev).await.unwrap_err(),
                "host_not_allowed"
            );
            assert_eq!(approved.load(Ordering::SeqCst), 0);
        }
        // http is refused outright when allow_http is off (production).
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        let env = Env {
            allowed_hosts: &hosts,
            outbound_disabled: false,
            base: &base,
            allow_http: false,
            free_space: |_| Some(u64::MAX),
        };
        let approved = AtomicUsize::new(0);
        assert_eq!(
            go(&env, &m, &approved, &mut Vec::new()).await.unwrap_err(),
            "host_not_allowed"
        );
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn production_allow_list_is_empty_and_command_uses_the_kill_switch() {
        assert!(PROD_ALLOWED_HOSTS.is_empty());
        let src = include_str!("model_download.rs");
        let cmd = &src[src.find("pub async fn model_download").unwrap()
            ..src.find("fn model_manifest_bytes").unwrap()];
        assert!(cmd.contains("super::telegram::outbound_disabled()"));
        assert!(cmd.contains("allow_http: false"));
        assert!(cmd.contains("PROD_ALLOWED_HOSTS"));
    }

    #[tokio::test]
    async fn declined_approval_opens_no_connection() {
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let base = tmp("deny");
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        let env = Env {
            allowed_hosts: &hosts,
            outbound_disabled: false,
            base: &base,
            allow_http: true,
            free_space: |_| Some(u64::MAX),
        };
        let mut cb = |_: &Progress| {};
        let r = run(
            &env,
            &m,
            &AcceptAll,
            "compact",
            |t| async move {
                assert!(t.contains("127.0.0.1") && t.contains("GB") && !t.contains("/m.bin"));
                Err::<(), String>(super::super::approval_gate::denied_message())
            },
            &mut cb,
        )
        .await;
        assert_eq!(r.unwrap_err(), "declined");
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn resumes_after_truncated_partial() {
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let base = tmp("resume");
        let part_dir = base.join("models/.partial");
        std::fs::create_dir_all(&part_dir).unwrap();
        std::fs::write(part_dir.join(sha(&b)), &b[..70_000]).unwrap();
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        let env = Env {
            allowed_hosts: &hosts,
            outbound_disabled: false,
            base: &base,
            allow_http: true,
            free_space: |_| Some(u64::MAX),
        };
        let mut ev = Vec::new();
        go(&env, &m, &AtomicUsize::new(0), &mut ev).await.unwrap();
        assert_eq!(f.ranges.lock().unwrap().as_slice(), ["70000-"]);
        assert_eq!(std::fs::read(base.join("models").join(sha(&b))).unwrap(), b);
        assert!(ev
            .iter()
            .filter(|e| e.state == "downloading")
            .all(|e| e.bytes_done >= 70_000));
    }

    #[tokio::test]
    async fn checksum_mismatch_deletes_only_the_partial() {
        let b = body();
        let wrong_sha = sha(b"something else");
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &wrong_sha);
        let base = tmp("bad");
        let models = base.join("models");
        std::fs::create_dir_all(models.join(".partial")).unwrap();
        std::fs::write(models.join("keepme"), b"other model").unwrap();
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        let env = Env {
            allowed_hosts: &hosts,
            outbound_disabled: false,
            base: &base,
            allow_http: true,
            free_space: |_| Some(u64::MAX),
        };
        let mut ev = Vec::new();
        assert_eq!(
            go(&env, &m, &AtomicUsize::new(0), &mut ev)
                .await
                .unwrap_err(),
            "checksum_mismatch"
        );
        assert!(!models.join(".partial").join(&wrong_sha).exists());
        assert!(!models.join(&wrong_sha).exists());
        assert_eq!(
            std::fs::read(models.join("keepme")).unwrap(),
            b"other model"
        );
        assert_eq!(ev.last().unwrap().state, "failed");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinks_and_traversal_are_refused() {
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        // models/.partial is a symlink out of the tree.
        let base = tmp("sym1");
        let outside = tmp("sym1-out");
        std::fs::create_dir_all(base.join("models")).unwrap();
        std::os::unix::fs::symlink(&outside, base.join("models/.partial")).unwrap();
        let env = Env {
            allowed_hosts: &hosts,
            outbound_disabled: false,
            base: &base,
            allow_http: true,
            free_space: |_| Some(u64::MAX),
        };
        assert_eq!(
            go(&env, &m, &AtomicUsize::new(0), &mut Vec::new())
                .await
                .unwrap_err(),
            "unsafe_path"
        );
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
        // the partial file itself is a symlink.
        let base = tmp("sym2");
        let victim = tmp("sym2-victim").join("victim");
        std::fs::write(&victim, b"keep").unwrap();
        std::fs::create_dir_all(base.join("models/.partial")).unwrap();
        std::os::unix::fs::symlink(&victim, base.join("models/.partial").join(sha(&b))).unwrap();
        let env = Env {
            allowed_hosts: &hosts,
            outbound_disabled: false,
            base: &base,
            allow_http: true,
            free_space: |_| Some(u64::MAX),
        };
        assert_eq!(
            go(&env, &m, &AtomicUsize::new(0), &mut Vec::new())
                .await
                .unwrap_err(),
            "unsafe_path"
        );
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        // a traversal "digest" never reaches the filesystem: the manifest is invalid.
        let evil = String::from_utf8(m.clone())
            .unwrap()
            .replace(&sha(&b), "../../../etc/passwd");
        let mut cb = |_: &Progress| {};
        let r = run(
            &env,
            evil.as_bytes(),
            &AcceptAll,
            "compact",
            |_t| async { Ok::<(), String>(()) },
            &mut cb,
        )
        .await;
        assert_eq!(r.unwrap_err(), "manifest_invalid");
        assert!(super::super::path_safety::safe_join(&base, "..").is_err());
    }

    #[tokio::test]
    async fn unpublished_size_or_digest_is_refused_not_guessed() {
        let base = tmp("unpub");
        let m = br#"{"schema":1,"issued":"t","tiers":[{"id":"compact","name":"Compact","minRamGb":8,"models":[{"role":"fast","name":"gemma4:latest","bytes":null,"sha256":null,"url":"http://127.0.0.1:9/m.bin"}]}]}"#;
        let env = Env {
            allowed_hosts: &["127.0.0.1:9"],
            outbound_disabled: false,
            base: &base,
            allow_http: true,
            free_space: |_| Some(u64::MAX),
        };
        assert_eq!(
            go(&env, m, &AtomicUsize::new(0), &mut Vec::new())
                .await
                .unwrap_err(),
            "not_published"
        );
    }

    fn env_with<'a>(
        hosts: &'a [&'a str],
        base: &'a Path,
        free_space: fn(&Path) -> Option<u64>,
    ) -> Env<'a> {
        Env {
            allowed_hosts: hosts,
            outbound_disabled: false,
            base,
            allow_http: true,
            free_space,
        }
    }

    #[tokio::test]
    async fn no_space_refuses_before_dialog_and_socket() {
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let base = tmp("nospace");
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        for fs in [
            (|_: &Path| Some(10u64)) as fn(&Path) -> Option<u64>,
            |_: &Path| None,
        ] {
            let env = env_with(&hosts, &base, fs);
            let approved = AtomicUsize::new(0);
            assert_eq!(
                go(&env, &m, &approved, &mut Vec::new()).await.unwrap_err(),
                "no_space"
            );
            assert_eq!(approved.load(Ordering::SeqCst), 0);
        }
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
        assert!(!base.join("models").exists());
    }

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    #[tokio::test]
    async fn no_space_is_rechecked_before_each_file() {
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let base = tmp("nospace2");
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        // First call (pre-approval) has room; the per-file call does not (space was used meanwhile).
        CALLS.store(0, Ordering::SeqCst);
        let env = env_with(&hosts, &base, |_| {
            if CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
                Some(u64::MAX)
            } else {
                Some(5)
            }
        });
        assert_eq!(
            go(&env, &m, &AtomicUsize::new(0), &mut Vec::new())
                .await
                .unwrap_err(),
            "no_space"
        );
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn host_entries_need_an_explicit_matching_port() {
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let base = tmp("port");
        let wrong = format!("127.0.0.1:{}", f.port.wrapping_add(1));
        for hosts in [vec!["127.0.0.1"], vec![wrong.as_str()]] {
            let env = env_with(&hosts, &base, |_| Some(u64::MAX));
            assert_eq!(
                go(&env, &m, &AtomicUsize::new(0), &mut Vec::new())
                    .await
                    .unwrap_err(),
                "host_not_allowed"
            );
        }
        assert_eq!(f.conns.load(Ordering::SeqCst), 0);
        let u = reqwest::Url::parse("https://models.example.com/x").unwrap();
        assert!(host_allowed(&["models.example.com:443"], &u));
        assert!(!host_allowed(&["models.example.com"], &u));
        assert!(!host_allowed(&["models.example.com:8443"], &u));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reused_dirs_and_partial_are_hardened_and_symlink_hash_refused() {
        use std::os::unix::fs::PermissionsExt;
        let b = body();
        let f = serve(b.clone()).await;
        let m = manifest_for(f.port, &b, &sha(&b));
        let base = tmp("perm");
        let part_dir = base.join("models/.partial");
        std::fs::create_dir_all(&part_dir).unwrap();
        for d in [base.join("models"), part_dir.clone()] {
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o777)).unwrap();
        }
        let pf = part_dir.join(sha(&b));
        std::fs::write(&pf, &b[..1000]).unwrap();
        std::fs::set_permissions(&pf, std::fs::Permissions::from_mode(0o666)).unwrap();
        let hosts = [format!("127.0.0.1:{}", f.port)];
        let hosts: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
        let env = env_with(&hosts, &base, |_| Some(u64::MAX));
        go(&env, &m, &AtomicUsize::new(0), &mut Vec::new())
            .await
            .unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&base.join("models")), 0o700);
        assert_eq!(mode(&part_dir), 0o700);
        assert_eq!(mode(&base.join("models").join(sha(&b))), 0o600);
        // sha_of never follows a symlink.
        let link = base.join("link");
        std::os::unix::fs::symlink(base.join("models").join(sha(&b)), &link).unwrap();
        assert_eq!(sha_of(&link).await.unwrap_err(), "unsafe_path");
        assert_eq!(sha_of(&base).await.unwrap_err(), "unsafe_path");
    }
}
