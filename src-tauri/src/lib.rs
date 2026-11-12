mod app_paths;
mod commands;
mod dock_icon;

use commands::n8n_status;
use commands::voice;
use commands::{
    arm, assistant, ble_common, business_ledger, continuity, crew, crew_grants, events, followups,
    goals, ha, home_brief, home_hr_take, kb_status, kv, llm_router, muse, noop, noop_query,
    onboarding, openclaw, outbox, packager, recordings, replay, research, restart_services,
    sessions, snapshots, system, take_events, take_log, telegram, terrence_actions,
    terrence_automations, terrence_context, terrence_priorities, terrence_projects,
    terrence_runtime, terrence_runtime_bridge,
};
// design-1111v stores
use commands::{adds, checkin, decisions_log, life_dates, needs_you};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

/// Re-exports for the integration tests in `src-tauri/tests/`. Not an API:
/// the items stay private to the app; this only lets the tests reach them.
#[doc(hidden)]
pub mod test_support {
    use std::path::Path;

    pub use crate::app_paths::{
        data_dir, merge_legacy_config_in, migrate_legacy_data_dir_in, private_open_options,
        DATA_DIR_ENV, DATA_DIR_NAME,
    };
    pub use crate::commands::goals::{read_goals, save_goals};
    pub use crate::commands::muse;
    pub use crate::commands::noop::health_snapshot;
    pub use crate::commands::onboarding::{check_onboarding, complete_onboarding};
    pub use crate::commands::outbox::get_outbox_status;
    pub use crate::commands::recordings;
    pub use crate::commands::replay::{
        mark_recording_aux, maybe_record_motion, maybe_record_ppg, maybe_record_sample,
        start_eeg_recording, stop_eeg_recording, AuxInfo, StreamSet,
    };
    pub use crate::commands::sessions::{
        list_sessions, save_session, RecordedSession, SessionSummary,
    };

    /// Open events.db (events + arm) and terrence_core.db through their real open paths, so
    /// tests can check the db and -wal/-shm modes.
    pub fn open_private_dbs() -> Result<(), String> {
        use crate::commands::*;
        let e = events::get_events_db()?;
        e.execute_batch(
            "CREATE TABLE IF NOT EXISTS _mode_probe(x); INSERT INTO _mode_probe VALUES (1);",
        )
        .map_err(|err| err.to_string())?;
        arm::arm_list_strategies()?;
        let t = terrence_context::open_terrence_core_db()?;
        t.execute_batch(
            "CREATE TABLE IF NOT EXISTS _mode_probe(x); INSERT INTO _mode_probe VALUES (1);",
        )
        .map_err(|err| err.to_string())?;
        drop((e, t));
        Ok(())
    }

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

    /// Whether a pack-relative path (e.g. `["sessions", "a.json"]`) may be restored.
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
    app_paths::check_dev_config_dir();
    app_paths::migrate_legacy_data_dir();
    tauri::Builder::default()
        .manage(terrence_runtime_bridge::NexusRuntimeBridge::default())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        // Hemi-Sync audio: hemisync://localhost/<track id>, id-only, read-only (commands/hemisync.rs).
        .register_asynchronous_uri_scheme_protocol("hemisync", |_ctx, request, responder| {
            std::thread::spawn(move || responder.respond(commands::hemisync::handle(&request)));
        })
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

