---
name: local-commit
description: The only way to commit in Amara OS: local, on amara-integration, explicit paths, gates green, secrets checked, never push or merge. Includes how to commit your hunk of a file that also holds Taisun's uncommitted edits.
---
# local-commit

1. `git branch --show-current` must print `amara-integration`.
2. Gates green (`run-gates`).
3. `git add path/one path/two` (explicit paths; never `-A` or `.`). Never stage BUILD.md, CLAUDE.md, NEXT_SESSION_PROMPT.md, README.md, STAGES.md, TAISUN-STEPS.md or amara-router/README.md wholesale: those carry Taisun's uncommitted edits.
   - To commit only your hunk in one of them: write your hunk as a unified diff against `HEAD` (`git show HEAD:FILE > /tmp/x; edit; diff -u`), then `git apply --cached --unidiff-zero PATCH` (or build a patch whose context doesn't overlap his hunks), and confirm with `git diff --cached FILE` that only your lines are staged.
4. `git diff --cached --stat` shows only intended files.
5. Commit as its own command: `git commit -m "type(scope): summary" -m "body + evidence" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"`. The Bash hook re-runs check:secrets.
6. Never `git push`, `git merge` into nexus-v2, or open a PR. Those are frozen (`docs/decisions/0001-merge-push-freeze.md`).
