//! Typed bridge (decision 0023, plan tasks/proposed/typed-bridge.md).
//!
//! `builder()` lists the commands that have a generated TypeScript binding (step 2: every command
//! src-console/bridge.ts calls except `amara_turn`, which takes a `Channel` and is step 3). `generate_handler!`
//! in lib.rs stays the source of truth for registration in this stage; this list only drives
//! `src-console/bindings.gen.ts`. Two tests: one rewrites the file (set `UPDATE_BINDINGS=1`), the
//! other fails when the committed file differs from a fresh export (staleness gate).
//!
//! BigInt decision log (decision 0023 requires one line per decision). Rule: ONE global decision,
//! `Builder::dangerously_cast_bigints_to_number()`, so every 64-bit integer exports as TS `number`
//! (option c). Reason: all 60-odd occurrences are values that serde_json already puts on the wire as
//! JSON numbers and that JS has always parsed as numbers (never as BigInt); no wire format changes.
//! None of them is an id/hash that can pass 2^53 (they are row ids, epoch seconds/ms, byte counts,
//! counters, durations, day counts, JPY amounts). Changing the Rust types (option a) was rejected:
//! each would need a range proof plus changes to SQLite/chrono code that also reads them.
//!
//! | Type.field (file) | Rust | Decision |
//! |---|---|---|
//! | LedgerFund.balance_jpy; VentureDetail.budget_used_jpy/fund_jpy; LedgerVenture.target_jpy; FundingRequest.amount_jpy; BusinessLedger.revenue_target_jpy (business_ledger.rs) | i64 / Option<i64> | c: number (JPY amounts) |
//! | ActivityEntry.started_at/completed_at/duration_ms (continuity.rs) | u64 | c: number (epoch ms) |
//! | CrewInfo.tokens/requests; CrewStepRow.id/start_ms/end_ms (crew.rs) | i64 / Option<i64> | c: number |
//! | CrewAuditRow.id/exit_code/duration_ms; crew_tool_audit arg `before` (crew_grants.rs) | i64 / Option<i64> | c: number |
//! | DailyRow.takes_*/followups_closed; DailyGoalRow.roadmap_steps_done/followups_closed; DailyBuild/DailyRange.built_at_ms (daily_table.rs) | Option<i64> / i64 | c: number (counts, epoch ms) |
//! | Experiment.followup_id; experiments_record_start arg `followup_id` (experiments.rs) | i64 | c: number (row id) |
//! | FollowupItem.id; followups_{mark_done,reopen_item,delete_item,update_item} arg `id` (followups.rs) | i64 | c: number (row id) |
//! | GoalWeek.this_week/prev_week; StalledGoal.days_since; CommonCause.before_days_*; CommonGoal.finished_days; InCommon.goals_considered (goal_progress.rs) | i64 / usize | c: number (day counts) |
//! | RankedTrack.rank/n; UnrankedTrack.n; TrackRanking.min_sessions (hemisync.rs) | usize | c: number |
//! | KbStatus.docs (kb_status.rs) | Option<u64> | c: number (document count) |
//! | NoopDailyMetric.resting_hr; NoopSleepSession.session_count/resting_hr; NoopHealthSnapshot.data_age_days; HeartSample.at; LiveHeartRate.at; NoopWorkout.avg_hr/max_hr/steps; HypnogramSegment.start/end (noop.rs) | i64 / Option<i64> / usize | c: number (bpm, counts, epoch s) |
//! | OutboxStatus.queue_depth/total_delivered/expired (outbox.rs) | u64 | c: number (counters) |
//! | RecordingListItem.duration_secs/size_bytes; read_recording_window arg `points` (recordings.rs) | u64 / Option<u64> / Option<usize> | c: number (seconds, bytes, point count) |
//! | RecordedSession.duration_secs/data_points (sessions.rs) | u64 / usize | c: number |
//! | LegacyContextMigrationReport.inserted/existing/skipped (terrence_context.rs) | u64 | c: number (counters) |
//! | TerrenceRuntimeHealth.active_schedules; TerrenceInteraction.duration_ms; AmaraNoteInput.duration_ms (terrence_runtime.rs) | u64 | c: number |
//! | VoiceStatus.state_age_s; voice_recent_turns arg `n` (voice.rs) | Option<u64> / Option<usize> | c: number |
//! | serde_json::Value payloads (ArmStrategy.config, CrewToolGrant.pending_request, TerrenceAction.parameters, ContextItem.metadata, RecordedSession data, voice turns) | Value | exported as the recursive JSON `Value` type (specta feature `serde_json`); its numbers are `number` under the same rule |
//!
//! Other notes: `f64` is exported by specta as `number | null` (NaN serialises as JSON null); the hand-written
//! TS types say `number`, which stays true for every finite value. `FollowupItemPatch.due/next` carry
//! `#[specta(type = Option<String>)]` because their `deserialize_with = "present"` hides the wire type from
//! specta-serde; the serde attributes are unchanged.

