//! The native open-file dialog for the Japanese-bank statement import. The renderer passes nothing:
//! the path comes from the dialog (tauri-plugin-dialog), is used here and is never returned.
//! Result: `{imported, skipped}` or one of a fixed set of error codes.

use super::money::{import_pick_in, ImportPicked};

#[tauri::command]
pub async fn money_import_pick(app: tauri::AppHandle) -> Result<ImportPicked, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a statement CSV")
            .add_filter("Statement CSV", &["csv"])
            .blocking_pick_file()
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
