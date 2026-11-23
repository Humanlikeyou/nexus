//! Amara Voice (the local "Alexa" loop in ~/projects/amara-voice): status, start/stop, recent turns, voice picker.
//!
//! Amara OS only observes and switches it; the loop itself stays a separate Python project.
//! Files it reads (written by amara-voice):
//!   logs/state        one word: loading | idle | listening | heard | thinking | speaking | error | stopped
//!   logs/turns.jsonl  one JSON object per turn (heard, route, reply, latencies, barged, ...)
//!   config.json       {"voice": "<name>"}, read by amara_voice.py at start (voice_config.py); written here
//!   voices/<name>.wav + voices/<name>.txt   the usable voices
//!
//! Paths: `NEXUS_AMARA_VOICE_DIR` / `NEXUS_AMARA_VOICE_PYTHON`, else `amara_voice_dir` / `amara_voice_python`
//! in the app's config.json, else ~/projects/amara-voice and ~/projects/ming-mac/.venv/bin/python.
//! Every process is started with a fixed argv (no shell string is ever built).

use super::shared::task::{blocking, JoinErr};
use serde::Serialize;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const LAUNCHD_LABEL: &str = "com.thevisualdude.amara-voice";
const DEFAULT_VOICE: &str = "neosoul_g";
const STATES: &[&str] = &[
    "loading",
    "idle",
    "listening",
    "heard",
    "thinking",
    "speaking",
    "error",
    "stopped",
];
const MAX_TURNS: usize = 200;
const TAIL_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct VoicePaths {
    pub dir: PathBuf,
    pub python: PathBuf,
}

impl VoicePaths {
    fn script(&self) -> PathBuf {
        self.dir.join("amara_voice.py")
    }
    fn state_file(&self) -> PathBuf {
        self.dir.join("logs").join("state")
    }
    fn turns_file(&self) -> PathBuf {
        self.dir.join("logs").join("turns.jsonl")
    }
    fn config_file(&self) -> PathBuf {
        self.dir.join("config.json")
    }
    fn voices_dir(&self) -> PathBuf {
        self.dir.join("voices")
    }
    fn installed(&self) -> bool {
        self.script().is_file()
    }
}

