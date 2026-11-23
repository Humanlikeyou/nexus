---
name: task-cycle
description: The goal-driven task lifecycle for Amara OS (PLAN → DELEGATE → EXECUTE → TEST → REVIEW → FIX → VERIFY → DOCUMENT). Use when starting, delegating, finishing or escalating any substantial task.
---
# task-cycle

**Open:** copy `tasks/TEMPLATE.md` to `tasks/active/<id>.md` (ids: `B01`–`B20` backlog, `W12a-…` wiring, `R13-A…` runtime). Fill objective, deliverables, acceptance criteria, plan and verification before writing code. Update `tasks/BOARD.md`.

**Delegate** only when it saves effort: a single-file fix is done inline. Pick the cheapest capable agent (`.claude/agents/`). The prompt names the task file, the exact files, the acceptance criteria, and "don't commit".

**Test:** a failing test first (record the failing output), then the fix, then the passing run.

**Review:** `qa-reviewer` for substantial/risky diffs; `security-reviewer` when the diff touches a surface listed in its description. Executors never review their own work.

**Fix:** return with a numbered list. Max 3 attempts, then add the item to `tasks/blocked.md` and move on.

**Verify:** `run-gates` skill; paste the result lines.

**Document + commit** (orchestrator only, `local-commit` skill). Move the file to `tasks/done/` with the commit hash and evidence. Lessons proven by this task go to `docs/lessons-learned/`.
