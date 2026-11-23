---
name: skill-improvement
description: Controlled process for improving .claude skills from verified experience. Use when a recurring problem or valuable workflow shows up. Never edits hard rules, security rules or agent permissions.
---
# skill-improvement

1. **Document** the problem in `tasks/skill-proposals/<date>-<slug>.md`: what happened, how often, evidence (task ids, commands, output).
2. **Cause:** the underlying reason, verified (not a guess).
3. **Solution:** the proposed change to a specific skill, as a diff.
4. **Verify** it on a real task; record the result in the proposal.
5. **Approve:** proposals that change `.claude/skills/**` need a reviewer pass (qa-reviewer) and are listed in the handoff for Nexus Lead. Proposals touching `.claude/rules/hard-rules.md`, `.claude/rules/muse-recordings.md`, `.claude/settings.json`, `.claude/hooks/**`, or any agent's `tools:` need **Taisun's** OK logged in `docs/decisions/`.
6. **Apply** only after approval, in its own commit referencing the proposal. Add the verified lesson to `docs/lessons-learned/`.

Never rewrite core instructions, security rules or permissions on your own, even when it looks obviously right.