fn setting(env_key: &str, config_key: &str) -> Option<PathBuf> {
    if let Some(v) = std::env::var(env_key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return Some(PathBuf::from(v));
    }
    let raw = fs::read_to_string(crate::app_paths::data_dir().join("config.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get(config_key)
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn voice_paths() -> VoicePaths {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    VoicePaths {
        dir: setting("NEXUS_AMARA_VOICE_DIR", "amara_voice_dir")
            .unwrap_or_else(|| home.join("projects").join("amara-voice")),
        python: setting("NEXUS_AMARA_VOICE_PYTHON", "amara_voice_python").unwrap_or_else(|| {
            home.join("projects")
                .join("ming-mac")
                .join(".venv")
                .join("bin")
                .join("python")
        }),
    }
}

// ---------- process discovery ----------

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoopProc {
    pub pid: u32,
    pub rss_kb: u64,
}

/// Parse `ps -axo pid=,rss=,args=`: the Python process running amara_voice.py (not a shell or pgrep
/// whose command line merely mentions it).
pub(crate) fn find_loop(ps_out: &str) -> Option<LoopProc> {
    ps_out.lines().find_map(|line| {
        let mut it = line.split_whitespace();
        let pid: u32 = it.next()?.parse().ok()?;
        let rss_kb: u64 = it.next()?.parse().ok()?;
        let exe = it.next()?;
        let args: Vec<&str> = it.collect();
        let exe_name = Path::new(exe)
            .file_name()?
            .to_string_lossy()
            .to_ascii_lowercase();
        let is_python = exe_name.starts_with("python");
        let runs_loop = args.iter().any(|a| {
            Path::new(a)
                .file_name()
                .is_some_and(|f| f == "amara_voice.py")
        });
        (is_python && runs_loop).then_some(LoopProc { pid, rss_kb })
    })
}

fn running_loop() -> Option<LoopProc> {
    let out = Command::new("/bin/ps")
        .args(["-axo", "pid=,rss=,args="])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    find_loop(&String::from_utf8_lossy(&out.stdout))
}

fn launchd_plist() -> Option<PathBuf> {
    let p = dirs::home_dir()?
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist"));
    p.is_file().then_some(p)
}

fn launchd_loaded() -> bool {
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    Command::new("/bin/launchctl")
        .args(["print", &format!("gui/{uid}/{LAUNCHD_LABEL}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ---------- state / voices / turns ----------

/// The loop's state word; anything unexpected (or a file left behind by a dead loop) is reported honestly.
pub(crate) fn normalize_state(raw: Option<&str>, running: bool) -> String {
    if !running {
        return "stopped".into();
    }
    match raw.map(str::trim) {
        Some(s) if STATES.contains(&s) => s.to_string(),
        Some("quit") => "stopped".into(),
        _ => "loading".into(), // running but nothing written yet
    }
}

pub(crate) fn current_voice(p: &VoicePaths) -> String {
    fs::read_to_string(p.config_file())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("voice").and_then(|x| x.as_str()).map(String::from))
        .filter(|v| crate::commands::path_safety::validate_name(v).is_ok())
        .unwrap_or_else(|| DEFAULT_VOICE.to_string())
}

pub(crate) fn list_voices_in(p: &VoicePaths) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(p.voices_dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let path = e.path();
                    if path.extension()? != "wav" || !e.file_type().ok()?.is_file() {
                        return None;
                    }
                    let stem = path.file_stem()?.to_str()?.to_string();
                    crate::commands::path_safety::validate_name(&stem).ok()?;
                    path.with_extension("txt").is_file().then_some(stem)
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

pub(crate) fn set_voice_in(p: &VoicePaths, name: &str) -> Result<(), String> {
    crate::commands::path_safety::validate_name(name)?;
    if !list_voices_in(p).iter().any(|v| v == name) {
        return Err(format!(
            "No voice named {name} (needs voices/{name}.wav and .txt)"
        ));
    }
    let path = p.config_file();
    let mut cfg = fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    cfg["voice"] = serde_json::Value::String(name.to_string());
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())? + "\n";
    fs::write(&tmp, body).map_err(|e| format!("Could not write voice setting: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("Could not save voice setting: {e}"))
}

/// Last `n` JSON objects of turns.jsonl, oldest first. Reads only the file's tail; bad lines are skipped.
pub(crate) fn tail_turns(path: &Path, n: usize) -> Vec<serde_json::Value> {
    let Ok(mut f) = fs::File::open(path) else {
        return vec![];
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(TAIL_BYTES);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // probably cut mid-line
    }
    let mut out: Vec<serde_json::Value> = lines
        .iter()
        .rev()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.is_object())
        .take(n.clamp(1, MAX_TURNS))
        .collect();
    out.reverse();
    out
}

// ---------- commands ----------

#[derive(Debug, Serialize, Clone, specta::Type)]
pub struct VoiceStatus {
    pub installed: bool,
    pub running: bool,
    pub pid: Option<u32>,
    /// loading | idle | listening | heard | thinking | speaking | error | stopped | not_installed
    pub state: String,
    /// Seconds since logs/state last changed
    pub state_age_s: Option<u64>,
    pub voice: String,
    pub rss_mb: Option<f64>,
    /// Installed as a LaunchAgent (`amara install`); start/stop then go through launchd
    pub launchd: bool,
    pub dir: String,
}

fn status_for(p: &VoicePaths) -> VoiceStatus {
    let installed = p.installed();
    let proc = if installed { running_loop() } else { None };
    let running = proc.is_some();
    let raw = fs::read_to_string(p.state_file()).ok();
    let age = fs::metadata(p.state_file())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs());
    VoiceStatus {
        installed,
        running,
        pid: proc.as_ref().map(|x| x.pid),
        state: if installed {
            normalize_state(raw.as_deref(), running)
        } else {
            "not_installed".into()
        },
        state_age_s: age,
        voice: current_voice(p),
        rss_mb: proc.map(|x| (x.rss_kb as f64 / 1024.0 * 10.0).round() / 10.0),
        launchd: launchd_plist().is_some(),
        dir: p.dir.display().to_string(),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn voice_status() -> Result<VoiceStatus, String> {
    blocking(JoinErr::Prefix("voice task failed: "), || {
        Ok(status_for(&voice_paths()))
    })
    .await
}

fn start_loop(p: &VoicePaths) -> Result<(), String> {
    if !p.installed() {
        return Err(format!(
            "Amara Voice isn't installed at {}",
            p.dir.display()
        ));
    }
    if running_loop().is_some() {
        return Ok(());
    }
    if launchd_plist().is_some() {
        let cli = p.dir.join("amara");
        let st = Command::new(&cli)
            .arg("start")
            .current_dir(&p.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("Could not run {}: {e}", cli.display()))?;
        return if st.success() {
            Ok(())
        } else {
            Err("amara start failed (see: amara logs)".into())
        };
    }
    if !p.python.is_file() {
        return Err(format!("Python not found at {}", p.python.display()));
    }
    // Same environment as the LaunchAgent (service/com.thevisualdude.amara-voice.plist).
    let venv_bin = p
        .python
        .parent()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let path_env =
        format!("{venv_bin}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin");
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p.dir.join("amara_voice.log"))
        .map_err(|e| format!("Could not open amara_voice.log: {e}"))?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    use std::os::unix::process::CommandExt;
    let mut child = Command::new(&p.python)
        .args(["-u", "amara_voice.py"])
        .current_dir(&p.dir)
        .env("PATH", path_env)
        .env("PYTHONWARNINGS", "ignore")
        .env("HF_HUB_OFFLINE", "1")
        // Where amara-voice finds the navy local-only flag file (telegram::NAVY_FLAG_FILE).
        .env("NEXUS_DATA_DIR", crate::app_paths::data_dir())
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err)
        .process_group(0) // its own group: closing Amara OS doesn't take the voice loop down
        .spawn()
        .map_err(|e| format!("Could not start Amara Voice: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait(); // reap it when it exits, no zombie
    });
    Ok(())
}

fn stop_loop(p: &VoicePaths) -> Result<(), String> {
    if launchd_plist().is_some() && launchd_loaded() {
        let _ = Command::new(p.dir.join("amara"))
            .arg("stop")
            .current_dir(&p.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let Some(proc) = running_loop() else {
        return Ok(());
    };
    let pid = i32::try_from(proc.pid).map_err(|_| "bad pid".to_string())?;
    // SAFETY: plain kill(2) on a pid just verified (via ps) to be the amara_voice.py Python process.
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        if running_loop().is_none_or(|x| x.pid != proc.pid) {
            return Ok(());
        }
    }
    Err("Amara Voice didn't stop within 4 s".into())
}

// ---------- stop speaking (SIGUSR1) ----------

/// Process list + signalling, injectable so tests never touch a real process.
pub(crate) trait ProcOps {
    /// `ps -U <uid> -o pid=,etime=,args=` output for this user's processes only.
    fn list_own(&self) -> Option<String>;
    fn sigusr1(&self, pid: u32) -> Result<(), String>;
}

struct RealProcs;

impl ProcOps for RealProcs {
    fn list_own(&self) -> Option<String> {
        // SAFETY: getuid(2) has no preconditions.
        let uid = unsafe { libc::getuid() };
        let out = Command::new("/bin/ps")
            .args(["-U", &uid.to_string(), "-o", "pid=,etime=,args="])
            .stdin(Stdio::null())
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn sigusr1(&self, pid: u32) -> Result<(), String> {
        let pid = i32::try_from(pid).map_err(|_| "bad pid".to_string())?;
        // SAFETY: plain kill(2) on a pid the caller verified (ps, own uid) to be amara_voice.py.
        let rc = unsafe { libc::kill(pid, libc::SIGUSR1) };
        if rc == 0 {
            Ok(())
        } else {
            Err("Could not signal Amara Voice".into())
        }
    }
}

/// `ps` etime ([[dd-]hh:]mm:ss) in seconds.
fn parse_etime(s: &str) -> Option<u64> {
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, s),
    };
    let parts: Vec<u64> = rest
        .split(':')
        .map(|p| p.parse::<u64>().ok())
        .collect::<Option<_>>()?;
    let secs = match parts.as_slice() {
        [m, s] => m * 60 + s,
        [h, m, s] => h * 3600 + m * 60 + s,
        _ => return None,
    };
    Some(days * 86_400 + secs)
}

/// The amara_voice.py loop among `ps -o pid=,etime=,args=` lines, with its age in seconds.
pub(crate) fn find_own_loop(ps_out: &str) -> Option<(LoopProc, u64)> {
    ps_out.lines().find_map(|line| {
        let mut it = line.split_whitespace();
        let pid = it.next()?;
        let etime = it.next()?;
        let rest: Vec<&str> = it.collect();
        let proc = find_loop(&format!("{pid} 0 {}", rest.join(" ")))?;
        Some((proc, parse_etime(etime)?))
    })
}

/// Send SIGUSR1 (interrupt the current reply only) to the running amara_voice.py loop.
/// Ok(false) when it is not running, or when it predates the stop-speaking patch: without a
/// SIGUSR1 handler that signal would terminate the loop, so it is never sent then.
pub(crate) fn stop_speaking_with(
    ops: &dyn ProcOps,
    script: &Path,
    handler_files: &[PathBuf],
) -> Result<bool, String> {
    let Some((proc, age_secs)) = ops.list_own().and_then(|o| find_own_loop(&o)) else {
        return Ok(false);
    };
    let patched = |f: &PathBuf| fs::read_to_string(f).is_ok_and(|t| t.contains("SIGUSR1"));
    if !fs::metadata(script).is_ok_and(|m| m.is_file()) || !handler_files.iter().any(patched) {
        return Ok(false);
    }
    // The running process must have started after the patched file was written.
    let started = SystemTime::now()
        .checked_sub(Duration::from_secs(age_secs))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let newest = handler_files
        .iter()
        .chain(std::iter::once(&script.to_path_buf()))
        .filter_map(|f| fs::metadata(f).ok()?.modified().ok())
        .max();
    if newest.is_some_and(|m| m > started + Duration::from_secs(2)) {
        return Ok(false);
    }
    ops.sigusr1(proc.pid)?;
    Ok(true)
}

/// Interrupt what Amara Voice is saying right now; the loop keeps running. Ok(false) = nothing sent.
#[tauri::command]
#[specta::specta]
pub async fn voice_stop_speaking() -> Result<bool, String> {
    blocking(JoinErr::Prefix("voice task failed: "), || {
        let p = voice_paths();
        stop_speaking_with(
            &RealProcs,
            &p.script(),
            &[p.script(), p.dir.join("streaming.py")],
        )
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn voice_start() -> Result<VoiceStatus, String> {
    blocking(JoinErr::Prefix("voice task failed: "), || {
        let p = voice_paths();
        start_loop(&p)?;
        std::thread::sleep(Duration::from_millis(600));
        Ok(status_for(&p))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn voice_stop() -> Result<VoiceStatus, String> {
    blocking(JoinErr::Prefix("voice task failed: "), || {
        let p = voice_paths();
        stop_loop(&p)?;
        Ok(status_for(&p))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn voice_recent_turns(n: Option<usize>) -> Result<Vec<serde_json::Value>, String> {
    blocking(JoinErr::Prefix("voice task failed: "), move || {
        Ok(tail_turns(&voice_paths().turns_file(), n.unwrap_or(20)))
    })
    .await
}

#[derive(Debug, Serialize, Clone, specta::Type)]
pub struct VoiceList {
    pub voices: Vec<String>,
    pub current: String,
}

#[tauri::command]
#[specta::specta]
pub async fn voice_list_voices() -> Result<VoiceList, String> {
    blocking(JoinErr::Prefix("voice task failed: "), || {
        let p = voice_paths();
        Ok(VoiceList {
            voices: list_voices_in(&p),
            current: current_voice(&p),
        })
    })
    .await
}

/// Saves the voice for the next start; the panel offers a restart when the loop is running.
#[tauri::command]
#[specta::specta]
pub async fn voice_set_voice(name: String) -> Result<VoiceList, String> {
    blocking(JoinErr::Prefix("voice task failed: "), move || {
        let p = voice_paths();
        set_voice_in(&p, &name)?;
        Ok(VoiceList {
            voices: list_voices_in(&p),
            current: current_voice(&p),
        })
    })
    .await
}

// ---------- hand-off file safety (speak + dictation) ----------

/// A request nobody consumed for this long is stale: removed, so it cannot block the hand-off forever.
pub(crate) const REQUEST_STALE: Duration = Duration::from_secs(20);

/// The hand-off dir must be a real directory (never a symlink), owned by `uid`, with no group/other write.
pub(crate) fn hand_off_dir_ok_for(dir: &Path, uid: u32) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    fs::symlink_metadata(dir).is_ok_and(|m| {
        m.file_type().is_dir() && m.uid() == uid && m.permissions().mode() & 0o022 == 0
    })
}

fn hand_off_dir_ok(dir: &Path) -> bool {
    // SAFETY: getuid has no preconditions and cannot fail.
    hand_off_dir_ok_for(dir, unsafe { libc::getuid() })
}

/// True when a live (not stale) request file is waiting. A stale one (mtime older than REQUEST_STALE)
/// is removed and does not count. A symlink or non-file at that path counts as waiting-and-unusable.
pub(crate) fn request_pending(target: &Path, stale_after: Duration) -> bool {
    let Ok(m) = fs::symlink_metadata(target) else {
        return false;
    };
    let old = m
        .modified()
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age > stale_after);
    if old && !m.file_type().is_dir() {
        let _ = fs::remove_file(target);
        return false;
    }
    true
}

// ---------- local speak (navy mode, voice-opened only) ----------
//
// Amara Voice has no "say this" entry today. The contract (tasks/proposed/amara-voice-speak.diff,
// NOT yet applied to the live loop): a patched loop creates <dir>/speak/ and writes its pid to
// <dir>/logs/speak_ready; the app drops ONE request file, <dir>/speak/request.json, atomically
// (tmp + rename); the loop reads, deletes and speaks it in Ming's voice. File only, no network.
// Until the patch is applied there is no ready marker, so everything here answers false (inert).

pub(crate) const SPEAK_MAX_CHARS: usize = 240;

impl VoicePaths {
    fn speak_dir(&self) -> PathBuf {
        self.dir.join("speak")
    }
    fn speak_ready_file(&self) -> PathBuf {
        self.dir.join("logs").join("speak_ready")
    }
}

/// Plain one-line text, control characters dropped, whitespace collapsed, capped. None when empty.
pub(crate) fn sanitize_speak_text(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let capped: String = cleaned.chars().take(SPEAK_MAX_CHARS).collect();
    let capped = capped.trim().to_string();
    (!capped.is_empty()).then_some(capped)
}

/// The loop is running AND it is the process that announced the speak entry (patch applied).
pub(crate) fn speak_ready_in(p: &VoicePaths, running_pid: Option<u32>) -> bool {
    let Some(pid) = running_pid else {
        return false;
    };
    hand_off_dir_ok(&p.speak_dir())
        && fs::read_to_string(p.speak_ready_file())
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
            == Some(pid)
}

/// Ok(true) = request handed to the loop. Ok(false) = nothing sent (not ready, local-only is off,
/// empty text, or the previous request is still waiting). Never speaks outside the loop.
pub(crate) fn speak_request_in(
    p: &VoicePaths,
    running_pid: Option<u32>,
    local_only: bool,
    text: &str,
    now_ms: u128,
) -> Result<bool, String> {
    if !local_only || !speak_ready_in(p, running_pid) {
        return Ok(false);
    }
    let Some(text) = sanitize_speak_text(text) else {
        return Ok(false);
    };
    let dir = p.speak_dir();
    let target = dir.join("request.json");
    if request_pending(&target, REQUEST_STALE) {
        return Ok(false);
    }
    let body = serde_json::json!({ "id": format!("{now_ms}"), "text": text, "ts": now_ms as u64 });
    let tmp = dir.join("request.json.tmp");
    crate::app_paths::write_private(&tmp, body.to_string())
        .map_err(|_| "speak failed".to_string())?;
    fs::rename(&tmp, &target).map_err(|_| "speak failed".to_string())?;
    Ok(true)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// True only when a patched Amara Voice loop is running and announced its local speak entry.
#[tauri::command]
#[specta::specta]
pub async fn voice_speak_ready() -> Result<bool, String> {
    blocking(JoinErr::Prefix("voice task failed: "), || {
        Ok(speak_ready_in(
            &voice_paths(),
            running_loop().map(|l| l.pid),
        ))
    })
    .await
}

/// Ask Amara Voice to say one line with her local voice. Needs the navy local-only switch to be on.
/// Ok(false) = nothing sent. Callers only ask when the look was opened by voice.
#[tauri::command]
#[specta::specta]
pub async fn voice_speak_local(text: String) -> Result<bool, String> {
    blocking(JoinErr::Prefix("voice task failed: "), move || {
        speak_request_in(
            &voice_paths(),
            running_loop().map(|l| l.pid),
            crate::telegram::local_only_on(),
            &text,
            now_ms(),
        )
    })
    .await
}

// ---------- local dictation (voice adds and check-ins) ----------
//
// Contract (tasks/proposed/amara-voice-dictate.diff, NOT yet applied to the live loop): a patched
// loop creates <dir>/dictate/ and writes its pid to <dir>/logs/dictate_ready. The app drops ONE
// request file <dir>/dictate/request.json (tmp + rename) = {id, purpose, max_secs, ts}. The loop
// records once from ITS OWN mic stream, transcribes with its local Whisper, writes ONLY text to
// <dir>/dictate/result.json = {id, ok, text} (tmp + rename) and drops the audio from memory. No audio
// is ever written to disk, by either side. The app reads the result, deletes it, returns the text
// (sanitised, capped). File only: no network, no cloud STT, no macOS dictation.
// The text goes to the existing add / check-in preview; nothing here saves anything.

pub(crate) const DICTATE_MAX_CHARS: usize = 500;
const DICTATE_MIN_SECS: u32 = 3;
const DICTATE_MAX_SECS: u32 = 30;
/// Extra wait on top of the recording length: Whisper base.en on a short clip.
const DICTATE_TRANSCRIBE_GRACE: Duration = Duration::from_secs(20);
const DICTATE_POLL: Duration = Duration::from_millis(150);

impl VoicePaths {
    fn dictate_dir(&self) -> PathBuf {
        self.dir.join("dictate")
    }
    fn dictate_ready_file(&self) -> PathBuf {
        self.dir.join("logs").join("dictate_ready")
    }
}

/// Same cleaning as the speak path, with the dictation cap. None when empty.
pub(crate) fn sanitize_dictation_text(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let capped: String = cleaned.chars().take(DICTATE_MAX_CHARS).collect();
    let capped = capped.trim().to_string();
    (!capped.is_empty()).then_some(capped)
}

/// The loop is running AND it is the process that announced the dictation entry (patch applied).
pub(crate) fn dictate_ready_in(p: &VoicePaths, running_pid: Option<u32>) -> bool {
    let Some(pid) = running_pid else {
        return false;
    };
    hand_off_dir_ok(&p.dictate_dir())
        && fs::read_to_string(p.dictate_ready_file())
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
            == Some(pid)
}

pub(crate) fn dictate_purpose_ok(purpose: &str) -> bool {
    matches!(purpose, "add" | "checkin")
}

/// One dictation. Ok(None) = nothing recorded (not ready, unknown purpose, busy,
/// nothing heard, or no answer in time). Ok(Some(text)) = sanitised transcript, result file deleted.
pub(crate) fn dictate_once_in(
    p: &VoicePaths,
    running_pid: Option<u32>,
    purpose: &str,
    max_secs: u32,
    now_ms: u128,
    wait: Duration,
) -> Result<Option<String>, String> {
    if !dictate_purpose_ok(purpose) || !dictate_ready_in(p, running_pid) {
        return Ok(None);
    }
    let dir = p.dictate_dir();
    let request = dir.join("request.json");
    let result = dir.join("result.json");
    if request_pending(&request, REQUEST_STALE) {
        return Ok(None); // a dictation is already waiting for the loop
    }
    let _ = fs::remove_file(&result); // a late answer from an earlier call
    let id = format!("{now_ms}");
    let secs = max_secs.clamp(DICTATE_MIN_SECS, DICTATE_MAX_SECS);
    let body =
        serde_json::json!({ "id": id, "purpose": purpose, "max_secs": secs, "ts": now_ms as u64 });
    let tmp = dir.join("request.json.tmp");
    crate::app_paths::write_private(&tmp, body.to_string())
        .map_err(|_| "dictation failed".to_string())?;
    fs::rename(&tmp, &request).map_err(|_| "dictation failed".to_string())?;
    let deadline = std::time::Instant::now() + wait;
    loop {
        if let Ok(raw) = fs::read_to_string(&result) {
            let _ = fs::remove_file(&result);
            let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
            if v.get("id").and_then(|x| x.as_str()) != Some(id.as_str()) {
                continue; // someone else's answer: dropped, keep waiting for ours
            }
            if v.get("ok").and_then(|x| x.as_bool()) != Some(true) {
                return Ok(None);
            }
            return Ok(v
                .get("text")
                .and_then(|t| t.as_str())
                .and_then(sanitize_dictation_text));
        }
        if std::time::Instant::now() >= deadline {
            let _ = fs::remove_file(&request); // never taken: do not record later
            return Ok(None);
        }
        std::thread::sleep(DICTATE_POLL);
    }
}

/// True only when a patched Amara Voice loop is running and announced its dictation entry.
#[tauri::command]
#[specta::specta]
pub async fn voice_dictate_ready() -> Result<bool, String> {
    blocking(JoinErr::Prefix("voice task failed: "), || {
        Ok(dictate_ready_in(
            &voice_paths(),
            running_loop().map(|l| l.pid),
        ))
    })
    .await
}

/// Record once on this Mac (Amara Voice's mic) and return the transcript as text. `purpose` is
/// "add" or "checkin". Always on-device (no navy gate: nothing leaves the Mac). The caller shows the text in the
/// existing preview; this never saves.
#[tauri::command]
#[specta::specta]
pub async fn dictate_once(
    purpose: String,
    max_secs: Option<u32>,
) -> Result<Option<String>, String> {
    let secs = max_secs
        .unwrap_or(15)
        .clamp(DICTATE_MIN_SECS, DICTATE_MAX_SECS);
    blocking(JoinErr::Prefix("voice task failed: "), move || {
        dictate_once_in(
            &voice_paths(),
            running_loop().map(|l| l.pid),
            &purpose,
            secs,
            now_ms(),
            Duration::from_secs(u64::from(secs)) + DICTATE_TRANSCRIBE_GRACE,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    #[test]
    fn spawned_voice_loop_is_told_where_the_navy_flag_file_lives() {
        let src = include_str!("voice.rs");
        let f = &src[src.find("fn start_loop").unwrap()..];
        assert!(f[..f.find("fn stop_loop").unwrap()]
            .contains(".env(\"NEXUS_DATA_DIR\", crate::app_paths::data_dir())"));
    }

    use super::*;

    fn tmp() -> (PathBuf, VoicePaths) {
        let dir = std::env::temp_dir().join(format!("nexus-voice-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("voices")).unwrap();
        fs::create_dir_all(dir.join("logs")).unwrap();
        let p = VoicePaths {
            dir: dir.clone(),
            python: PathBuf::from("/usr/bin/false"),
        };
        (dir, p)
    }

    #[test]
    fn find_loop_only_matches_python() {
        let ps = "  101  2048 /bin/bash /Users/x/projects/amara-voice/service/run.sh\n\
                  102   900 pgrep -f amara_voice.py\n\
                  103 3145728 /Users/x/projects/ming-mac/.venv/bin/python -u amara_voice.py\n";
        assert_eq!(
            find_loop(ps),
            Some(LoopProc {
                pid: 103,
                rss_kb: 3145728
            })
        );
        assert_eq!(find_loop("  5 10 /usr/bin/vim amara_voice.py\n"), None);
        assert_eq!(
            find_loop("  7 10 /opt/homebrew/Cellar/python/Python /x/amara_voice.py\n")
                .map(|p| p.pid),
            Some(7)
        );
        assert_eq!(find_loop("  8 10 python amara_voice.py.bak\n"), None);
        assert_eq!(find_loop("garbage\n\n"), None);
    }

    #[test]
    fn state_normalized() {
        assert_eq!(normalize_state(Some("speaking\n"), true), "speaking");
        assert_eq!(normalize_state(Some("speaking"), false), "stopped");
        assert_eq!(normalize_state(Some("quit"), true), "stopped");
        assert_eq!(normalize_state(Some("<script>"), true), "loading");
        assert_eq!(normalize_state(None, true), "loading");
    }

    #[test]
    fn voices_need_a_transcript_and_safe_name() {
        let (dir, p) = tmp();
        for f in [
            "neosoul_g.wav",
            "neosoul_g.txt",
            "amara_ref_16k.wav",
            "bad name.wav",
            "bad name.txt",
        ] {
            fs::write(dir.join("voices").join(f), "x").unwrap();
        }
        assert_eq!(list_voices_in(&p), vec!["neosoul_g".to_string()]);
        assert_eq!(current_voice(&p), DEFAULT_VOICE);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn set_voice_persists_and_rejects_unknown() {
        let (dir, p) = tmp();
        for f in ["a1.wav", "a1.txt", "b2.wav", "b2.txt"] {
            fs::write(dir.join("voices").join(f), "x").unwrap();
        }
        fs::write(p.config_file(), r#"{"other": 1}"#).unwrap();
        set_voice_in(&p, "b2").unwrap();
        assert_eq!(current_voice(&p), "b2");
        let cfg: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(p.config_file()).unwrap()).unwrap();
        assert_eq!(cfg["other"], 1); // other keys kept
        assert!(set_voice_in(&p, "zz").is_err());
        assert!(set_voice_in(&p, "../b2").is_err());
        assert_eq!(current_voice(&p), "b2");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tail_turns_reads_last_n_and_skips_junk() {
        let (dir, p) = tmp();
        let mut body = String::new();
        for i in 0..30 {
            body += &format!("{{\"heard\": \"turn {i}\", \"route\": \"hermes\"}}\n");
            if i == 25 {
                body += "not json\n[1,2]\n";
            }
        }
        fs::write(p.turns_file(), body).unwrap();
        let t = tail_turns(&p.turns_file(), 3);
        let heard: Vec<_> = t.iter().map(|v| v["heard"].as_str().unwrap()).collect();
        assert_eq!(heard, vec!["turn 27", "turn 28", "turn 29"]);
        assert_eq!(tail_turns(&p.turns_file(), 10_000).len(), 30);
        assert!(tail_turns(&dir.join("missing.jsonl"), 5).is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn not_installed_status() {
        let p = VoicePaths {
            dir: std::env::temp_dir().join("nexus-voice-definitely-missing"),
            python: PathBuf::from("/nope"),
        };
        let s = status_for(&p);
        assert!(!s.installed && !s.running);
        assert_eq!(s.state, "not_installed");
        assert!(start_loop(&p).is_err());
    }

    struct FakeProcs {
        ps: Option<String>,
        sent: std::cell::RefCell<Vec<u32>>,
    }
    impl ProcOps for FakeProcs {
        fn list_own(&self) -> Option<String> {
            self.ps.clone()
        }
        fn sigusr1(&self, pid: u32) -> Result<(), String> {
            self.sent.borrow_mut().push(pid);
            Ok(())
        }
    }

    fn fake(ps: &str) -> FakeProcs {
        FakeProcs {
            ps: Some(ps.into()),
            sent: Default::default(),
        }
    }

    const PS: &str = "  101 05:00 /bin/bash /Users/x/projects/amara-voice/service/run.sh\n\
                      102 00:01 pgrep -f amara_voice.py\n\
                      103 1-02:03:04 /x/.venv/bin/python -u amara_voice.py\n";

    #[test]
    fn etime_parses() {
        assert_eq!(parse_etime("05:09"), Some(309));
        assert_eq!(parse_etime("02:03:04"), Some(7384));
        assert_eq!(parse_etime("1-02:03:04"), Some(86_400 + 7384));
        assert_eq!(parse_etime("zz"), None);
    }

    #[test]
    fn stop_speaking_signals_only_the_patched_loop() {
        let (dir, p) = tmp();
        let script = p.script();
        fs::write(&script, "# loop\n").unwrap();
        let streaming = dir.join("streaming.py");
        fs::write(&streaming, "x\n").unwrap();
        let files = [script.clone(), streaming.clone()];
        // Not patched: never signalled (SIGUSR1 would kill an unpatched loop).
        let f = fake(PS);
        assert_eq!(stop_speaking_with(&f, &script, &files), Ok(false));
        assert!(f.sent.borrow().is_empty());
        // Patched (file older than the 1-day-old process): exactly pid 103.
        fs::write(&script, "signal.signal(signal.SIGUSR1, h)\n").unwrap();
        let old = SystemTime::now() - Duration::from_secs(10 * 86_400);
        fs::File::options()
            .write(true)
            .open(&script)
            .unwrap()
            .set_modified(old)
            .unwrap();
        fs::File::options()
            .write(true)
            .open(&streaming)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let f = fake(PS);
        assert_eq!(stop_speaking_with(&f, &script, &files), Ok(true));
        assert_eq!(*f.sent.borrow(), vec![103]);
        // Process older than the patch (started before the file changed): no signal.
        let f = fake("103 00:30 /x/python -u amara_voice.py\n");
        fs::File::options()
            .write(true)
            .open(&script)
            .unwrap()
            .set_modified(SystemTime::now())
            .unwrap();
        assert_eq!(stop_speaking_with(&f, &script, &files), Ok(false));
        assert!(f.sent.borrow().is_empty());
    }

    #[test]
    fn stop_speaking_is_a_noop_when_not_running() {
        let (_, p) = tmp();
        let f = fake("  101 05:00 /bin/bash run.sh\n  102 00:01 python other.py\n");
        assert_eq!(stop_speaking_with(&f, &p.script(), &[]), Ok(false));
        let none = FakeProcs {
            ps: None,
            sent: Default::default(),
        };
        assert_eq!(stop_speaking_with(&none, &p.script(), &[]), Ok(false));
        assert!(f.sent.borrow().is_empty());
    }

    fn ready(p: &VoicePaths, pid: u32) {
        fs::create_dir_all(p.dir.join("speak")).unwrap();
        fs::write(p.dir.join("logs").join("speak_ready"), format!("{pid}\n")).unwrap();
    }

    #[test]
    fn speak_is_inert_without_the_patch_or_the_loop() {
        let (dir, p) = tmp();
        // No marker, no speak dir: nothing is sent and nothing is created.
        assert!(!speak_ready_in(&p, Some(7)));
        assert_eq!(speak_request_in(&p, Some(7), true, "hi", 1), Ok(false));
        assert!(!dir.join("speak").exists());
        // Marker but loop not running, or a different pid (stale marker): still inert.
        ready(&p, 7);
        assert_eq!(speak_request_in(&p, None, true, "hi", 1), Ok(false));
        assert_eq!(speak_request_in(&p, Some(8), true, "hi", 1), Ok(false));
        assert!(!dir.join("speak").join("request.json").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn speak_needs_local_only_and_sends_one_sanitised_request() {
        let (dir, p) = tmp();
        ready(&p, 7);
        let req = dir.join("speak").join("request.json");
        assert_eq!(speak_request_in(&p, Some(7), false, "hi", 1), Ok(false));
        assert!(!req.exists());
        let long = format!("Lights\u{7}\n out. {}", "x".repeat(1000));
        assert_eq!(speak_request_in(&p, Some(7), true, &long, 5), Ok(true));
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&req).unwrap()).unwrap();
        let t = v["text"].as_str().unwrap();
        assert!(t.starts_with("Lights out. xxx") && t.chars().count() == SPEAK_MAX_CHARS);
        assert!(!t.chars().any(|c| c.is_control()));
        assert!(!dir.join("speak").join("request.json.tmp").exists());
        // The previous request has not been consumed: no second one is queued.
        assert_eq!(speak_request_in(&p, Some(7), true, "again", 6), Ok(false));
        assert_eq!(speak_request_in(&p, Some(7), true, " \n\t ", 6), Ok(false));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn speak_is_file_only_no_network_no_say() {
        let src = include_str!("voice.rs");
        let body = &src[src.find("// ---------- local speak").unwrap()
            ..src.find("#[cfg(test)]\nmod tests").unwrap()];
        for banned in [
            "http",
            "TcpStream",
            "reqwest",
            "/usr/bin/say",
            "Command::new",
        ] {
            assert!(
                !body.contains(banned),
                "{banned} must not appear in the speak path"
            );
        }
    }

    // ---------- dictation ----------

    fn dready(p: &VoicePaths, pid: u32) {
        fs::create_dir_all(p.dir.join("dictate")).unwrap();
        fs::write(p.dir.join("logs").join("dictate_ready"), format!("{pid}\n")).unwrap();
    }

    /// A stand-in for the patched loop: takes the request, answers with TEXT ONLY.
    fn fake_loop(
        dir: PathBuf,
        answer: serde_json::Value,
        echo_id: bool,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let req = dir.join("dictate").join("request.json");
            for _ in 0..200 {
                if let Ok(raw) = fs::read_to_string(&req) {
                    fs::remove_file(&req).unwrap();
                    let r: serde_json::Value = serde_json::from_str(&raw).unwrap();
                    let mut a = answer.clone();
                    if echo_id {
                        a["id"] = r["id"].clone();
                    }
                    let tmp = dir.join("dictate").join("result.json.tmp");
                    fs::write(&tmp, a.to_string()).unwrap();
                    fs::rename(&tmp, dir.join("dictate").join("result.json")).unwrap();
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    }

    #[test]
    fn dictation_is_inert_without_the_patch_or_the_loop() {
        let (dir, p) = tmp();
        let w = Duration::from_millis(50);
        assert!(!dictate_ready_in(&p, Some(7)));
        assert_eq!(dictate_once_in(&p, Some(7), "add", 5, 1, w), Ok(None));
        assert!(!dir.join("dictate").exists());
        dready(&p, 7);
        assert_eq!(dictate_once_in(&p, None, "add", 5, 1, w), Ok(None));
        assert_eq!(dictate_once_in(&p, Some(8), "add", 5, 1, w), Ok(None)); // stale pid
        assert!(!dir.join("dictate").join("request.json").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dictation_works_without_local_only_but_needs_ready_and_a_known_purpose() {
        let (dir, p) = tmp();
        dready(&p, 7);
        // No navy gate: a ready loop and a known purpose are enough.
        let h = fake_loop(
            dir.clone(),
            serde_json::json!({ "ok": true, "text": "hello" }),
            true,
        );
        assert_eq!(
            dictate_once_in(&p, Some(7), "checkin", 5, 1, Duration::from_secs(5)),
            Ok(Some("hello".to_string()))
        );
        h.join().unwrap();
        let w = Duration::from_millis(50);
        assert_eq!(dictate_once_in(&p, Some(7), "chat", 5, 1, w), Ok(None));
        assert_eq!(dictate_once_in(&p, Some(7), "", 5, 1, w), Ok(None));
        assert_eq!(dictate_once_in(&p, Some(8), "add", 5, 1, w), Ok(None));
        assert!(!dir.join("dictate").join("request.json").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dictation_returns_sanitised_capped_text_and_leaves_no_files_behind() {
        let (dir, p) = tmp();
        dready(&p, 7);
        let long = format!("Add\u{7} a goal\n to run. {}", "y".repeat(2000));
        let h = fake_loop(
            dir.clone(),
            serde_json::json!({ "ok": true, "text": long }),
            true,
        );
        let got = dictate_once_in(&p, Some(7), "add", 99, 5, Duration::from_secs(5)).unwrap();
        h.join().unwrap();
        let t = got.unwrap();
        assert!(t.starts_with("Add a goal to run. yyy") && t.chars().count() == DICTATE_MAX_CHARS);
        assert!(!t.chars().any(|c| c.is_control()));
        let mut left: Vec<String> = fs::read_dir(dir.join("dictate"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert!(
            left.is_empty(),
            "request/result must be deleted, found {left:?}"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dictation_request_is_clamped_and_carries_only_the_purpose() {
        let (dir, p) = tmp();
        dready(&p, 7);
        let d2 = dir.clone();
        let spy = std::thread::spawn(move || {
            let req = d2.join("dictate").join("request.json");
            for _ in 0..200 {
                if let Ok(raw) = fs::read_to_string(&req) {
                    return raw;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            String::new()
        });
        let _ = dictate_once_in(&p, Some(7), "checkin", 999, 5, Duration::from_millis(400));
        let v: serde_json::Value = serde_json::from_str(&spy.join().unwrap()).unwrap();
        assert_eq!(v["purpose"], "checkin");
        assert_eq!(v["max_secs"], u64::from(DICTATE_MAX_SECS));
        assert_eq!(v.as_object().unwrap().len(), 4);
        // Nobody took it: the request is withdrawn, so the loop never records late.
        assert!(!dir.join("dictate").join("request.json").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dictation_ignores_other_ids_failures_and_empty_text() {
        let (dir, p) = tmp();
        dready(&p, 7);
        let w = Duration::from_millis(400);
        // Answer for a different id is dropped.
        let h = fake_loop(
            dir.clone(),
            serde_json::json!({ "id": "other", "ok": true, "text": "hi" }),
            false,
        );
        assert_eq!(dictate_once_in(&p, Some(7), "add", 5, 6, w), Ok(None));
        h.join().unwrap();
        let h = fake_loop(
            dir.clone(),
            serde_json::json!({ "ok": false, "text": "" }),
            true,
        );
        assert_eq!(
            dictate_once_in(&p, Some(7), "add", 5, 7, Duration::from_secs(5)),
            Ok(None)
        );
        h.join().unwrap();
        let h = fake_loop(
            dir.clone(),
            serde_json::json!({ "ok": true, "text": " \n\t " }),
            true,
        );
        assert_eq!(
            dictate_once_in(&p, Some(7), "add", 5, 8, Duration::from_secs(5)),
            Ok(None)
        );
        h.join().unwrap();
        // A request already waiting means busy: no second one.
        fs::write(dir.join("dictate").join("request.json"), "{}").unwrap();
        assert_eq!(dictate_once_in(&p, Some(7), "add", 5, 9, w), Ok(None));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn dictation_never_writes_audio_and_stays_local() {
        let src = include_str!("voice.rs");
        let body = &src[src.find("// ---------- local dictation").unwrap()
            ..src.find("#[cfg(test)]\nmod tests").unwrap()];
        for banned in [
            "http",
            "TcpStream",
            "reqwest",
            "/usr/bin/say",
            "Command::new",
            ".wav",
            ".mp3",
            ".m4a",
            "hound",
            "File::create",
            "OpenOptions",
            "cloud",
        ] {
            // The contract comment names "no cloud STT"; only code lines are checked.
            let hit = body
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .any(|l| l.contains(banned));
            assert!(!hit, "{banned} must not appear in the dictation path");
        }
        // The only file the app writes is the JSON request.
        assert_eq!(body.matches("write_private(").count(), 1);
        // The patched loop: text-only result, audio dropped, no audio file API.
        let diff = include_str!("../../../tasks/proposed/amara-voice-dictate.diff");
        let added: Vec<&str> = diff
            .lines()
            .filter(|l: &&str| l.starts_with('+') && !l.starts_with("+++"))
            .collect();
        let added = added.join("\n");
        for banned in [
            "wave.open",
            "sf.write",
            "soundfile",
            ".wav",
            ".tofile",
            "np.save",
            "np.savez",
            "write_bytes",
            "tempfile",
            "requests.",
            "urllib",
            "socket",
            "openai",
            "WebSocket",
        ] {
            assert!(
                !added.contains(banned),
                "{banned} must not appear in the dictate patch"
            );
        }
        assert!(
            added.contains("del audio"),
            "the patch must drop the audio buffer"
        );
        assert!(added.contains("DICTATE_READY"));
    }

    #[test]
    fn hand_off_dir_must_be_a_real_private_dir_owned_by_us() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, p) = tmp();
        let uid = unsafe { libc::getuid() };
        let d = dir.join("speak");
        fs::create_dir_all(&d).unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(hand_off_dir_ok_for(&d, uid));
        assert!(!hand_off_dir_ok_for(&d, uid.wrapping_add(1))); // someone else's
        fs::set_permissions(&d, fs::Permissions::from_mode(0o770)).unwrap();
        assert!(!hand_off_dir_ok_for(&d, uid)); // group write
        fs::set_permissions(&d, fs::Permissions::from_mode(0o702)).unwrap();
        assert!(!hand_off_dir_ok_for(&d, uid)); // other write
                                                // A symlink to a good dir is refused, for both hand-offs.
        let real = dir.join("real");
        fs::create_dir_all(&real).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["speak", "dictate"] {
            let link = dir.join(name);
            let _ = fs::remove_dir_all(&link);
            std::os::unix::fs::symlink(&real, &link).unwrap();
        }
        fs::write(dir.join("logs").join("speak_ready"), "7\n").unwrap();
        fs::write(dir.join("logs").join("dictate_ready"), "7\n").unwrap();
        assert!(!speak_ready_in(&p, Some(7)));
        assert!(!dictate_ready_in(&p, Some(7)));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stale_request_is_cleared_so_it_cannot_block_forever() {
        let (dir, p) = tmp();
        ready(&p, 7);
        dready(&p, 7);
        let old = SystemTime::now() - Duration::from_secs(60);
        for (sub, f) in [("speak", "request.json"), ("dictate", "request.json")] {
            let path = dir.join(sub).join(f);
            fs::write(&path, "{}").unwrap();
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        assert_eq!(speak_request_in(&p, Some(7), true, "hi", 5), Ok(true));
        let v: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(dir.join("speak").join("request.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(v["text"], "hi"); // the old one is gone, ours is there
                                     // The stale dictation request is cleared by the check itself (before a stand-in loop starts).
        let dreq = dir.join("dictate").join("request.json");
        assert!(!request_pending(&dreq, REQUEST_STALE));
        assert!(!dreq.exists());
        let h = fake_loop(
            dir.clone(),
            serde_json::json!({ "ok": true, "text": "yes" }),
            true,
        );
        assert_eq!(
            dictate_once_in(&p, Some(7), "add", 5, 6, Duration::from_secs(5)),
            Ok(Some("yes".to_string()))
        );
        h.join().unwrap();
        // A fresh one still blocks.
        assert_eq!(speak_request_in(&p, Some(7), true, "again", 7), Ok(false));
        let _ = fs::remove_dir_all(dir);
    }
}
