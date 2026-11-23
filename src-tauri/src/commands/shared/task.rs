//! Run blocking work (file IO, sqlite) off the async runtime.

/// What a failed join (the closure panicked or was cancelled) turns into. Each caller keeps the
/// error text it had before the helpers were shared, because the UI maps some of them as codes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum JoinErr {
    /// A fixed code, e.g. `"store_error"`.
    Code(&'static str),
    /// `"{prefix}{join error}"`, e.g. `Prefix("Task failed: ")`.
    Prefix(&'static str),
    /// The join error's own text.
    Plain,
}

impl JoinErr {
    fn render(self, e: &dyn std::fmt::Display) -> String {
        match self {
            JoinErr::Code(code) => code.to_string(),
            JoinErr::Prefix(prefix) => format!("{prefix}{e}"),
            JoinErr::Plain => e.to_string(),
        }
    }
}

/// Run `f` on the blocking pool and return its result. `tauri::Error::JoinError` is transparent, so
/// `Plain` / `Prefix` render the same text as the tokio `JoinError` callers used to format.
pub(crate) async fn blocking<T: Send + 'static>(
    on_join_error: JoinErr,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| on_join_error.render(&e))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn passes_the_result_and_the_closure_error_through() {
        assert_eq!(blocking(JoinErr::Plain, || Ok(7)).await, Ok(7));
        assert_eq!(
            blocking::<()>(JoinErr::Plain, || Err("nope".into())).await,
            Err("nope".to_string())
        );
    }

    #[tokio::test]
    async fn a_panic_becomes_the_callers_error_text() {
        let code = blocking::<()>(JoinErr::Code("store_error"), || panic!("boom")).await;
        assert_eq!(code, Err("store_error".to_string()));
        let prefixed = blocking::<()>(JoinErr::Prefix("Task failed: "), || panic!("boom")).await;
        assert!(prefixed.unwrap_err().starts_with("Task failed: "));
    }
}
