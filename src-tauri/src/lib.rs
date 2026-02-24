mod commands;

use commands::{arm, ble_common, events, goals, muse, onboarding, openclaw, outbox, packager, replay, sessions, snapshots, system, telegram, whoop};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            // System tray
            let quit = MenuItem::with_id(app, "quit", "Quit Electron", true, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open Electron", true, None::<&str>)?;
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
            // OpenClaw / Terrance
            openclaw::read_openclaw_status,
            openclaw::read_cron_jobs,
            openclaw::read_skills,
            openclaw::read_tasks,
            openclaw::read_content_pipeline,
            openclaw::read_daily_log,
            openclaw::get_openclaw_health,
            // Muse BLE
            muse::scan_muse,
            muse::connect_muse,
            muse::disconnect_muse,
            muse::start_eeg_stream,
            muse::stop_eeg_stream,
            muse::get_muse_state,
            // Whoop BLE
            whoop::scan_whoop,
            whoop::connect_whoop,
            whoop::disconnect_whoop,
            whoop::start_hr_stream,
            whoop::stop_hr_stream,
            whoop::get_whoop_state,
            // Telegram
            telegram::get_config,
            telegram::save_settings,
            telegram::get_telegram_messages,
            telegram::send_telegram_message,
            telegram::test_telegram_connection,
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
            // Replay / Demo
            replay::start_eeg_recording,
            replay::stop_eeg_recording,
            replay::list_replays,
            replay::start_replay,
            replay::stop_replay,
            replay::start_demo_eeg,
            replay::stop_demo_eeg,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Electron");
}