            if let Some(window) = app.get_webview_window("main") {
                dock_icon::sync(&window);
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::ThemeChanged(_) => {
                if let Some(webview) = window.app_handle().get_webview_window(window.label()) {
                    dock_icon::sync(&webview);
                }
            }
            // The setting is changed in System Settings, so a change shows on coming back.
            tauri::WindowEvent::Focused(true) => {
                system::sync_reduce_transparency(window.app_handle());
            }
            _ => {}
        })
        // Unregistered 2026-10-29: no live frontend caller (docs/terrance-merge-inventory.md).
        // The fns stay; re-add the `module::fn` line to restore one:
        // terrence_actions::{list_terrence_tools, request_terrence_action},
        // terrence_context::create_context_item,
        // terrence_priorities::{record_terrence_decision, resolve_terrence_decision},
        // terrence_projects::upsert_terrence_project.
        .invoke_handler(tauri::generate_handler![
            // System
            system::get_system_info,
            system::get_reduce_transparency,
            system::health_check,
            system::check_docker_service,
            system::restart_docker_service,
            restart_services::restart_services,
            system::check_service_port,
            // Skills / tasks / content pipeline
            openclaw::read_skills,
            openclaw::read_tasks,
            openclaw::read_content_pipeline,
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
            noop::live_heart_rate,
            noop::noop_recent_workouts,
            noop::noop_hypnogram,
            home_hr_take::hr_window,
            home_hr_take::home_hr_take_status,
            home_hr_take::home_hr_take_maybe_refresh,
            take_events::take_events_append,
            take_events::take_events_list,
            take_log::take_log_append,
            take_log::take_log_list,
            take_log::take_log_forget,
            take_log::take_log_migrate,
            // design-1111v: decisions log, life dates, adds, check-in, Needs you today
            decisions_log::decisions_list,
            decisions_log::decisions_append,
            decisions_log::decisions_undo,
            decisions_log::decisions_add_outcome,
            life_dates::life_date_list,
            life_dates::life_date_upsert,
            life_dates::life_date_delete,
            life_dates::life_date_preview,
            adds::add_preview,
            adds::add_save,
            adds::add_undo,
            adds::add_history,
            checkin::checkin_save,
            checkin::checkin_list,
            needs_you::needs_you_list,
            noop_query::noop_query,
            noop_query::get_noop_settings,
            noop_query::set_noop_settings,
            home_brief::home_brief,
            // Amara's follow-up lists (commands/followups.rs)
            business_ledger::read_business_ledger,
            followups::followups_list,
            followups::followups_session_edits,
            followups::followups_add_item,
            followups::followups_update_item,
            followups::followups_mark_done,
            followups::followups_reopen_item,
            followups::followups_delete_item,
            followups::followups_add_muted_topic,
            followups::followups_delete_muted_topic,
            // Home Assistant (commands/ha.rs)
            ha::ha_states,
            ha::ha_call_service,
            ha::ha_conversation,
            ha::ha_container_status,
            ha::ha_container_start,
            ha::ha_container_stop,
            ha::ha_live_start,
            ha::ha_live_stop,
            ha::ha_get_config,
            ha::ha_set_config,
            ha::ha_clear_token,
            ha::ha_test_connection,
            // Telegram
            telegram::get_config,
            telegram::save_settings,
            telegram::get_telegram_messages,
            telegram::send_telegram_message,
            telegram::set_local_only,
            telegram::test_telegram_connection,
            telegram::send_to_terrance,
            telegram::poll_telegram_updates,
            // Money, read-only (commands/money.rs)
            commands::money::money_accounts_list,
            commands::money::money_account_add,
            commands::money::money_watch_add,
            commands::money::money_balance_set_manual,
            commands::money::money_import_statement,
            commands::money::fx_rate_get,
            commands::money::fx_rate_set_manual,
            commands::money::fx_rate_refresh,
            // Packager
            packager::pack_it_up,
            packager::unpack_backup,
            packager::get_app_paths,
            commands::backups::backup_settings_get,
            commands::backups::backup_settings_set,
            commands::backups::backup_now,
            commands::backups::backup_list,
            // Hemi-Sync (decision 0010)
            commands::hemisync::hemisync_import,
            commands::hemisync::hemisync_library,
            commands::hemisync::hemisync_session_link,
            commands::hemisync::hemisync_session_finish,
            commands::hemisync::hemisync_session_rate,
            commands::hemisync::hemisync_sessions,
            commands::hemisync::hemisync_stats,
            // Keychain secrets + n8n key file (write-only; never return a value)
            commands::keychain::secrets_status,
            commands::keychain::secret_set,
            commands::key_file::key_file_status,
            commands::key_file::key_file_make,
            commands::key_file::key_file_unlock,
            // Goals
            goals::read_goals,
            goals::save_goals,
            goals::delete_goal,
            // Sessions
            sessions::save_session,
            sessions::list_sessions,
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
            llm_router::get_allow_claude,
            llm_router::set_allow_claude,
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
            terrence_runtime::prepare_amara_turn,
            terrence_runtime::record_amara_interaction,
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
            replay::mark_recording_aux,
            recordings::list_recordings,
            recordings::read_recording_window,
            recordings::read_body_window,
            recordings::recording_series,
            recordings::get_session_samples,
            // CrewAI
            crew::crew_list,
            crew::crew_start,
            crew::crew_status,
            crew::crew_run_steps,
            crew::crew_stop,
            crew::crew_list_approvals,
            crew::crew_decide_approval,
            crew::crew_health,
            crew_grants::crew_tool_grant_get,
            crew_grants::crew_tool_grant_set,
            crew_grants::crew_tool_grant_revoke,
            crew_grants::crew_tool_audit,
            n8n_status::n8n_status,
            kb_status::kb_status,
            kb_status::crew_tools,
            // Amara Voice (~/projects/amara-voice)
            voice::voice_status,
            voice::voice_start,
            voice::voice_stop,
            voice::voice_recent_turns,
            voice::voice_list_voices,
            voice::voice_set_voice,
            voice::voice_stop_speaking,
            commands::voice_bridge::voice_bridge_start,
            commands::voice_bridge::voice_bridge_stop,
            // Appearance
            dock_icon::sync_dock_icon,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Amara OS");
}
