mod app_paths;
mod commands;

use commands::{
    arm, assistant, ble_common, continuity, crew, events, goals, kv, llm_router, muse, noop,
    onboarding, openclaw, outbox, packager, replay, research, restart_services, sessions,
    snapshots, system, telegram, terrence_actions, terrence_automations, terrence_context,
    terrence_priorities, terrence_projects, terrence_runtime, terrence_runtime_bridge,
};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

/// Re-exports for the integration tests in `src-tauri/tests/`. Not an API:
/// the items stay private to the app; this only lets the tests reach them.
#[doc(hidden)]
pub mod test_support {
    use std::path::Path;

    pub use crate::app_paths::{
        data_dir, merge_legacy_config_in, migrate_legacy_data_dir_in, DATA_DIR_ENV, DATA_DIR_NAME,
    };
    pub use crate::commands::goals::{read_goals, save_goals};
    pub use crate::commands::noop::health_snapshot;
    pub use crate::commands::onboarding::{check_onboarding, complete_onboarding};
    pub use crate::commands::outbox::get_outbox_status;
    pub use crate::commands::sessions::list_sessions;

    /// Data paths of the modules moved onto app_paths::data_dir(), as
    /// (module, path): the dir itself, or kv/ and llm_usage.json for kv and llm_router.
    pub fn module_data_paths() -> Vec<(&'static str, std::path::PathBuf)> {
        use crate::commands::*;
        vec![
            ("replay", replay::nexus_data_dir()),
            ("terrence_context", terrence_context::nexus_data_dir()),
            ("arm", arm::nexus_data_dir()),
            ("terrence_runtime", terrence_runtime::app_data_dir()),
            ("snapshots", snapshots::nexus_data_dir()),
            ("llm_router", llm_router::usage_log_path()),
            ("sessions", sessions::nexus_data_dir()),
            ("kv", kv::kv_root()),
            ("assistant", assistant::nexus_data_dir()),
        ]
    }

    /// Data dir the packager packs from and restores into.
    pub fn packager_data_dir() -> std::path::PathBuf {
        crate::commands::packager::nexus_data_dir()
    }
    pub use crate::commands::path_safety::{safe_join, validate_file_name, validate_name};

    pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        crate::commands::continuity::atomic_write(path, bytes)
    }

    pub fn unpack_archive_checked(
        archive: &Path,
        dest: &Path,
        max_total_bytes: u64,
        max_entries: usize,
    ) -> Result<usize, String> {
        crate::commands::packager::unpack_archive_checked(
            archive,
            dest,
            max_total_bytes,
            max_entries,
        )
    }

    /// Whether a pack-relative path (e.g. `["openclaw", "skills", "a.md"]`) may be restored.
    pub fn restore_allowed(rel: &[&str]) -> bool {
        let rel: Vec<String> = rel.iter().map(|s| s.to_string()).collect();
        crate::commands::packager::restore_target(&rel).is_some()
    }

    /// (restored destinations, skipped entries) for an extracted pack.
    pub fn plan_restore(pack_root: &Path) -> Result<(Vec<String>, Vec<String>), String> {
        let plan = crate::commands::packager::plan_restore(pack_root)?;
        Ok((
            plan.items
                .iter()
                .map(|i| i.rel.to_string_lossy().to_string())
                .collect(),
            plan.skipped,
        ))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app_paths::migrate_legacy_data_dir();
    tauri::Builder::default()
        .manage(terrence_runtime_bridge::NexusRuntimeBridge::default())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // A process cannot resume an in-flight Hermes child after restart.
            let _ = terrence_actions::reconcile_interrupted_conversations();

            // System tray
            let quit = MenuItem::with_id(app, "quit", "Quit Amara OS", true, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open Amara OS", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Start outbox delivery worker
            outbox::start_delivery_worker(app.handle().clone());

            Ok(())
        })
        // Unregistered 2026-10-29: no live frontend caller (docs/terrance-merge-inventory.md).
        // The fns stay; re-add the `module::fn` line to restore one:
        // openclaw::{get_openclaw_sandbox, toggle_openclaw_sandbox},
        // terrence_actions::{list_terrence_tools, request_terrence_action},
        // terrence_context::create_context_item,
        // terrence_priorities::{record_terrence_decision, resolve_terrence_decision},
        // terrence_projects::upsert_terrence_project.
        .invoke_handler(tauri::generate_handler![
            // System
            system::get_system_info,
            system::health_check,
            system::check_docker_service,
            system::restart_docker_service,
            restart_services::restart_services,
            system::check_service_port,
            // OpenClaw / Amara
            openclaw::read_openclaw_status,
            openclaw::read_cron_jobs,
            openclaw::read_skills,
            openclaw::read_tasks,
            openclaw::read_content_pipeline,
            openclaw::read_daily_log,
            openclaw::get_openclaw_health,
            openclaw::restart_openclaw,
            openclaw::save_skill,
            openclaw::delete_skill,
            // Muse BLE
            muse::scan_muse,
            muse::connect_muse,
            muse::disconnect_muse,
            muse::start_eeg_stream,
            muse::stop_eeg_stream,
            muse::get_muse_state,
            // NOOP is the exclusive WHOOP/health reader. It owns strap sync and health storage.
            noop::get_noop_health_snapshot,
            // Telegram
            telegram::get_config,
            telegram::save_settings,
            telegram::get_telegram_messages,
            telegram::send_telegram_message,
            telegram::test_telegram_connection,
            telegram::send_to_terrance,
            telegram::poll_telegram_updates,
            // Packager
            packager::pack_it_up,
            packager::unpack_backup,
            packager::get_app_paths,
            // Goals
            goals::read_goals,
            goals::save_goals,
            goals::delete_goal,
            // Sessions
            sessions::save_session,
            sessions::list_sessions,
            sessions::delete_session,
            // Snapshots
            snapshots::save_snapshot,
            snapshots::list_snapshots,
            // Onboarding
            onboarding::check_onboarding,
            onboarding::complete_onboarding,
            // Event Store
            events::insert_events,
            events::query_events,
            events::migrate_existing_data,
            // BLE Proof Mode
            ble_common::set_proof_mode,
            ble_common::get_proof_log,
            // Outbox / Gateway Pipeline
            outbox::get_outbox_status,
            outbox::simulate_gateway_down,
            outbox::simulate_gateway_up,
            // ARM — Autonomous Revenue Module
            arm::arm_list_strategies,
            arm::arm_create_strategy,
            arm::arm_update_strategy,
            arm::arm_delete_strategy,
            arm::arm_activate_strategy,
            arm::arm_pause_strategy,
            arm::arm_kill_strategy,
            arm::arm_poll_revenue,
            arm::arm_get_revenue_summary,
            arm::arm_log_decision,
            arm::arm_list_decisions,
            arm::arm_update_decision_outcome,
            arm::arm_get_risk_status,
            arm::arm_set_global_limits,
            arm::arm_kill_switch,
            arm::arm_health_check,
            arm::arm_poll_all,
            arm::arm_get_config,
            arm::arm_save_config,
            // Continuity
            continuity::read_identity_file,
            continuity::read_profile_file,
            kv::load_kv,
            kv::save_kv,
            continuity::load_terrance_context,
            continuity::save_terrance_context,
            continuity::store_session_log,
            continuity::list_session_logs,
            continuity::archive_old_session_logs,
            continuity::prune_context_memory,
            continuity::load_custom_jobs,
            continuity::save_custom_jobs,
            continuity::get_session_insights,
            // Activity Log
            continuity::store_activity_entry,
            continuity::list_activity_entries,
            continuity::prune_activity_log,
            continuity::archive_activity_log,
            // Subagents
            continuity::load_subagents,
            continuity::save_subagents,
            // LLM Router
            llm_router::chat_with_router,
            llm_router::get_cost_estimate,
            llm_router::get_llm_usage,
            llm_router::clear_llm_usage,
            // Amara action registry and approval ledger
            terrence_actions::list_terrence_actions,
            terrence_actions::approve_terrence_action,
            terrence_actions::retry_terrence_action,
            terrence_actions::cancel_terrence_action,
            // Amara automation records from the persistent Hermes runner
            terrence_automations::list_terrence_automations,
            terrence_automations::list_terrence_automation_runs,
            // Amara canonical context
            terrence_context::list_context_items,
            terrence_context::correct_context_item,
            terrence_context::delete_context_item,
            terrence_context::get_terrence_context_snapshot,
            terrence_context::migrate_legacy_terrence_context,
            // Amara current-priority and decision loop
            terrence_priorities::get_current_terrence_priority_snapshot,
            terrence_priorities::list_terrence_decisions,
            // Amara project intelligence
            terrence_projects::list_terrence_projects,
            // Amara runtime (Hermes)
            terrence_runtime::get_terrence_runtime_health,
            terrence_runtime::list_terrence_capabilities,
            terrence_runtime::interact_with_terrence,
            terrence_runtime::list_terrence_interactions,
            // Assistant chat history
            assistant::create_chat_session,
            assistant::list_chat_sessions,
            assistant::save_chat_message,
            assistant::load_chat_history,
            assistant::delete_chat_session,
            // Research
            research::web_search,
            research::fetch_url,
            // Replay / Demo
            replay::start_eeg_recording,
            replay::stop_eeg_recording,
            replay::list_replays,
            replay::start_replay,
            replay::stop_replay,
            replay::start_demo_eeg,
            replay::stop_demo_eeg,
            // CrewAI
            crew::crew_list,
            crew::crew_start,
            crew::crew_status,
            crew::crew_stop,
            crew::crew_health,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Amara OS");
}
