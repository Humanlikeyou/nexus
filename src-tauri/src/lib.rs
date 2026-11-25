mod app_paths;
#[cfg(test)]
mod bindings;
mod commands;
mod dock_icon;

use commands::n8n_status;
use commands::voice;
use commands::{
    arm, assistant, ble_common, business_ledger, continuity, crew, crew_grants, events,
    experiments, followups, goal_roadmap, goal_route, goals, ha, ha_devices, home_brief,
    home_hr_take, kb_status, kv, llm_router, muse, noop, noop_freeze, noop_query, onboarding,
    openclaw, outbox, packager, recordings, replay, research, restart_services, screen_opens,
    sessions, snapshots, system, take_events, take_log, telegram, terrence_actions,
    terrence_automations, terrence_context, terrence_priorities, terrence_projects,
    terrence_runtime, terrence_runtime_bridge,
};
// design-1111v stores
use commands::home_windows;
use commands::{adds, checkin, decisions_log, habits, life_dates, needs_you};
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
    pub use crate::commands::continuity::{
        list_session_logs, load_terrance_context, read_identity_file, read_profile_file,
        save_terrance_context, store_session_log, SessionLogEntry, TerranceContext,
    };
    pub use crate::commands::goals::{read_goals, save_goals};
    pub use crate::commands::muse;
    pub use crate::commands::noop::health_snapshot;
    pub use crate::commands::onboarding::{check_onboarding, complete_onboarding};
    pub use crate::commands::outbox::get_outbox_status;
    pub use crate::commands::private_store;
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
        crate::commands::shared::fsx::atomic_write(path, bytes)
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
    // 0050: one-time copy of private data older builds kept in the repository (counts-only log).
    commands::private_store::migrate_on_startup();
    tauri::Builder::default()
        .manage(terrence_runtime_bridge::NexusRuntimeBridge::default())
        .manage(commands::amara_turn::TurnRegistry::default())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        // Hemi-Sync audio: hemisync://localhost/<track id>, id-only, read-only (commands/hemisync.rs).
        .register_asynchronous_uri_scheme_protocol("hemisync", |_ctx, request, responder| {
            std::thread::spawn(move || responder.respond(commands::hemisync::handle(&request)));
        })
        .setup(|app| {
            // A process cannot resume an in-flight Hermes child after restart.
            let _ = terrence_actions::reconcile_interrupted_conversations();
            // 0052: settle executions a quit or crash interrupted (read-back only, never a re-run).
            std::thread::spawn(commands::execution::recover_on_startup);
            // 0053 step 1: create the activity and recommendation stores (idempotent; no existing file touched).
            std::thread::spawn(|| commands::amara_store::init_on_startup(&app_paths::data_dir()));
            // Navy local-only is session-only: off at every start, and the voice loop's flag file says so.
            commands::telegram::init_navy_flag();
            // Outbound is off unless Settings enabled it (and the env var does not force it off).
            commands::outbound::init_outbound_setting();
            // Saved-draft undo text is dropped from History once its 8 s window is over.
            commands::adds::trim_expired_drafts();
            // Updater: registered only when the switch, a real public key and https endpoints all
            // exist (none do today, so this is a no-op). Open at login is never touched here.
            if commands::updater::resolve_from_config(app.config()).is_ok() {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            // Per-launch token for Amara-owned loopback services (see commands/launch_token.rs).
            commands::launch_token::mint();
            // Services supervisor: observe-only probes and a cached snapshot (commands/supervisor).
            commands::supervisor::start();

            // System tray
            let quit = MenuItem::with_id(app, "quit", "Quit Amara OS", true, None::<&str>)?;
            let open = MenuItem::with_id(app, "open", "Open Amara OS", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;

            // One menu-bar icon: the Amara mark only, black on transparent, marked as a template so
            // macOS tints it for light and dark menu bars (the full-colour app icon showed as a blob,
            // and a second tray from tauri.conf.json showed as a solid square).
            TrayIconBuilder::with_id("amara")
                .icon(tauri::include_image!("icons/tray-template@2x.png"))
                .icon_as_template(true)
                .tooltip("Amara OS")
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
            // Nightly backup scheduler (in-app only; nothing runs while the app is closed).
            commands::backup_schedule::start(app.handle().clone());
            // Telegram replies (inbox files from Hermes) and the check-in questions Nexus sends (decision 0045).
            commands::inbox::start(app.handle().clone());
            commands::proactive::start();
            commands::daily_table::start(app.handle().clone());
            // Telegram relay timer: a no-op until the relay is on, approved and outbound is allowed.
            commands::notify_relay::start();
            // Treasury exception monitor: nightly with catch-up; does nothing until treasury.db exists.
            commands::treasury_monitor::start();
            // Route detector (1118a): proposes branches only, local tier, at most once a day.
            commands::goal_route::start();
            // Watchlist prices (0038): one poll per 15 min, only when outbound is allowed; inert in tests.
            commands::market_feed::start();
            // Voice only while the app is open (decision 0024): app_alive heartbeat every 5 s.
            commands::voice_heartbeat::start();

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
            commands::skills_lifecycle::list_skill_versions,
            commands::skills_lifecycle::propose_skill_version,
            commands::skills_lifecycle::activate_skill_version,
            commands::skills_lifecycle::reject_skill_version,
            commands::execution::list_executions,
            commands::execution::execution_history,
            commands::amara_store::commands::activity_list,
            commands::amara_store::commands::recommendation_list,
            commands::amara_store::commands::recommendation_get,
            commands::amara_store::commands::recommendation_create,
            commands::amara_store::commands::recommendation_respond,
            commands::amara_store::commands::recommendation_record_outcome,
            commands::amara_store::commands::approvals_list,
            commands::amara_store::commands::audit_trail,
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
            habits::habit_mark,
            habits::habit_days,
            adds::add_preview,
            adds::add_save,
            adds::add_undo,
            adds::add_history,
            checkin::checkin_save,
            checkin::checkin_list,
            commands::proactive::proactive_settings_get,
            commands::proactive::proactive_settings_set,
            commands::inbox::inbox_status,
            commands::patterns::patterns_read,
            commands::weekly_review::weekly_review_read,
            home_windows::home_window_preflight,
            home_windows::home_window_open,
            home_windows::home_window_layout,
            home_windows::home_window_focus,
            home_windows::home_window_close,
            home_windows::home_window_undo,
            home_windows::home_window_pop_out,
            home_windows::home_window_sites,
            home_windows::home_window_sign_out,
            needs_you::needs_you_list,
            commands::agents_now::agents_now,
            noop_query::noop_query,
            noop_query::noop_query_batch,
            noop_query::noop_binary_status,
            noop_freeze::noop_freeze_report,
            noop_freeze::noop_freeze_copy,
            noop_query::get_noop_settings,
            noop_query::set_noop_settings,
            home_brief::home_brief,
            // Amara's follow-up lists (commands/followups.rs)
            business_ledger::read_business_ledger,
            business_ledger::ledger_venture_save,
            business_ledger::ledger_venture_archive,
            business_ledger::ledger_fx_set,
            business_ledger::ledger_history_list,
            business_ledger::ledger_undo,
            followups::followups_list,
            followups::followups_session_edits,
            followups::followups_add_item,
            experiments::experiments_record_start,
            experiments::experiments_list,
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
            ha_devices::ha_devices_list,
            ha_devices::ha_device_set,
            // Telegram
            telegram::get_config,
            telegram::save_settings,
            telegram::get_telegram_messages,
            telegram::send_telegram_message,
            telegram::set_local_only,
            commands::outbound::outbound_get,
            commands::outbound::outbound_set,
            commands::login_item::login_item_status,
            commands::login_item::login_item_enable,
            commands::login_item::login_item_disable,
            commands::updater::update_status,
            commands::updater::update_check,
            commands::updater::update_install_now,
            commands::updater::update_set_prefs,
            telegram::test_telegram_connection,
            telegram::send_to_terrance,
            telegram::poll_telegram_updates,
            // Money, read-only (commands/money.rs)
            commands::money::money_accounts_list,
            commands::money::money_balances_list,
            commands::money::money_account_add,
            commands::money::money_watch_add,
            commands::money::money_balance_set_manual,
            commands::money_import_pick::money_import_pick,
            commands::treasury::treasury_stream_add,
            commands::treasury::treasury_stream_update,
            commands::treasury::treasury_streams_list,
            commands::treasury::treasury_import_pick,
            commands::treasury::treasury_import_preview,
            commands::treasury::treasury_attention_set,
            commands::treasury::treasury_attention_list,
            commands::treasury::treasury_income_month,
            commands::treasury::treasury_income_trend,
            commands::treasury::treasury_per_hour,
            commands::treasury::treasury_fx_status,
            commands::treasury_entries::treasury_entry_add,
            commands::treasury_entries::treasury_entry_undo,
            commands::treasury_entries::treasury_venture_month,
            commands::treasury_counts::treasury_counts_import_pick,
            commands::treasury_counts::treasury_counts_list,
            commands::treasury_sequences::treasury_sequence_draft,
            commands::treasury_sequences::treasury_sequence_list,
            commands::treasury_sequences::treasury_sequence_update,
            commands::treasury_sequences::treasury_sequence_approve,
            commands::treasury_sequences::treasury_results_import_pick,
            commands::treasury_sequences::treasury_results_list,
            commands::treasury_monitor::treasury_flags_list,
            commands::treasury_monitor::treasury_flags_ack,
            commands::treasury_monitor::treasury_monitor_run_now,
            commands::treasury_monitor::treasury_monitor_settings_get,
            commands::treasury_monitor::treasury_monitor_settings_set,
            commands::treasury_review::treasury_review_read,
            commands::treasury_review::treasury_review_build,
            commands::money::fx_rate_get,
            commands::money::fx_rate_set_manual,
            commands::money::fx_rate_refresh,
            commands::money_summary::money_summary_read,
            commands::money_summary::money_facts_read,
            commands::money_entries::money_entry_add,
            commands::money_entries::money_entry_void,
            commands::money_entries::money_entries_list,
            commands::money_entries::money_month_view,
            commands::money_entries::money_savings_goal_set,
            // Packager
            packager::pack_it_up,
            packager::unpack_backup,
            packager::restore_backup,
            packager::get_app_paths,
            commands::backups::backup_settings_get,
            commands::backups::backup_settings_set,
            commands::backups::backup_now,
            commands::backups::backup_list,
            commands::backups::backup_verify_last,
            commands::backup_schedule::backup_prune_plan,
            commands::backup_schedule::restore_preview,
            commands::service_control::service_restart,
            commands::service_logs::service_logs,
            commands::supervisor::services_status,
            commands::supervisor::service_events,
            commands::supervisor::services_summary,
            commands::supervisor::supervisor_get,
            commands::supervisor::supervisor_set_enabled,
            commands::service_logs::service_log_tail,
            commands::model_tiers::model_tiers,
            commands::model_download::model_download,
            commands::setup_state::setup_done,
            commands::setup_state::setup_complete,
            commands::setup_state::setup_reset,
            commands::services_start_all::services_start_all,
            commands::data_harden::data_dir_harden_status,
            commands::data_harden::data_dir_harden,
            commands::whoop_status::whoop_status,
            commands::noop_parity::noop_parity_report,
            commands::system_profile::system_profile,
            commands::daily_table::daily_rebuild,
            commands::daily_table::daily_range,
            commands::goal_progress::goal_progress_week,
            // Hemi-Sync (decision 0010)
            commands::hemisync::hemisync_import,
            commands::hemisync::hemisync_library,
            commands::hemisync::hemisync_session_link,
            commands::hemisync::hemisync_session_finish,
            commands::hemisync::hemisync_session_rate,
            commands::hemisync::hemisync_sessions,
            commands::hemisync::hemisync_stats,
            commands::hemisync::hemisync_ranking,
            commands::hemisync::hemisync_mark_done,
            commands::hemisync::hemisync_mark_wave_done,
            // Keychain secrets + n8n key file (write-only; never return a value)
            commands::keychain::secrets_status,
            commands::keychain::secret_set,
            commands::keychain_migrate::keychain_import_review,
            commands::keychain_migrate::keychain_migrate_one,
            commands::keychain_migrate::keychain_move_review,
            commands::keychain_migrate::keychain_move_one,
            commands::notify_relay::notify_settings_get,
            commands::notify_relay::notify_settings_set,
            commands::notify_relay::notify_preview,
            commands::notify_relay::notify_send_test,
            commands::notify_relay::notify_dispatch_due,
            commands::key_file::key_file_status,
            commands::key_file::key_file_make,
            commands::key_file::key_file_unlock,
            // Goals
            goals::read_goals,
            goals::save_goals,
            goals::delete_goal,
            goal_roadmap::roadmap_get,
            goal_roadmap::roadmap_request,
            goal_roadmap::roadmap_accept,
            goal_roadmap::roadmap_edit_step,
            goal_roadmap::roadmap_answer_step,
            goal_roadmap::roadmap_todays_question,
            goal_roadmap::roadmap_draft_pending,
            goal_roadmap::roadmap_catchup_status,
            goal_route::goal_route_get,
            goal_route::goal_step_edit,
            goal_route::goal_step_complete,
            goal_route::goal_branch_open,
            goal_route::goal_branch_accept,
            goal_route::goal_branch_decline,
            goal_route::goal_route_settings_get,
            goal_route::goal_route_settings_set,
            commands::watch_proposals::watch_proposals_list,
            commands::watch_proposals::watch_proposal_record,
            commands::watch_proposals::watch_proposal_dismiss,
            commands::market_feed::market_feed_status,
            commands::watchlist::watchlist_get,
            commands::watchlist::watchlist_research_now,
            commands::watchlist::watchlist_drop,
            commands::enterprise_views::finances_view,
            commands::enterprise_views::venture_tracking_list,
            commands::content_pipeline::content_stage_set,
            commands::content_pipeline::content_draft_propose,
            commands::content_drafts::content_draft_save,
            commands::content_drafts::content_draft_get,
            commands::goal_venture::goal_venture_set,
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
            screen_opens::screen_open_record,
            screen_opens::screen_open_counts,
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
            commands::amara_turn::amara_turn,
            commands::amara_turn::amara_turn_cancel,
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
            commands::memory_read::memory_list,
            commands::memory_write::memory_edit,
            commands::memory_write::memory_delete,
            commands::memory_write::memory_restore_last,
            n8n_status::n8n_status,
            kb_status::kb_status,
            commands::system_map::system_map_status,
            kb_status::crew_tools,
            // Amara Voice (~/projects/amara-voice)
            voice::voice_status,
            voice::voice_start,
            voice::voice_stop,
            voice::voice_recent_turns,
            voice::voice_list_voices,
            voice::voice_set_voice,
            voice::voice_stop_speaking,
            voice::voice_speak_ready,
            voice::voice_speak_local,
            voice::voice_dictate_ready,
            voice::dictate_once,
            commands::voice_bridge::voice_bridge_start,
            commands::voice_bridge::voice_bridge_stop,
            // Appearance
            dock_icon::sync_dock_icon,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Amara OS")
        .run(|app, event| match event {
            // Held only when the supervisor's act is on AND this launch started a docker service;
            // otherwise a no-op, so quitting is exactly what it was (commands/supervisor/lifecycle.rs).
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                let handle = app.clone();
                commands::supervisor::lifecycle::on_exit_requested(&api, move || {
                    handle.exit(code.unwrap_or(0))
                });
            }
            tauri::RunEvent::Exit => {
                commands::voice_heartbeat::stop();
                commands::launch_token::remove();
                commands::noop_query::clear_private_copies();
            }
            _ => {}
        });
}
