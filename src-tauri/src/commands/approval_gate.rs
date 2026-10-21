//! Native approval gate for privileged commands invoked from the webview
//! (service restarts, sandbox changes). Mirrors the terrence_actions gate:
//! a native dialog titled "Nexus — Approve Action" showing the exact
//! operation, one in-flight dialog per command key, and Deny = error, no-op.
//!
//! Error strings are machine-distinguishable by prefix (stable contract for
//! the frontend; the text after the prefix is human-readable and may change):
//! - `declined:` — the user clicked Deny. Nothing was changed.
//! - `busy:`     — an approval dialog for the same key is already open.
//! - `refused:`  — the operation text is too large to show in full, so it was
//!   refused without a dialog.
//! - `expired:`  — the caller's deadline passed (before the dialog, or by the
//!   time Allow was clicked); nothing was run.
//! - `changed:`  — (command-specific) state changed while the dialog was open;
//!   the operation was refused and nothing was written.
//!
//! Any other error is an ordinary failure of the command itself.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

pub const APPROVAL_DIALOG_TITLE: &str = "Nexus — Approve Action";
/// Longer text is refused rather than truncated.
pub const MAX_APPROVAL_TEXT_CHARS: usize = 2_000;
/// Prefix for a Deny outcome.
pub const DECLINED_PREFIX: &str = "declined:";
/// Prefix when a dialog for the same key is already open.
pub const BUSY_PREFIX: &str = "busy:";
/// Prefix when the operation cannot be shown in full (oversize).
pub const REFUSED_PREFIX: &str = "refused:";
/// Error returned when the user clicks Deny.
pub fn denied_message() -> String {
    format!("{DECLINED_PREFIX} Denied in the native approval dialog. Nothing was changed.")
}

static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Held for the whole approve + execute span; releases the key on drop.
pub struct InFlightGuard(String);

impl InFlightGuard {
    pub fn acquire(key: &str) -> Result<Self, String> {
        let mut set = IN_FLIGHT
            .lock()
            .map_err(|_| "Approval lock is unavailable.".to_string())?;
        if !set.insert(key.to_string()) {
            return Err(format!(
                "{BUSY_PREFIX} An approval dialog for this operation is already open."
            ));
        }
        Ok(InFlightGuard(key.to_string()))
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = IN_FLIGHT.lock() {
            set.remove(&self.0);
        }
    }
}

/// Build the dialog body; the exact operation comes first.
pub fn approval_text(operation: &str, detail: &str) -> Result<String, String> {
    let text = format!("{operation}\n\n{detail}");
    if text.chars().count() > MAX_APPROVAL_TEXT_CHARS {
        return Err(
            format!("{REFUSED_PREFIX} This operation is too large to show in full for approval, so it was refused."),
        );
    }
    Ok(text)
}

/// Acquire the per-key lock, show the native dialog, and return the guard if
/// the user clicked Allow. Deny returns `Err(denied_message())`.
pub async fn require_approval(
    app: &tauri::AppHandle,
    key: &str,
    text: String,
) -> Result<InFlightGuard, String> {
    let guard = InFlightGuard::acquire(key)?;
    let app = app.clone();
    let allowed = tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
        app.dialog()
            .message(text)
            .title(APPROVAL_DIALOG_TITLE)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Allow".to_string(),
                "Deny".to_string(),
            ))
            .blocking_show()
    })
    .await
    .map_err(|error| format!("Approval dialog failed: {error}"))?;
    if allowed {
        Ok(guard)
    } else {
        Err(denied_message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_per_key_and_released_on_drop() {
        let a = InFlightGuard::acquire("test:restart:a").unwrap();
        let busy = InFlightGuard::acquire("test:restart:a").err().unwrap();
        assert!(busy.starts_with(BUSY_PREFIX), "{busy}");
        assert!(denied_message().starts_with(DECLINED_PREFIX));
        let _b = InFlightGuard::acquire("test:restart:b").unwrap();
        drop(a);
        assert!(InFlightGuard::acquire("test:restart:a").is_ok());
    }

    #[test]
    fn text_puts_operation_first_and_refuses_oversize() {
        let t = approval_text("Run: docker restart n8n", "Why: requested by Nexus").unwrap();
        assert!(t.starts_with("Run: docker restart n8n"));
        let err = approval_text("Run: x", &"y".repeat(MAX_APPROVAL_TEXT_CHARS)).unwrap_err();
        assert!(err.starts_with(REFUSED_PREFIX), "{err}");
    }
}
