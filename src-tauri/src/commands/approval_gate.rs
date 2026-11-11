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

pub const APPROVAL_DIALOG_TITLE: &str = "Amara OS — Approve Action";
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

/// Lock key for one restartable service, shared by single and batch commands:
/// `svc:docker:<name>`.
pub fn service_key(service_id: &str) -> String {
    format!("svc:{service_id}")
}

/// Held for the whole approve + execute span; releases its keys on drop.
pub struct InFlightGuard(Vec<String>);

impl InFlightGuard {
    #[cfg(test)]
    fn acquire(key: &str) -> Result<Self, String> {
        Self::acquire_all(&[key.to_string()])
    }

    /// All-or-nothing: under one mutex hold, take every key (sorted, deduped)
    /// or none. If any key is already held, returns `busy:` and takes nothing.
    pub fn acquire_all(keys: &[String]) -> Result<Self, String> {
        let mut wanted: Vec<String> = keys.to_vec();
        wanted.sort();
        wanted.dedup();
        let mut set = IN_FLIGHT
            .lock()
            .map_err(|_| "Approval lock is unavailable.".to_string())?;
        if wanted.iter().any(|k| set.contains(k)) {
            return Err(format!(
                "{BUSY_PREFIX} An approval dialog or restart for this service is already in progress."
            ));
        }
        for key in &wanted {
            set.insert(key.clone());
        }
        Ok(InFlightGuard(wanted))
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = IN_FLIGHT.lock() {
            for key in &self.0 {
                set.remove(key);
            }
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
    keys: &[String],
    text: String,
) -> Result<InFlightGuard, String> {
    let guard = InFlightGuard::acquire_all(keys)?;
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
    fn acquire_all_is_all_or_nothing() {
        let single = InFlightGuard::acquire(&service_key("docker:t-shared")).unwrap();
        // Batch overlapping a single-service restart is busy and takes nothing.
        let batch = vec![service_key("docker:t-free"), service_key("docker:t-shared")];
        assert!(InFlightGuard::acquire_all(&batch)
            .err()
            .unwrap()
            .starts_with(BUSY_PREFIX));
        let free = InFlightGuard::acquire(&service_key("docker:t-free")).unwrap();
        drop(free);
        drop(single);
        // Overlapping batches.
        let b1 = InFlightGuard::acquire_all(&batch).unwrap();
        let b2 = vec![
            service_key("docker:t-shared"),
            service_key("docker:t-other"),
        ];
        assert!(InFlightGuard::acquire_all(&b2).is_err());
        assert!(InFlightGuard::acquire(&service_key("docker:t-other")).is_ok());
        drop(b1);
        assert!(InFlightGuard::acquire_all(&b2).is_ok());
    }

    #[test]
    fn text_puts_operation_first_and_refuses_oversize() {
        let t = approval_text("Run: docker restart n8n", "Why: requested by Amara OS").unwrap();
        assert!(t.starts_with("Run: docker restart n8n"));
        let err = approval_text("Run: x", &"y".repeat(MAX_APPROVAL_TEXT_CHARS)).unwrap_err();
        assert!(err.starts_with(REFUSED_PREFIX), "{err}");
    }
}
