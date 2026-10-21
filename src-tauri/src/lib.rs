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
            let quit = MenuItem::with_id(app, "quit", "Quit Nexus", true, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open Nexus", true, None::<&str>)?;
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
        .invoke_handler(tauri::generate_handler![
            // System
            system::get_system_info,
            system::health_check,
            system::check_docker_service,
            system::restart_docker_service,
            restart_services::restart_services,
            system::check_service_port,
            // OpenClaw / Terrance
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
            openclaw::get_openclaw_sandbox,
            openclaw::toggle_openclaw_sandbox,
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
            // Subagents
            continuity::load_subagents,
            continuity::save_subagents,
            // LLM Router
            llm_router::chat_with_router,
            llm_router::get_cost_estimate,
            llm_router::get_llm_usage,
            llm_router::clear_llm_usage,
            // Terrence action registry and approval ledger
            terrence_actions::list_terrence_tools,
            terrence_actions::request_terrence_action,
            terrence_actions::list_terrence_actions,
            terrence_actions::approve_terrence_action,
            terrence_actions::retry_terrence_action,
            terrence_actions::cancel_terrence_action,
            // Terrence automation records from the persistent Hermes runner
            terrence_automations::list_terrence_automations,
            terrence_automations::list_terrence_automation_runs,
            // Terrence canonical context
            terrence_context::create_context_item,
            terrence_context::list_context_items,
            terrence_context::correct_context_item,
            terrence_context::delete_context_item,
            terrence_context::get_terrence_context_snapshot,
            terrence_context::migrate_legacy_terrence_context,
            // Terrence current-priority and decision loop
            terrence_priorities::get_current_terrence_priority_snapshot,
            terrence_priorities::record_terrence_decision,
            terrence_priorities::list_terrence_decisions,
            terrence_priorities::resolve_terrence_decision,
            // Terrence project intelligence
            terrence_projects::upsert_terrence_project,
            terrence_projects::list_terrence_projects,
            // Terrence runtime (Hermes)
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
        .expect("error while running Nexus");
}
