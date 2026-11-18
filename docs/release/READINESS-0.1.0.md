# Release readiness 0.1.0 (Phase 17, 2026-11-18)

Base: HEAD `71d8d7e` plus this run's uncommitted Phase 16/17 files (nothing committed, pushed, merged or
opened as a PR). **Verdict: not releasable yet.** Code and tests are green; signing, notarization, the
updater, the login item, the live parity/relay runs and the fresh-account migration are all waiting on
Taisun. Capability-by-capability status: `docs/release/parity-0.1.0.md`.

## Gates this run

| Gate | Result |
|---|---|
| `.claude/skills/run-gates/gates.sh` | tsc PASS; lint PASS; vitest 2351 passed / 9 skipped; neuro 21 pass / 0 fail; check:secrets ok (1062 tracked files); src-tauri fmt + clippy PASS, 1455 passed / 0 failed / 6 ignored; amara-router fmt + clippy PASS, 131 passed / 0 failed / 0 ignored |
| Full e2e `NEXUS_OUTBOUND_DISABLED=1 npm run test:e2e` (alone, both themes) | **784 passed, 12 skipped, 0 failed** (3.9 min), axe serious/critical tests all passed, including `a11y.spec.ts` and the per-screen axe tests in both themes |
| crew-service (Docker was running; `docker run --rm --network none crew-test`) | **230 passed** (images `crew-base`, `crew-test` rebuilt; no container touched) |
| swift test for NOOP packages | **not run** (NOOP repo out of bounds this run) |
| `scripts/migration-probe.sh` | PASS, 9 probe lines (synthetic data) |

Counts did not drop (baseline cd5e4cc: vitest 1140, src-tauri 418, router 112). Added this run: 1 Rust test
(`migration_probe_tests`), 2 vitest tests (`noAbsolutePaths.test.ts`).

### The 12 skipped e2e tests
All are opt-in screenshot generators, skipped by `test.skip(() => !process.env.X, ...)`:
`landing.spec.ts:215` (`LANDING_SHOTS`, 10 skips = 5 shots x 2 themes) and `lists.spec.ts:191`
(`LISTS_SHOTS`, 2 skips). They are Design's sign-off screenshot tools, not assertions, and existed before
this run. They were not changed and not run (they write image files).

### takes-1111t.spec.ts:80 [light]: investigated, NOT fixed (Design-owned)
- The full e2e passed it this run, but it is intermittent: `--repeat-each=6 --project=light` ran it alone
  and 5 of 6 failed, each with axe `color-contrast: div[role="alert"] > b`.
- Cause (unchanged from `tasks/design-questions.md` 2026-11-13): `.m-warn b` uses `--copper-text`
  (light `#8c6239`, `tokens.css:38`) on a 10% copper tint over a translucent sheet. Measured earlier at
  4.28:1 (needs 4.5:1). Whether the first run passes depends on what is behind the sheet when axe runs.
- A fix is a light-theme token or `money.css` colour change, which is Design's call
  (`.claude/rules/design-boundary.md`); there is no logic-only fix. The test stays unweakened.

## Definition-of-done boxes (handover section c)

Key: PASS = evidence in repo; WAIT = waiting on Taisun; NOT BUILT.

**Product and install**
- [ ] Signed, notarized, stapled DMG. WAIT. `scripts/release-local.sh` (`71d8d7e`) exits 2 with 0 Developer ID identities. Existing DMG `src-tauri/target/release/bundle/dmg/Amara OS_0.1.0_aarch64.dmg` is unsigned/ad-hoc, built before the Stage H review fixes. `spctl`, `stapler`, `codesign --deep --strict`: not run.
- [ ] Bundle id, productName, crate names unchanged; version bumped by a logged decision. Names PASS (`test/scripts/stageH.test.ts`). Bump WAIT (still 0.1.0, Q14.4).
- [ ] Auto-update on, signed manifest, rollback. WAIT. Built and tested with a fake host (`71d8d7e`); `UPDATER_SWITCH=false`, placeholder pubkey, no host. One real update and one forced rollback: not run.
- [ ] Opens at login. WAIT. `login_item.rs` SMAppService, default off, not registered live (`71d8d7e`); Startup toggle not wired (Design).
- [ ] First run reaches Home with all lights green on a clean account. WAIT. Built (`11c7883`, `test/e2e/standalone-1111s.spec.ts` passes); clean-account run is `docs/release/migration-test.md` Part B.

