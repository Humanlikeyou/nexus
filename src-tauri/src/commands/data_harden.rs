//! Data-dir permission hardening (Phase 7 step 6). [GATE: Taisun "not yet"]
//!
//! Built, default-off, never called automatically, no UI. Two commands:
//! * `data_dir_harden_status`: dry run, counts only (no paths).
//! * `data_dir_harden`: applies 0700/0600, and only after a native approval dialog. The apply
//!   core takes an `ApprovalToken` that only the approval path can mint, so nothing else can
//!   reach the chmod. Also refuses while `DATA_HARDEN_ENABLED` is false (the gate).
//!
//! Walk rules: never follows symlinks (lstat; symlinks are counted and skipped), skips any
//! directory named `replays`, `sessions` or `muse` entirely at any depth (Muse recordings are
//! sacred, hard rule 9), skips entries not owned by the current user, never touches content.

use serde::Serialize;
use std::fs;
use std::path::Path;

/// The gate. Flip only with Taisun's words logged in docs/decisions/.
pub const DATA_HARDEN_ENABLED: bool = false;
const SKIP_DIRS: [&str; 3] = ["replays", "sessions", "muse"];
pub const DISABLED: &str = "disabled: data-dir hardening is not enabled yet.";

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HardenPlan {
    pub dirs_to_change: u32,
    pub files_to_change: u32,
    pub already_private: u32,
    pub skipped_protected_dirs: u32,
    pub skipped_symlinks: u32,
    pub skipped_not_owned: u32,
    pub errors: u32,
}

/// Proof that the native dialog returned Allow. Constructed only in this module.
pub struct ApprovalToken(());

#[cfg(unix)]
fn owned_by_us(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions and cannot fail.
    meta.uid() == unsafe { libc::geteuid() }
}
#[cfg(not(unix))]
fn owned_by_us(_: &fs::Metadata) -> bool {
    true
}

#[cfg(unix)]
fn mode_of(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o777
}
#[cfg(not(unix))]
fn mode_of(_: &fs::Metadata) -> u32 {
    0
}

fn walk(dir: &Path, apply: bool, plan: &mut HardenPlan) {
    let Ok(rd) = fs::read_dir(dir) else {
        plan.errors += 1;
        return;
    };
    for entry in rd {
        let Ok(entry) = entry else {
            plan.errors += 1;
            continue;
        };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            plan.errors += 1;
            continue;
        };
        let ft = meta.file_type();
        if ft.is_symlink() {
            plan.skipped_symlinks += 1;
            continue;
        }
        let is_dir = ft.is_dir();
        if !is_dir && !ft.is_file() {
            continue;
        }
        if is_dir
            && entry
                .file_name()
                .to_str()
                .is_some_and(|n| SKIP_DIRS.contains(&n))
        {
            plan.skipped_protected_dirs += 1;
            continue;
        }
        if !owned_by_us(&meta) {
            plan.skipped_not_owned += 1;
            continue;
        }
        let target = if is_dir { 0o700 } else { 0o600 };
        if mode_of(&meta) == target {
            plan.already_private += 1;
        } else {
            if is_dir {
                plan.dirs_to_change += 1;
            } else {
                plan.files_to_change += 1;
            }
            if apply && !chmod_no_follow(&path, target) {
                plan.errors += 1;
            }
        }
        if is_dir {
            walk(&path, apply, plan);
        }
    }
}

/// chmod on an opened handle: the open itself refuses a symlink (`O_NOFOLLOW`) so there is no
/// window between a check and the change. Directories also need `O_DIRECTORY`; `O_NONBLOCK`
/// keeps a swapped-in FIFO from hanging the open. Anything that cannot be opened is skipped.
#[cfg(unix)]
fn chmod_no_follow(path: &Path, mode: u32) -> bool {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let want_dir = mode & 0o100 != 0;
    let mut flags = libc::O_NOFOLLOW | libc::O_NONBLOCK;
    if want_dir {
        flags |= libc::O_DIRECTORY;
    }
    match fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)
    {
        Ok(f) => f.set_permissions(fs::Permissions::from_mode(mode)).is_ok(),
        Err(_) => false,
    }
}
#[cfg(not(unix))]
fn chmod_no_follow(_: &Path, _: u32) -> bool {
    true
}

