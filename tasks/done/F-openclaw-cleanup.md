# F: OpenClaw code cleanup (Taisun instruction F)

No removal-plan file found (searched repo, ~/projects, /workspace/nexus/handoff); worked from the spec. Nothing under ~/.openclaw, ~/.hermes or ~/Applications-backup touched; no tokens revoked; `telegram_bot_token`, `gateway_auth_token` (and `openclaw_path`) config fields kept.

## Changes
Rust (src-tauri):
- commands/openclaw.rs: removed read_openclaw_status, restart_openclaw (+kickstart), get_openclaw_health, read_cron_jobs, read_daily_log, sandbox fns, InstallPaths/launchd probing and their types/tests. Kept (live callers) read_skills, read_tasks, read_content_pipeline, save_skill, delete_skill; they still read ~/.openclaw/{skills,workspace} and return empty when absent.
- lib.rs: removed only the 5 openclaw handler lines + a stale comment; module name `openclaw` kept.
- restart_services.rs: `Service::OpenClaw` gone; "openclaw" is now `Unknown service`; dialog never mentions it.
- packager.rs: no ~/.openclaw backup, no "openclaw not installed" skip entry, `RestoreRoot::OpenClaw` and `restore_target` openclaw arm removed (old backups' `openclaw/` entries are listed as Skipped, never restored); get_app_paths no longer returns openclaw_dir.
- compute_mode "gateway" alias to local: already present (`retired_gateway_mode_is_local`, src/lib/llm-router.defaults.test.ts), untouched.
TS: selfDiagnosis.ts (no get_openclaw_health call, no openclaw service id or report fields), scheduler.ts (no OpenClaw note), lib/types.ts + lib/api.ts (legacy wrappers/types removed: OpenClawStatus/Health/Channels, CronJob, readCronJobs, readDailyLog, restart/status/health). docs/terrance-merge-inventory.md rows for removed commands dropped.
No src-console UI/markup/styles touched. bridge.ts already had no openclaw call. test/commands.registered.test.ts needed no edit (inventory-driven; passes). e2e mocks needed no edit (none mocked removed commands).

## Tests replaced (assert absence, none weakened)
- restart_services: 3 openclaw tests -> `openclaw_is_not_a_service_any_more`, `the_gate_runs_once_and_its_errors_stop_everything`; dialog text asserted free of "openclaw".
- packager: `without_openclaw_it_skips...`/`with_openclaw_it_packs...` -> `packing_never_expects_or_reads_openclaw`, `an_existing_dot_openclaw_is_not_backed_up`; allow-list, plan, apply, snapshot tests + src-tauri/tests/packager.rs now assert openclaw/ entries are skipped.
- selfDiagnosis.autoheal.test.ts, scheduler.health.test.ts, sanitisation.test.ts: openclaw-installed/offline cases -> "no OpenClaw call/field/note/restart" (incl. compute_mode gateway).
- Rust count: openclaw.rs 25 -> 3 tests (-22: status/health/install/sandbox/uid/daily-log/version tests of removed code), restart_services 8 -> 7 (-1); packager count unchanged (19 / 4). Net -23 vs 507 baseline (484 at my last isolated run; 523 later because other agents added tests).
- vitest: scheduler.health 5 -> 2 (-3), autoheal 8 -> 7 (-1) of removed OpenClaw cases; total 1472 passed / 9 skipped (baseline 1474, other agents' edits also in flight).

## Evidence
- tsc --noEmit: clean. npm run lint: clean. test:unit: 1472 passed / 9 skipped. commands.registered: 5 passed.
- cargo test src-tauri: 523 passed / 5 ignored (all targets), 0 failed.
- cargo fmt --check / clippy -D warnings: my files clean (rustfmt --check on openclaw, packager, restart_services, tests/packager = no diff). Whole-crate gate currently RED from other agents' in-progress files (keychain.rs unused import, key_file.rs auto-deref + constant assertion + unformatted, adds.rs char comparison); not touched.
- e2e home-subline, standalone-1111s, takes-1111t, errors: Playwright reported "14 passed", exit 0 (the run was piped through tail, so no per-spec breakdown).

## Open
- save_skill/delete_skill/read_skills still use ~/.openclaw/skills (save_skill would recreate the dir); moving to app data needs Taisun's call.
- `openclaw_path` config field and error-copy mappers in continuity.ts left (config/legacy error strings).
