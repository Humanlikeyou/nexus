//! `system_profile` (first-run tier picking): RAM, chip, CPU cores, architecture, macOS version and
//! FileVault state. Read-only. No serial number, hardware UUID, hostname, user name or network
//! identifier is read or returned. Runs fixed system tools by absolute path; nothing from IPC.

use serde::Serialize;
use std::time::Duration;

const SYSCTL: &str = "/usr/sbin/sysctl";
const FDESETUP: &str = "/usr/bin/fdesetup";
const TOOL_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_TEXT_CHARS: usize = 64;
const UNAVAILABLE: &str = "profile_unavailable";

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SystemProfile {
    /// Installed memory in GB (rounded).
    pub ram_gb: u32,
    /// CPU brand string, e.g. "Apple M3 Pro".
    pub chip: String,
    /// `std::env::consts::ARCH` (aarch64 / x86_64).
    pub arch: String,
    pub cores: u32,
    /// `kern.osproductversion`, e.g. "15.1". None when it can't be read.
    pub macos: Option<String>,
    /// None when the state can't be read (never guessed).
    pub file_vault: Option<bool>,
}

/// `hw.memsize` bytes to whole GB (rounded to nearest). None for 0 or non-numbers.
pub(crate) fn parse_ram_gb(out: &str) -> Option<u32> {
    let bytes: u64 = out.trim().parse().ok()?;
    if bytes == 0 {
        return None;
    }
    let gib = (bytes as f64 / (1u64 << 30) as f64).round();
    u32::try_from(gib as u64).ok().filter(|g| *g > 0)
}

pub(crate) fn parse_cores(out: &str) -> Option<u32> {
    out.trim()
        .parse::<u32>()
        .ok()
        .filter(|c| *c > 0 && *c <= 4096)
}

/// `fdesetup status` text: "FileVault is On." / "FileVault is Off.".
pub(crate) fn parse_filevault(out: &str) -> Option<bool> {
    let t = out.trim();
    if t.starts_with("FileVault is On") {
        Some(true)
    } else if t.starts_with("FileVault is Off") {
        Some(false)
    } else {
        None
    }
}

/// First line, printable ASCII/Unicode only, capped (char-boundary safe). None if empty.
pub(crate) fn clean_text(out: &str) -> Option<String> {
    let line = out.lines().next()?.trim();
    let s: String = line
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT_CHARS)
        .collect();
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

async fn run(bin: &str, args: &[&str]) -> Option<String> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(TOOL_TIMEOUT, cmd.output())
        .await
        .ok()?
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn sysctl(name: &str) -> Option<String> {
    run(SYSCTL, &["-n", name]).await
}

#[tauri::command]
#[specta::specta]
pub async fn system_profile() -> Result<SystemProfile, String> {
    let ram_gb = sysctl("hw.memsize").await.as_deref().and_then(parse_ram_gb);
    let chip = sysctl("machdep.cpu.brand_string")
        .await
        .as_deref()
        .and_then(clean_text);
    let (Some(ram_gb), Some(chip)) = (ram_gb, chip) else {
        return Err(UNAVAILABLE.to_string());
    };
    Ok(SystemProfile {
        ram_gb,
        chip,
        arch: std::env::consts::ARCH.to_string(),
        cores: sysctl("hw.ncpu")
            .await
            .as_deref()
            .and_then(parse_cores)
            .unwrap_or(0),
        macos: sysctl("kern.osproductversion")
            .await
            .as_deref()
            .and_then(clean_text),
        file_vault: run(FDESETUP, &["status"])
            .await
            .as_deref()
            .and_then(parse_filevault),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers_are_strict() {
        assert_eq!(parse_ram_gb("38654705664\n"), Some(36));
        assert_eq!(parse_ram_gb("17179869184"), Some(16));
        assert_eq!(parse_ram_gb("0"), None);
        assert_eq!(parse_ram_gb("lots"), None);
        assert_eq!(parse_ram_gb(""), None);
        assert_eq!(parse_cores("12\n"), Some(12));
        assert_eq!(parse_cores("0"), None);
        assert_eq!(parse_cores("x"), None);
        assert_eq!(parse_filevault("FileVault is On.\n"), Some(true));
        assert_eq!(parse_filevault("FileVault is Off."), Some(false));
        assert_eq!(parse_filevault("Error: not permitted"), None);
        assert_eq!(clean_text("Apple M3 Pro\n"), Some("Apple M3 Pro".into()));
        assert_eq!(clean_text("\n"), None);
        assert_eq!(clean_text(&"é".repeat(200)).unwrap().chars().count(), 64);
    }

    #[test]
    fn shape_has_no_identifiers() {
        let p = SystemProfile {
            ram_gb: 36,
            chip: "Apple M3 Pro".into(),
            arch: "aarch64".into(),
            cores: 12,
            macos: Some("15.1".into()),
            file_vault: Some(true),
        };
        let v = serde_json::to_value(&p).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["arch", "chip", "cores", "fileVault", "macos", "ramGb"]
        );
    }

    #[test]
    fn source_reads_no_identifiers_and_takes_no_ipc_input() {
        let code = include_str!("system_profile.rs");
        let code = code.split("#[cfg(test)]").next().unwrap().to_lowercase();
        for banned in [
            "hw.serialnumber",
            "serialnumber",
            "ioplatformserial",
            "platformuuid",
            "hw.uuid",
            "hostname",
            "ioreg",
            "system_profiler",
            "whoami",
        ] {
            // The doc comment names them as exclusions; the code must not call them.
            let hits = code
                .lines()
                .filter(|l| !l.trim_start().starts_with("//") && l.contains(banned))
                .count();
            assert_eq!(hits, 0, "{banned}");
        }
        assert!(code.contains("pub async fn system_profile()"));
    }

    #[tokio::test]
    async fn live_call_has_the_right_shape_on_this_machine() {
        // Real read-only sysctl calls (macOS only app).
        let p = system_profile().await.unwrap();
        assert!(p.ram_gb >= 4 && !p.chip.is_empty() && p.cores > 0);
    }
}
