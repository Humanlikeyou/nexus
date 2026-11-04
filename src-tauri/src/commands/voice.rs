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

#[derive(Debug, Serialize, Clone)]
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

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("voice task failed: {e}"))?
}

#[tauri::command]
pub async fn voice_status() -> Result<VoiceStatus, String> {
    blocking(|| Ok(status_for(&voice_paths()))).await
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

#[tauri::command]
pub async fn voice_start() -> Result<VoiceStatus, String> {
    blocking(|| {
        let p = voice_paths();
        start_loop(&p)?;
        std::thread::sleep(Duration::from_millis(600));
        Ok(status_for(&p))
    })
    .await
}

#[tauri::command]
pub async fn voice_stop() -> Result<VoiceStatus, String> {
    blocking(|| {
        let p = voice_paths();
        stop_loop(&p)?;
        Ok(status_for(&p))
    })
    .await
}

#[tauri::command]
pub async fn voice_recent_turns(n: Option<usize>) -> Result<Vec<serde_json::Value>, String> {
    blocking(move || Ok(tail_turns(&voice_paths().turns_file(), n.unwrap_or(20)))).await
}

#[derive(Debug, Serialize, Clone)]
pub struct VoiceList {
    pub voices: Vec<String>,
    pub current: String,
}

#[tauri::command]
pub async fn voice_list_voices() -> Result<VoiceList, String> {
    blocking(|| {
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
pub async fn voice_set_voice(name: String) -> Result<VoiceList, String> {
    blocking(move || {
        let p = voice_paths();
        set_voice_in(&p, &name)?;
        Ok(VoiceList {
            voices: list_voices_in(&p),
            current: current_voice(&p),
        })
    })
    .await
}

#[cfg(test)]
mod tests {
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
}
