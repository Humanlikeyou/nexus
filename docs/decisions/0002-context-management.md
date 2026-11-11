# 0002: Context management uses Claude Code's native auto-compaction

- **Date:** 2026-11-11. **Decided by:** orchestrator (Claude Code), within Taisun's prompt §5.
- **Decision:** keep native automatic compaction at its default threshold. No custom threshold is configured in `.claude/settings.json`.
- **60% threshold:** evaluated. There is no documented `settings.json` key for the compaction threshold, so none was added (the prompt forbids unsupported options). An environment-variable override exists in some Claude Code versions but isn't a documented setting; revisit only if it becomes one.
- **What survives compaction:** `CLAUDE.md` + `.claude/rules/*.md` are reloaded every session; `tasks/BOARD.md` and the active task files hold the unfinished work; this folder holds decisions. Keep task files short so resuming costs little.
- **Load on demand:** big docs (`docs/architecture/local-multi-agent-design.md`, STAGES.md, BUILD.md) are read only when a task needs them; path-scoped rules (`paths:` frontmatter) load only for matching files.
