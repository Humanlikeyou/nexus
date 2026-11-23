//! The native open-file dialog for the Japanese-bank statement import. The renderer passes nothing:
//! the path comes from the dialog (tauri-plugin-dialog), is used here and is never returned.
//! Result: `{imported, skipped}` or one of a fixed set of error codes. One import at a time:
//! a second call while the dialog or an import is open returns `busy`.

use super::money::{import_pick_in, ImportPicked};
use std::sync::atomic::{AtomicBool, Ordering};

static OPEN: AtomicBool = AtomicBool::new(false);

/// Held for the whole call (dialog and import). Dropping it, on any path including a panic, frees the slot.
struct Slot;

impl Slot {
    fn take() -> Option<Slot> {
        OPEN.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        OPEN.store(false, Ordering::Release);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn money_import_pick(app: tauri::AppHandle) -> Result<ImportPicked, String> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;
    let _slot = Slot::take().ok_or_else(|| "busy".to_string())?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_title("Choose a statement CSV")
            .add_filter("Statement CSV", &["csv"]);
        // Parent the sheet to the main window when there is one, so it can't hide behind it.
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_pick_file()
    })
    .await
    .map_err(|_| "failed".to_string())?;
    let path = picked.and_then(|p| p.into_path().ok());
    tauri::async_runtime::spawn_blocking(move || {
        import_pick_in(&crate::app_paths::data_dir(), path.as_deref())
    })
    .await
    .map_err(|_| "failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production_source() -> String {
        let src = include_str!("money_import_pick.rs");
        let marker = concat!("#[cfg(test)]", "\nmod tests {");
        src.split(marker).next().unwrap().to_string()
    }

    // The slot is a process-wide static, so every assertion about it lives in this one test.
    #[test]
    fn one_import_at_a_time_and_the_slot_frees_on_drop() {
        let first = Slot::take();
        assert!(first.is_some());
        assert!(
            Slot::take().is_none(),
            "a second call while one is open is busy"
        );
        drop(first);
        let again = Slot::take();
        assert!(again.is_some(), "dropping the guard frees the slot");
        // A panic while holding the slot frees it too.
        drop(again);
        let r = std::panic::catch_unwind(|| {
            let _s = Slot::take().unwrap();
            panic!("boom");
        });
        assert!(r.is_err());
        assert!(Slot::take().is_some());
    }

    #[test]
    fn exactly_one_command_and_it_is_not_named_like_a_money_mover() {
        let src = production_source();
        assert_eq!(src.matches("#[tauri::command]").count(), 1);
        let name = src
            .split("pub async fn ")
            .nth(1)
            .unwrap()
            .split('(')
            .next()
            .unwrap();
        assert_eq!(name, "money_import_pick");
        for banned in [
            "transfer",
            "pay",
            "trade",
            "convert",
            "withdraw",
            "send",
            "buy",
            "sell",
            "move",
            "wire",
            "remit",
            "swap",
            "sign",
            "broadcast",
            "order",
            "deposit",
            "exchange",
        ] {
            assert!(!name.contains(banned), "{name} contains {banned}");
        }
        // No URL and no network client here, and the dialog is parented to the main window.
        assert_eq!(src.matches(concat!("http", "://")).count(), 0);
        assert_eq!(src.matches(concat!("https", "://")).count(), 0);
        assert!(!src.contains("reqwest"));
        assert!(src.contains(".set_parent("));
    }
}
