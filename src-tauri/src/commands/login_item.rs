//! Open at login (handover Phase 13). OFF by default, and only the user can turn it on.
//!
//! Mechanism: `SMAppService.mainApp` (macOS 13+). It registers the app as a modern login item
//! under System Settings > General > Login Items, and writes NO LaunchAgent plist. The previous
//! `tauri_plugin_autostart` (MacosLauncher::LaunchAgent) was removed: neither it (2.5.1) nor its
//! `auto-launch` 0.5.0 backend has any SMAppService code, so it could only write a plist.
//!
//! Hard guarantees (tests below):
//! - nothing is registered at app start: no function here is called from `setup`;
//! - `login_item_status` only reads;
//! - `login_item_enable` / `login_item_disable` act only when the user invokes them;
//! - the user's last explicit choice persists in `<data_dir>/login_item.json` (0600, atomic
//!   write); a missing or malformed file reads as "not wanted".
//!
//! `RequiresApproval` is a normal outcome: macOS may ask the user to allow the item in System
//! Settings. The status then reports `enabled: false, needs_approval: true`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(crate) const FILE_NAME: &str = "login_item.json";

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BackendStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
    /// Not macOS, or macOS older than 13.
    Unsupported,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct LoginItemStatus {
    /// The system says the login item is registered and allowed.
    pub enabled: bool,
    /// The user's last explicit choice (default false).
    pub wanted: bool,
    pub backend: BackendStatus,
    pub needs_approval: bool,
    /// Always `smappservice`; never a LaunchAgent.
    pub mechanism: String,
    pub error: Option<String>,
}

pub(crate) trait LoginItemBackend {
    fn status(&self) -> BackendStatus;
    fn register(&self) -> Result<(), String>;
    fn unregister(&self) -> Result<(), String>;
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    #[serde(default)]
    wanted: bool,
}

fn path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

pub(crate) fn wanted_in(dir: &Path) -> bool {
    std::fs::read(path(dir))
        .ok()
        .and_then(|b| serde_json::from_slice::<Saved>(&b).ok())
        .map(|s| s.wanted)
        .unwrap_or(false)
}

fn store_wanted(dir: &Path, wanted: bool) -> Result<(), String> {
    let tmp = dir.join(format!("{FILE_NAME}.tmp"));
    let body = serde_json::to_vec(&Saved { wanted }).map_err(|e| e.to_string())?;
    crate::app_paths::write_private(&tmp, body).map_err(|e| format!("Could not save: {e}"))?;
    std::fs::rename(&tmp, path(dir)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not save: {e}")
    })
}

fn build(dir: &Path, b: &dyn LoginItemBackend, error: Option<String>) -> LoginItemStatus {
    let backend = b.status();
    LoginItemStatus {
        enabled: backend == BackendStatus::Enabled,
        wanted: wanted_in(dir),
        backend,
        needs_approval: backend == BackendStatus::RequiresApproval,
        mechanism: "smappservice".into(),
        error,
    }
}

/// Read-only.
pub(crate) fn status_in(dir: &Path, b: &dyn LoginItemBackend) -> LoginItemStatus {
    build(dir, b, None)
}

/// Fixed error codes (no input echoed).
pub(crate) const ERR_LOCATION: &str = "refused: not_in_applications";

/// The running bundle must live in /Applications or ~/Applications. A dev build, a DMG mount or a
/// Downloads copy would register a path that can move or vanish.
pub(crate) fn bundle_location_ok(bundle: Option<&Path>, home: Option<&Path>) -> bool {
    let Some(b) = bundle else { return false };
    if b.extension().and_then(|e| e.to_str()) != Some("app") {
        return false;
    }
    let Some(parent) = b.parent() else {
        return false;
    };
    parent == Path::new("/Applications") || home.is_some_and(|h| parent == h.join("Applications"))
}

/// The `.app` that contains `exe` (Contents/MacOS/<exe>), if any.
pub(crate) fn bundle_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("app"))
        .map(Path::to_path_buf)
}

