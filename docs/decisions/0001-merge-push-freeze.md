# 0001: Merges and pushes are frozen

- **Date:** 2026-11-10 (Taisun's goal prompt), recorded 2026-11-11.
- **Decided by:** Taisun.
- **Decision:** no `git push`, no merge into `nexus-v2`, no PR without his explicit OK for that specific action, logged here. Commit locally on `amara-integration` only.
- **Why:** a prior session made 23 merges into nexus-v2 and 8 pushes (4–5 Nov) with no approval. Until he answers whether those were approved, all further merges and pushes are frozen.
- **Supersedes:** "Commit on amara-integration, then `git merge --no-ff` into nexus-v2" in CLAUDE.md / README.md and the 11-11 session notes.
- **Enforced by:** `.claude/hooks/guard-bash.sh` (PreToolUse Bash) and `permissions.deny` in `.claude/settings.json`.
