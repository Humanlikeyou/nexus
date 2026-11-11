# Git and commits

- Branch `amara-integration` in `~/projects/nexus-amara` only. `~/projects/nexus` is read-only for agents.
- The **orchestrator is the only committer**: one commit at a time, `git add <explicit paths>`, never `git add -A` / `.`. Subagents never commit.
- Before each commit: all gates in `verification.md` pass, and `git diff --cached --stat` shows only intended files.
- The 7 uncommitted doc edits (BUILD.md, CLAUDE.md, NEXT_SESSION_PROMPT.md, README.md, STAGES.md, TAISUN-STEPS.md, amara-router/README.md) are Taisun's decision. Don't commit, discard or overwrite them. To land your own hunk in one of those files, stage only that hunk (`git diff` → hand-built patch → `git apply --cached`), and keep his lines unstaged.
- Untracked `tasks/claude-goal-prompt-1111.md` and `docs/architecture/local-multi-agent-design.md` are Taisun's; don't commit them without his OK.
- Commit message: `type(scope): summary` + body with test evidence, ending with the Co-Authored-By line.
- Checkpoint (a local commit) before any substantial change. Use a WIP commit, not `git stash` (shared stash stack).
- The guard hook is fail-safe: a command that merely *mentions* `git … push`, or `git merge … nexus-v2`, is blocked (for example a commit message containing the word "push"). Reword the message; do not weaken the hook.