/// Step 1 of enabling: location check (no dialog if refused), then the native confirm. Touches no
/// system state.
pub(crate) async fn authorize<F, Fut>(
    bundle: Option<&Path>,
    home: Option<&Path>,
    confirm: F,
) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    if !bundle_location_ok(bundle, home) {
        return Err(ERR_LOCATION.into());
    }
    confirm().await
}

/// Step 2, only after `authorize` succeeded: register. The choice is saved only after the system
/// accepts it.
pub(crate) fn register_in(dir: &Path, b: &dyn LoginItemBackend) -> Result<LoginItemStatus, String> {
    if b.status() == BackendStatus::Unsupported {
        return Err("Open at login needs macOS 13 or newer".into());
    }
    b.register()?;
    store_wanted(dir, true)?;
    Ok(build(dir, b, None))
}

/// Unregister (user action). Always records "not wanted"; a system error is returned after.
pub(crate) fn disable_in(dir: &Path, b: &dyn LoginItemBackend) -> Result<LoginItemStatus, String> {
    store_wanted(dir, false)?;
    if matches!(
        b.status(),
        BackendStatus::Enabled | BackendStatus::RequiresApproval
    ) {
        b.unregister()?;
    }
    Ok(build(dir, b, None))
}

#[cfg(target_os = "macos")]
struct SmAppService;

#[cfg(target_os = "macos")]
impl LoginItemBackend for SmAppService {
    fn status(&self) -> BackendStatus {
        use objc2_service_management::{SMAppService, SMAppServiceStatus};
        // SAFETY: plain Objective-C calls on the main-app service object; no pointers escape.
        let s = unsafe { SMAppService::mainAppService().status() };
        match s {
            SMAppServiceStatus::Enabled => BackendStatus::Enabled,
            SMAppServiceStatus::RequiresApproval => BackendStatus::RequiresApproval,
            SMAppServiceStatus::NotFound => BackendStatus::NotFound,
            _ => BackendStatus::NotRegistered,
        }
    }
    fn register(&self) -> Result<(), String> {
        use objc2_service_management::SMAppService;
        // SAFETY: as above.
        unsafe { SMAppService::mainAppService().registerAndReturnError() }.map_err(|e| {
            format!(
                "Could not enable open at login: {}",
                e.localizedDescription()
            )
        })
    }
    fn unregister(&self) -> Result<(), String> {
        use objc2_service_management::SMAppService;
        // SAFETY: as above.
        unsafe { SMAppService::mainAppService().unregisterAndReturnError() }.map_err(|e| {
            format!(
                "Could not disable open at login: {}",
                e.localizedDescription()
            )
        })
    }
}

#[cfg(not(target_os = "macos"))]
struct SmAppService;

#[cfg(not(target_os = "macos"))]
impl LoginItemBackend for SmAppService {
    fn status(&self) -> BackendStatus {
        BackendStatus::Unsupported
    }
    fn register(&self) -> Result<(), String> {
        Err("Open at login needs macOS".into())
    }
    fn unregister(&self) -> Result<(), String> {
        Err("Open at login needs macOS".into())
    }
}

/// Read-only. Safe to call at any time.
#[tauri::command]
#[specta::specta]
pub async fn login_item_status() -> Result<LoginItemStatus, String> {
    let dir = crate::app_paths::data_dir();
    tokio::task::spawn_blocking(move || status_in(&dir, &SmAppService))
        .await
        .map_err(|e| e.to_string())
}

/// Registers Amara OS as a login item. Called only by the user's toggle.
#[tauri::command]
#[specta::specta]
pub async fn login_item_enable(app: tauri::AppHandle) -> Result<LoginItemStatus, String> {
    use super::approval_gate;
    let dir = crate::app_paths::data_dir();
    let bundle = std::env::current_exe().ok().and_then(|e| bundle_of(&e));
    let home = dirs::home_dir();
    let confirm = || async {
        let text = approval_gate::approval_text(
            "Open Amara OS at login",
            "This adds Amara OS to System Settings > General > Login Items (no LaunchAgent file). \
             macOS may ask you to allow it. You can turn it off here or in System Settings.",
        )?;
        approval_gate::require_approval(&app, &[approval_gate::service_key("login-item")], text)
            .await
            .map(|_guard| ())
    };
    authorize(bundle.as_deref(), home.as_deref(), confirm).await?;
    tokio::task::spawn_blocking(move || register_in(&dir, &SmAppService))
        .await
        .map_err(|e| e.to_string())?
}