**Runtime and security**
- [ ] Amara supervises the services. PARTIAL. Observe-only supervisor (`2642629`, 15 services); act paths behind `SUPERVISOR_ACT_ENABLED=false`; LaunchAgent retirement and crew rebuild gated.
- [ ] Loopback only, per-launch token, Host/Origin checks, Keychain. PARTIAL. `988a464`, `a1d1cb5`, `a33e623`; Letta and n8n do not check Host/Origin.
- [ ] Capability inventory parity. PARTIAL. See parity-0.1.0.md; `standalone-app-plan.md` section 4 missing from the repo.
- [ ] Open WebUI side tool outside the DMG; OpenClaw removed. PASS (`889bd19`; DMG holds 20 files, no Open WebUI).
- [ ] Cloud AI off by default; navy forces local. PASS in tests (`988a464`, `6215a12`, `0dd3996`); Hermes web tools under navy: Q1 open.
- [ ] No secret anywhere; sentinel tests pass. PASS for scans (below); Keychain moves need his UI approval (0028).
- [ ] Unified logs rotated/redacted/0600. PASS in tests (`988a464`).
- [ ] Data dir 0700/0600. WAIT: one-time chmod needs his OK (`data_harden.rs` built).

**Data safety**
- [x] Nightly backups 03:00, 14 kept, 0600 in 0700, checksummed, exclude Keychain and n8n key file. PASS (`6507382`, `1c93d5e`, `a783a5f`, `de30ebe`; probe confirms archive 0600 / dir 0700 and secrets absent).
- [x] Restore drill on a copy, counts and hashes match. PASS (`docs/release/restore-drill.md`: 3,294 of 3,294; plus synthetic probe this run).
- [ ] Migration test. PARTIAL: automated part PASS (`scripts/migration-probe.sh`; no absolute path in shipped source, `noAbsolutePaths.test.ts`); fresh-account part WAIT (migration-test.md Part B).
- [x] Muse recordings untouched, zero deletions, checked by a test. PASS: no delete path (`a34a224`, `e182d9f`), byte-identical backups. Count check below.

**NOOP fully absorbed**
- [ ] Sidecars bundled and signed; WHOOP 4 single-writer. NOT BUILT / WAIT (Phase 10 takeover on hardware).
- [ ] Store copied into app-data; old store read-only 30 days. NOT BUILT.
- [ ] Lucid cues from the Mac; Oura/Hevy/coach secrets in Keychain; NOOP.app retired. NOT BUILT (Phase 12, 2-week parity).
- [ ] NOOP attribution in About and NOTICE. NOT DONE: no `NOTICE` file, no PolyForm text in code or UI.
- [ ] Telegram relay in Amara's words with checked numbers. PARTIAL: built and tested (`a783a5f`, `bddb46f`); timer inert; live send needs token, chat, Send Test.

**Designs wired**
- [x] 1110e-1110i, 1111a, 1111s, 1111t, 1111u, 1111v, 1111w, 1111x. Built with the gaps listed in `tasks/BOARD.md` (habits source, Jarvis comment/blocked-embed signals, voice patches, Life Dates edit form, take rating control, Money gated parts). Known data gaps documented, not faked: nutrition, ledger file.

**Verification**
- [x] Full gates and full e2e green, axe serious/critical = 0 both themes. PASS this run (one intermittent Design contrast test, above).
- [x] Parity matrix written. PASS (`parity-0.1.0.md`, with the missing-source caveat).
- [x] This report. PASS.
- [x] Nothing pushed. PASS.

