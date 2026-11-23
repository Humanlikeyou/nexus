# Verification gates (all must pass before a commit)

Run `.claude/skills/run-gates/gates.sh` (or the individual commands):

| Gate | Command |
|---|---|
| Types | `npx tsc --noEmit` |
| Lint | `npm run lint` |
| Unit | `npm run test:unit` (vitest) and `npm run test:neuro` |
| Secrets | `npm run check:secrets` |
| Rust | for `src-tauri` and `amara-router`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` |
| e2e (UI-affecting changes) | `npm run test:e2e` |

**Baseline (cd5e4cc, 2026-11-11):** vitest 1140 passed / 9 skipped; test:neuro 21; src-tauri 418 passed / 5 ignored; amara-router 112; fmt + clippy clean. Counts must not drop without a written reason in the task file.

Rules:
- Bug fix ⇒ a test that fails before and passes after (record both runs in the task file).
- Never loosen tolerances, gate tests by platform, or add always-passing mocks to go green. Flaky e2e (`route-motion.spec.ts:78,:100`) gets a deterministic wait on the transition end, not a longer timeout.
- Pre-existing axe hits (training calm ring, Delta/Gamma meters): fix only without visual change; otherwise flag for Design Lead.
- 3 attempts per task, then `tasks/blocked.md`.
- Report anything not run as **not run**. Never fabricate.
