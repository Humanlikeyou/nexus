# LANE-R: amara-router fixes (B01, B02, B03, B05, B07, B09)
- **Owner:** rust-executor (router)  **Crate:** amara-router only
## Objectives / acceptance criteria
- **B01** `restore_fast` (engine.rs ~647) no longer unloads deep on any fast request (incl. the 30-min home_brief cron). Add a grace period after the last deep use and/or a VRAM headroom check using `gpu_budget_gb` (config.rs:186, currently unused) — either wire it or remove it. Restore re-warm test coverage. Test fails before, passes after.
- **B02** `auto_deep` defaults to `false` in code (config.rs:157), matching config.example.toml. `auto_deep=false` also gates the classifier hard→deep paths (engine.rs:178, 187). Tests for both.
- **B03** Investigate stability: LaunchAgent last exit -9; router.log repeated "fast tier: no first token in time" and "deep load failed, using fast: fast stayed busy" through 9 Nov. Find the cause (read `~/Library/Logs/amara-router/router.log` read-only; never print prompt content) and fix it with a test, or document it in `amara-router/STABILITY.md`.
- **B05** config.rs:314-321: add `allow_remote_ollama` (default false) and use it instead of `anthropic_enabled` there. Refuse to load when `anthropic_enabled=true` and no bearer key is set for loopback clients. Tests for both.
- **B07** gemma_tools.rs:19 text tool-call recovery can invoke `mcp__vault__*`. Restrict recovery to an allowlist that excludes vault/credential tools (or require approval). Tests.
- **B09** `strip_phrase` (route.rs:146) is char-boundary safe. Test with multibyte input (Japanese, emoji) that panics before.
## Verification
fmt, clippy -D warnings, cargo test for amara-router: baseline 112 passed. Each fix: failing test output recorded before the fix.
## Review: qa-reviewer + security-reviewer (B05, B07).

## Result (rust-executor, 2026-11-11)
Gates (amara-router): `cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean; `cargo test` 124 passed (54 lib + 70 http; baseline 112, +12 tests, none removed). Router not rebuilt/restarted.
- B01 fail-before: `a_fast_request_does_not_unload_deep_inside_the_grace_period` ("deep was unloaded inside the grace period"), `a_fast_request_leaves_deep_when_both_fit_the_gpu_budget` (unloads [FAST, DEEP]). Pass-after: both + `a_fast_request_moves_deep_out_when_both_do_not_fit`, unit `plan_restore_rules`. `a_fast_request_waits_for_deep_then_restores_fast` now sets deep_grace_secs=0. gpu_budget_gb wired.
- B02 fail-before: `config::auto_deep_defaults_to_off`, `by_default_nothing_goes_deep_automatically`, `by_default_hard_looking_prompts_stay_fast_...`. Pass-after: same. route test `token_threshold_and_code_block` opts in to auto_deep; tests/http base_config sets auto_deep=true unless extra has "default_routing".
- B03: documented in amara-router/STABILITY.md (cause: fast/deep swap thrash; -9 cause NOT proven). Fixed by B01/B02.
- B05: `remote_ollama_needs_its_own_flag_not_anthropic_enabled` failed before; `cloud_tier_needs_a_bearer_key` (new fn, compile-fail before) and `loopback_requires_bearer_when_the_cloud_tier_is_on` (200 vs 401 before).
- B07: `vault_and_credential_tools_are_never_recovered` failed before (vault tool recovered); vault removed from allowlist plus blocked-word list.
- B09: `route::strip_phrase_is_char_boundary_safe` panicked before (route.rs:158 slice); fixed by char-wise matching.

### Security follow-ups (N1, N3, N4), rust-executor
fmt/clippy clean; cargo test 125 passed (55 lib + 70 http).
- N1: GET /health exempt from the loopback bearer check (body: model names, ollama_url, load state, cloud flags; no secrets or content). `loopback_requires_bearer_when_the_cloud_tier_is_on` extended: /health 200 and /v1/chat/completions + /api/tags 401 without key. Fail-before observed once (extended test hit 401 on /health before the exemption was in).
- N3: recoverable() requires `[a-z0-9_]+` after mcp__followups__ (test in vault_and_credential_tools_are_never_recovered, incl. U+212A). Test written with the fix, no separate fail-before run.
- N4: chose clamp: config validation rejects deep_grace_secs*1000 > fast_wait_ms (`deep_grace_cannot_exceed_fast_wait`). Also written with the fix, no separate fail-before run.
