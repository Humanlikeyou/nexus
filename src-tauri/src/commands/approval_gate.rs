//! Native approval gate for privileged commands invoked from the webview
//! (service restarts, sandbox changes). Mirrors the terrence_actions gate:
//! a native dialog titled "Nexus — Approve Action" showing the exact
//! operation, one in-flight dialog per command key, and Deny = error, no-op.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

pub const APPROVAL_DIALOG_TITLE: &str = "Nexus — Approve Action";
/// Longer text is refused rather than truncated.
pub const MAX_APPROVAL_TEXT_CHARS: usize = 2_000;
pub const DENIED_MESSAGE: &str = "Denied in the native approval dialog. Nothing was changed.";

static IN_FLIGHT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Held for the whole approve + execute span; releases the key on drop.
pub struct InFlightGuard(String);

impl InFlightGuard {
    pub fn acquire(key: &str) -> Result<Self, String> {
        let mut set = IN_FLIGHT
            .lock()
            .map_err(|_| "Approval lock is unavailable.".to_string())?;
        if !set.insert(key.to_string()) {
            return Err("An approval dialog for this operation is already open.".to_string());
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
            "This operation is too large to show in full for approval, so it was refused."
                .to_string(),
        );
    }
    Ok(text)
}

/// Acquire the per-key lock, show the native dialog, and return the guard if
/// the user clicked Allow. Deny returns `Err(DENIED_MESSAGE)`.
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
        Err(DENIED_MESSAGE.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_per_key_and_released_on_drop() {
        let a = InFlightGuard::acquire("test:restart:a").unwrap();
        assert!(InFlightGuard::acquire("test:restart:a").is_err());
        let _b = InFlightGuard::acquire("test:restart:b").unwrap();
        drop(a);
        assert!(InFlightGuard::acquire("test:restart:a").is_ok());
    }

    #[test]
    fn text_puts_operation_first_and_refuses_oversize() {
        let t = approval_text("Run: docker restart n8n", "Why: requested by Nexus").unwrap();
        assert!(t.starts_with("Run: docker restart n8n"));
        assert!(approval_text("Run: x", &"y".repeat(MAX_APPROVAL_TEXT_CHARS)).is_err());
    }
}