/// Dry run over `root`. Root itself is counted too (dir to 0700) when owned by us.
pub fn plan_for(root: &Path) -> Result<HardenPlan, String> {
    run(root, false)
}

/// Apply over `root`. Needs an `ApprovalToken`.
pub fn apply_for(root: &Path, _token: &ApprovalToken) -> Result<HardenPlan, String> {
    run(root, true)
}

fn run(root: &Path, apply: bool) -> Result<HardenPlan, String> {
    let meta =
        fs::symlink_metadata(root).map_err(|_| "Data directory is unavailable.".to_string())?;
    if !meta.is_dir() {
        return Err("Data directory is unavailable.".to_string());
    }
    let mut plan = HardenPlan::default();
    if owned_by_us(&meta) {
        if mode_of(&meta) == 0o700 {
            plan.already_private += 1;
        } else {
            plan.dirs_to_change += 1;
            if apply && !chmod_no_follow(root, 0o700) {
                plan.errors += 1;
            }
        }
    } else {
        plan.skipped_not_owned += 1;
        return Ok(plan);
    }
    walk(root, apply, &mut plan);
    Ok(plan)
}

fn dialog_text(plan: &HardenPlan) -> String {
    format!(
        "Make the Amara OS data folder owner-only: {} folders to 0700 and {} files to 0600. \
         Muse recordings, replays and sessions are left exactly as they are. Nothing is deleted or moved.",
        plan.dirs_to_change, plan.files_to_change
    )
}

#[tauri::command]
#[specta::specta]
pub async fn data_dir_harden_status() -> Result<HardenPlan, String> {
    let root = crate::app_paths::data_dir();
    tauri::async_runtime::spawn_blocking(move || plan_for(&root))
        .await
        .map_err(|e| format!("Hardening plan failed: {e}"))?
}

