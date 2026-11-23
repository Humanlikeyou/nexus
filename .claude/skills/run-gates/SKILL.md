---
name: run-gates
description: Run all Amara OS pre-commit gates (tsc, lint, vitest, neuro, check:secrets, cargo fmt/clippy -D warnings/test for src-tauri and amara-router) and compare counts with the baseline. Use before every commit and when verifying a task.
---
# run-gates

1. `.claude/skills/run-gates/gates.sh` (full) or `--quick` (skips Rust crates the diff doesn't touch; never use --quick for the final pre-handoff run).
2. Compare with the baseline in `.claude/rules/verification.md`. A count drop needs a written reason in the task file.
3. Paste the PASS/FAIL lines into the task file's Evidence section. If a gate fails, fix it; don't commit.
4. UI-affecting changes also need `npm run test:e2e` (or the relevant `npx playwright test SPEC`). Report e2e as "not run" if you didn't run it.