use crate::commands::{
    amara_turn, arm, business_ledger, continuity, crew, crew_grants, daily_table, experiments,
    followups, goal_progress, goals, hemisync, home_brief, kb_status, kv, llm_router, muse,
    n8n_status, noop, openclaw, outbox, recordings, replay, screen_opens, sessions, system,
    terrence_actions, terrence_automations, terrence_context, terrence_priorities,
    terrence_projects, terrence_runtime, voice,
};
use specta_typescript::Typescript;
use tauri_specta::{collect_commands, Builder};

const OUT_PATH: &str = "../src-console/bindings.gen.ts";
const HEADER: &str = "// GENERATED by tauri-specta from src-tauri/src/bindings.rs. DO NOT EDIT.\n\
// Regenerate: UPDATE_BINDINGS=1 cargo test --manifest-path src-tauri/Cargo.toml bindings\n\
// eslint-disable\n";

pub(crate) fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .dangerously_cast_bigints_to_number()
        .commands(collect_commands![
            kv::load_kv,
            kv::save_kv,
            screen_opens::screen_open_record,
            screen_opens::screen_open_counts,
            n8n_status::n8n_status,
            amara_turn::amara_turn_cancel,
            system::get_reduce_transparency,
            goals::read_goals,
            goal_progress::goal_progress_week,
            home_brief::home_brief,
            llm_router::get_allow_claude,
            llm_router::set_allow_claude,
            outbox::get_outbox_status,
            kb_status::crew_tools,
            kb_status::kb_status,
            business_ledger::read_business_ledger,
            hemisync::hemisync_ranking,
            daily_table::daily_range,
            daily_table::daily_rebuild,
            experiments::experiments_list,
            experiments::experiments_record_start,
            continuity::list_activity_entries,
            openclaw::read_content_pipeline,
            openclaw::read_tasks,
            arm::arm_get_revenue_summary,
            arm::arm_get_risk_status,
            arm::arm_list_strategies,
            crew::crew_decide_approval,
            crew::crew_health,
            crew::crew_list,
            crew::crew_list_approvals,
            crew::crew_run_steps,
            crew::crew_start,
            crew::crew_status,
            crew::crew_stop,
            crew_grants::crew_tool_audit,
            crew_grants::crew_tool_grant_get,
            crew_grants::crew_tool_grant_revoke,
            crew_grants::crew_tool_grant_set,
            followups::followups_add_item,
            followups::followups_add_muted_topic,
            followups::followups_delete_item,
            followups::followups_delete_muted_topic,
            followups::followups_list,
            followups::followups_mark_done,
            followups::followups_reopen_item,
            followups::followups_update_item,
            muse::connect_muse,
            muse::disconnect_muse,
            muse::get_muse_state,
            muse::scan_muse,
            muse::start_eeg_stream,
            muse::stop_eeg_stream,
            noop::get_noop_health_snapshot,
            noop::live_heart_rate,
            noop::noop_hypnogram,
            noop::noop_recent_workouts,
            recordings::get_session_samples,
            recordings::list_recordings,
            recordings::read_body_window,
            recordings::read_recording_window,
            recordings::recording_series,
            replay::mark_recording_aux,
            replay::start_eeg_recording,
            replay::stop_eeg_recording,
            sessions::list_sessions,
            sessions::save_session,
            terrence_actions::approve_terrence_action,
            terrence_actions::cancel_terrence_action,
            terrence_actions::list_terrence_actions,
            terrence_actions::retry_terrence_action,
            terrence_automations::list_terrence_automation_runs,
            terrence_automations::list_terrence_automations,
            terrence_context::correct_context_item,
            terrence_context::delete_context_item,
            terrence_context::get_terrence_context_snapshot,
            terrence_context::list_context_items,
            terrence_context::migrate_legacy_terrence_context,
            terrence_priorities::get_current_terrence_priority_snapshot,
            terrence_priorities::list_terrence_decisions,
            terrence_projects::list_terrence_projects,
            terrence_runtime::get_terrence_runtime_health,
            terrence_runtime::interact_with_terrence,
            terrence_runtime::list_terrence_capabilities,
            terrence_runtime::list_terrence_interactions,
            terrence_runtime::prepare_amara_turn,
            terrence_runtime::record_amara_interaction,
            voice::voice_list_voices,
            voice::voice_recent_turns,
            voice::voice_set_voice,
            voice::voice_start,
            voice::voice_status,
            voice::voice_stop,
            voice::voice_stop_speaking,
        ])
}

fn exporter() -> Typescript {
    Typescript::default().header(HEADER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn out() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(OUT_PATH)
    }

    #[test]
    fn export_bindings() {
        let tmp = std::env::temp_dir().join(format!("nexus-bindings-{}.ts", std::process::id()));
        builder().export(exporter(), &tmp).expect("export failed");
        let fresh = std::fs::read_to_string(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        if std::env::var("UPDATE_BINDINGS").is_ok() {
            std::fs::write(out(), &fresh).unwrap();
        }
        let disk = std::fs::read_to_string(out()).unwrap_or_default();
        assert_eq!(
            disk, fresh,
            "src-console/bindings.gen.ts is stale: run UPDATE_BINDINGS=1 cargo test --manifest-path src-tauri/Cargo.toml bindings"
        );
    }
}