## No secrets: scans
- Repo: `npm run check:secrets` ok (1062 tracked files), twice (gates).
- DMG: mounted read-only (`hdiutil attach -readonly -nobrowse`, temp mountpoint, detached afterwards). 20 files, 140,768,308 bytes scanned with the repo's `scanText` over printable strings. No file named like a key, token, secret, password, credential or `.env`. 6 `assigned-high-entropy` hits, all inside `Contents/Resources/ollama-runtime/ollama` (a Go binary); the matched names are env-var NAMES such as ANTHROPIC_API_KEY, OPENAI_API_KEY, COPILOT_PROVIDER_API_KEY in embedded text next to minified strings. Judged false positives; values not printed. Caveat: the DMG was built before the Stage H review fixes, so it is not a scan of the final config.
- Archives and logs: no real archive or log was opened. Synthetic archive probe confirms the packer leaves out secret files.

## Muse files untouched: counts only (no contents read)
`find <dir> -type f | wc -l` under the live app-data folder: `replays` 23, `sessions` 13, `muse` 7.
Restore-drill baseline on 2026-11-13 was 18 / 11 / 5. Counts are higher, which fits new recordings since
then; counts cannot prove no file was altered. This run made no write to those folders (the probe uses temp dirs only).

## Residual risks
- M1: loopback reach remains for other ports while the sandboxed `ollama serve` runs (handoff 25a M1); only the download window is mutually exclusive.
- Signing/notarization not done; no Developer ID. Hardened runtime is false in config and applied only by the signed script path: the first signed build must be live-tested with Muse BLE, Ollama and the router.
- Updater inert (switch false, placeholder key, no host, no real swap host).
- Login item untested live (never registered on this Mac).
- NOOP parity report and Telegram relay not run live; Q4 (what parity means after takeover) open.
- Treasury phases 3-5 not built.
- WHOOP takeover, Lucid cues, NOOP.app retirement and NOOP attribution not built.
- Letta and n8n do not check Host/Origin; Letta stays 0.16.8 (CVE upgrade not applied).
- Light-theme contrast on `.m-warn b` (4.28:1) is intermittent in e2e; Design's fix.
- Hemi-Sync audio of 50 MB or more is left out of backups (open question in restore-drill.md).
- `standalone-app-plan.md` section 4 not available; parity rows may miss capabilities.
- Some runtime fallbacks are home-relative `~/projects/...` paths; they work only where those folders exist.
- Not run: swift test (NOOP repo), live relaunch, signed build, notarization, quarantine test.

## Open gates
- Stage H: `tasks/proposed/stage-h-gates.md` (Q13.1-Q15.5: login mechanism, Developer ID, notary profile, B14, version bump, entitlements, arch, manifest host, updater key, switch, defaults, swap host).
- Decision 0032 item 7: "77 vs 7". He said "77" by voice; read as 7 days stale. Confirm.
- Stage G Q1-Q8 (`tasks/proposed/phase10-12-stage-g.md`): noop-syncd source, takeover flag, real reports committed, parity meaning, drop DEEP/REM/RHR, secrets location, legacy Telegram token migration, haptic vs audio.
- Treasury phases 3-5.
- Others: Q1 navy vs Hermes web tools; apply the Amara Voice diffs (`tasks/proposed/`); Keychain moves; chmod; Weaviate switch-over; Hermes key rotation and backup prune (last, with his OK).

## Taisun's remaining steps
1. Answer the open gates above (log in `docs/decisions/`).
2. Install the Developer ID cert, create the `nexus-notary` profile, then `scripts/release-local.sh --check` and `scripts/release-local.sh`.
3. Live-test the first signed build with Muse, Ollama and the router; Gatekeeper test on a quarantined copy.
4. Run the fresh-account migration (`docs/release/migration-test.md` Part B).
5. Pick the update host, generate the updater key (offline), approve the switch and the swap host.
6. Turn on Startup himself when Design wires the toggle.
7. Run the live NOOP parity report and the Telegram Send Test when ready.
8. Say explicitly if a push, merge or PR is wanted (frozen until then).