/// Removes the login item. Called only by the user's toggle.
#[tauri::command]
#[specta::specta]
pub async fn login_item_disable() -> Result<LoginItemStatus, String> {
    let dir = crate::app_paths::data_dir();
    tokio::task::spawn_blocking(move || disable_in(&dir, &SmAppService))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct Fake {
        state: Cell<BackendStatus>,
        calls: RefCell<Vec<&'static str>>,
        fail_register: bool,
        after_register: BackendStatus,
    }
    impl Fake {
        fn new() -> Self {
            Fake {
                state: Cell::new(BackendStatus::NotRegistered),
                calls: RefCell::new(vec![]),
                fail_register: false,
                after_register: BackendStatus::Enabled,
            }
        }
    }
    impl LoginItemBackend for Fake {
        fn status(&self) -> BackendStatus {
            self.state.get()
        }
        fn register(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("register");
            if self.fail_register {
                return Err("denied".into());
            }
            self.state.set(self.after_register);
            Ok(())
        }
        fn unregister(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("unregister");
            self.state.set(BackendStatus::NotRegistered);
            Ok(())
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "nexus-login-{tag}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn default_is_off_and_status_never_registers() {
        let (d, f) = (tmp("default"), Fake::new());
        let s = status_in(&d, &f);
        assert!(!s.enabled && !s.wanted);
        assert_eq!(s.mechanism, "smappservice");
        assert!(f.calls.borrow().is_empty());
        assert!(!path(&d).exists(), "reading must not create the file");
    }

    const APP: &str = "/Applications/Amara OS.app";
    async fn yes() -> Result<(), String> {
        Ok(())
    }
    fn enable_in<F, Fut>(
        d: &Path,
        f: &Fake,
        bundle: Option<&Path>,
        home: Option<&Path>,
        confirm: F,
    ) -> Result<LoginItemStatus, String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        futures::executor::block_on(authorize(bundle, home, confirm))?;
        register_in(d, f)
    }
    fn enable(d: &Path, f: &Fake) -> Result<LoginItemStatus, String> {
        enable_in(d, f, Some(Path::new(APP)), None, yes)
    }

    #[test]
    fn declined_confirm_registers_nothing_and_saves_nothing() {
        let (d, f) = (tmp("declined"), Fake::new());
        let r = enable_in(&d, &f, Some(Path::new(APP)), None, || async {
            Err::<(), String>("declined: Denied".into())
        });
        assert!(r.unwrap_err().starts_with("declined:"));
        assert!(f.calls.borrow().is_empty() && !wanted_in(&d));
    }

    #[test]
    fn wrong_bundle_location_is_refused_before_any_dialog_or_register() {
        let (d, f) = (tmp("loc"), Fake::new());
        let asked = Cell::new(false);
        for bundle in [
            "/Users/x/Downloads/Amara OS.app",
            "/Volumes/Amara/Amara OS.app",
            "/Applications/Sub/Amara OS.app",
            "/Applications/x",
        ] {
            let r = enable_in(
                &d,
                &f,
                Some(Path::new(bundle)),
                Some(Path::new("/Users/x")),
                || async {
                    asked.set(true);
                    Ok::<(), String>(())
                },
            );
            assert_eq!(r.unwrap_err(), ERR_LOCATION, "{bundle}");
        }
        let r = enable_in(&d, &f, None, None, yes);
        assert_eq!(r.unwrap_err(), ERR_LOCATION);
        assert!(!asked.get() && f.calls.borrow().is_empty() && !wanted_in(&d));
    }

    #[test]
    fn the_enable_command_asks_natively_before_registering() {
        let src = include_str!("login_item.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap()];
        let cmd = &prod[prod.find("pub async fn login_item_enable").unwrap()..];
        let ask = cmd.find("require_approval").unwrap();
        let loc = cmd.find("authorize(").unwrap();
        let reg = cmd.find("register_in(").unwrap();
        assert!(
            ask < loc && loc < reg,
            "confirm, then location check, then register"
        );
    }

    #[test]
    fn user_applications_folder_is_allowed_and_bundle_of_finds_the_app() {
        let home = Path::new("/Users/x");
        assert!(bundle_location_ok(
            Some(Path::new("/Users/x/Applications/Amara OS.app")),
            Some(home)
        ));
        assert!(!bundle_location_ok(
            Some(Path::new("/Users/y/Applications/Amara OS.app")),
            Some(home)
        ));
        let exe = Path::new("/Applications/Amara OS.app/Contents/MacOS/nexus");
        assert_eq!(bundle_of(exe).unwrap(), Path::new(APP));
        assert!(bundle_of(Path::new("/repo/target/debug/nexus")).is_none());
    }

    #[test]
    fn enable_registers_once_and_the_choice_persists() {
        let (d, f) = (tmp("enable"), Fake::new());
        let s = enable(&d, &f).unwrap();
        assert!(s.enabled && s.wanted);
        assert_eq!(*f.calls.borrow(), vec!["register"]);
        // a "restart": fresh backend object, same dir, reads only
        let f2 = Fake::new();
        f2.state.set(BackendStatus::Enabled);
        let s2 = status_in(&d, &f2);
        assert!(s2.wanted && s2.enabled);
        assert!(f2.calls.borrow().is_empty());
    }

    #[test]
    fn disable_unregisters_and_persists_off() {
        let (d, f) = (tmp("disable"), Fake::new());
        enable(&d, &f).unwrap();
        let s = disable_in(&d, &f).unwrap();
        assert!(!s.enabled && !s.wanted);
        assert_eq!(*f.calls.borrow(), vec!["register", "unregister"]);
        assert!(!wanted_in(&d));
    }

    #[test]
    fn disable_when_not_registered_does_not_call_the_system() {
        let (d, f) = (tmp("noop"), Fake::new());
        disable_in(&d, &f).unwrap();
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn failed_register_saves_nothing() {
        let (d, mut f) = (tmp("fail"), Fake::new());
        f.fail_register = true;
        assert!(enable(&d, &f).is_err());
        assert!(!wanted_in(&d));
    }

    #[test]
    fn requires_approval_is_reported_not_hidden() {
        let (d, mut f) = (tmp("approval"), Fake::new());
        f.after_register = BackendStatus::RequiresApproval;
        let s = enable(&d, &f).unwrap();
        assert!(s.wanted && !s.enabled && s.needs_approval);
    }

    #[test]
    fn unsupported_system_refuses_without_saving() {
        let (d, f) = (tmp("unsupported"), Fake::new());
        f.state.set(BackendStatus::Unsupported);
        assert!(enable(&d, &f).is_err());
        assert!(!wanted_in(&d) && f.calls.borrow().is_empty());
    }

    #[test]
    fn malformed_file_reads_as_not_wanted() {
        let d = tmp("bad");
        std::fs::write(path(&d), "{not json").unwrap();
        assert!(!wanted_in(&d));
    }

    #[test]
    fn nothing_is_registered_at_app_start_and_the_launch_agent_plugin_is_gone() {
        let lib = include_str!("../lib.rs");
        assert!(!lib.contains("login_item::login_item_enable(") && !lib.contains("enable_in("));
        // only generate_handler! mentions the module
        for line in lib.lines().filter(|l| l.contains("login_item::")) {
            assert!(
                line.trim_start()
                    .starts_with("commands::login_item::login_item_"),
                "unexpected use of login_item in lib.rs: {line}"
            );
        }
        assert!(!lib.contains("tauri_plugin_autostart"));
        assert!(!lib.contains("MacosLauncher"));
    }
}