#[tauri::command]
#[specta::specta]
pub async fn data_dir_harden(app: tauri::AppHandle) -> Result<HardenPlan, String> {
    if !DATA_HARDEN_ENABLED {
        return Err(DISABLED.to_string());
    }
    let root = crate::app_paths::data_dir();
    let r2 = root.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || plan_for(&r2))
        .await
        .map_err(|e| format!("Hardening plan failed: {e}"))??;
    let text =
        super::approval_gate::approval_text("Harden data folder permissions", &dialog_text(&plan))?;
    let _guard =
        super::approval_gate::require_approval(&app, &["data_dir_harden".to_string()], text)
            .await?;
    let token = ApprovalToken(());
    tauri::async_runtime::spawn_blocking(move || apply_for(&root, &token))
        .await
        .map_err(|e| format!("Hardening failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);
    fn tmp() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "nexus-harden-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[cfg(unix)]
    fn set(p: &Path, m: u32) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
    }
    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        mode_of(&fs::symlink_metadata(p).unwrap())
    }

    #[cfg(unix)]
    fn fixture() -> std::path::PathBuf {
        let root = tmp();
        set(&root, 0o755);
        fs::create_dir_all(root.join("kv")).unwrap();
        set(&root.join("kv"), 0o755);
        fs::write(root.join("kv/a.json"), "x").unwrap();
        set(&root.join("kv/a.json"), 0o644);
        fs::write(root.join("ok.db"), "x").unwrap();
        set(&root.join("ok.db"), 0o600);
        for d in ["replays", "sessions", "muse"] {
            fs::create_dir_all(root.join(d)).unwrap();
            set(&root.join(d), 0o755);
            fs::write(root.join(d).join("eeg_x.csv"), "x").unwrap();
            set(&root.join(d).join("eeg_x.csv"), 0o644);
        }
        // a protected name nested deeper is skipped too
        fs::create_dir_all(root.join("kv/muse")).unwrap();
        set(&root.join("kv/muse"), 0o755);
        root
    }

    #[cfg(unix)]
    #[test]
    fn plan_counts_are_correct_and_dry_run_changes_nothing() {
        let root = fixture();
        let plan = plan_for(&root).unwrap();
        assert_eq!(plan.dirs_to_change, 2, "root + kv: {plan:?}");
        assert_eq!(plan.files_to_change, 1, "{plan:?}");
        assert_eq!(plan.already_private, 1, "{plan:?}");
        assert_eq!(plan.skipped_protected_dirs, 4, "{plan:?}");
        assert_eq!(mode(&root), 0o755);
        assert_eq!(mode(&root.join("kv/a.json")), 0o644);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn apply_hardens_and_leaves_protected_dirs_untouched() {
        let root = fixture();
        let plan = apply_for(&root, &ApprovalToken(())).unwrap();
        assert_eq!(plan.errors, 0);
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(&root.join("kv")), 0o700);
        assert_eq!(mode(&root.join("kv/a.json")), 0o600);
        for d in ["replays", "sessions", "muse"] {
            assert_eq!(mode(&root.join(d)), 0o755, "{d}");
            assert_eq!(mode(&root.join(d).join("eeg_x.csv")), 0o644, "{d}");
        }
        assert_eq!(mode(&root.join("kv/muse")), 0o755);
        assert_eq!(
            apply_for(&root, &ApprovalToken(())).unwrap().dirs_to_change,
            0,
            "idempotent"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let root = tmp();
        let outside = tmp();
        fs::write(outside.join("secret.txt"), "x").unwrap();
        set(&outside.join("secret.txt"), 0o644);
        set(&outside, 0o755);
        std::os::unix::fs::symlink(&outside, root.join("dirlink")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("filelink")).unwrap();
        let plan = apply_for(&root, &ApprovalToken(())).unwrap();
        assert_eq!(plan.skipped_symlinks, 2, "{plan:?}");
        assert_eq!(mode(&outside), 0o755);
        assert_eq!(mode(&outside.join("secret.txt")), 0o644);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn chmod_refuses_a_symlink_swapped_in_after_the_walk() {
        let outside = tmp();
        fs::write(outside.join("t.txt"), "x").unwrap();
        set(&outside.join("t.txt"), 0o644);
        let root = tmp();
        let link = root.join("swapped");
        std::os::unix::fs::symlink(outside.join("t.txt"), &link).unwrap();
        assert!(!chmod_no_follow(&link, 0o600));
        assert_eq!(mode(&outside.join("t.txt")), 0o644);
        let dlink = root.join("dswapped");
        std::os::unix::fs::symlink(&outside, &dlink).unwrap();
        set(&outside, 0o755);
        assert!(!chmod_no_follow(&dlink, 0o700));
        assert_eq!(mode(&outside), 0o755);
        // Real file and dir still work.
        let f = root.join("f");
        fs::write(&f, "x").unwrap();
        set(&f, 0o644);
        assert!(chmod_no_follow(&f, 0o600));
        assert_eq!(mode(&f), 0o600);
        assert!(chmod_no_follow(&root, 0o700));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn apply_is_unreachable_without_the_approval_gate() {
        // Gate is closed by default.
        const { assert!(!DATA_HARDEN_ENABLED) };
        let src = include_str!("data_harden.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        // The token is minted exactly once, after require_approval, and apply_for is only
        // called with it.
        assert_eq!(
            body.matches("ApprovalToken(())").count(),
            2,
            "def + one mint"
        );
        let mint = body.find("let token = ApprovalToken(())").unwrap();
        let approval = body.find("require_approval(").unwrap();
        assert!(approval < mint, "token minted before approval");
        let disabled = body.find("DATA_HARDEN_ENABLED {").unwrap();
        assert!(disabled < approval);
        assert_eq!(body.matches("apply_for(").count(), 2, "fn def + one call");
    }

    #[test]
    fn missing_root_is_an_error() {
        assert!(plan_for(&std::env::temp_dir().join("nexus-harden-absent-xyz")).is_err());
    }
}
