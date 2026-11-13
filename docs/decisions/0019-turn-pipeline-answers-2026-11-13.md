# 0019: Turn pipeline answers (Taisun, 2026-11-13, in session)

Plan: `tasks/plans/turn-pipeline.md` (decision 0016 #2–3).

His answers:
1. **Claude on typed chat: "Never".** Router-direct typed turns always send `X-Amara-Allow-Claude: 0` (local tiers only), whatever the saved mode. Claude keeps only its existing opt-in uses (e.g. /deliberate, behind Allow Claude).
2. **Hermes for tools only, on these words: "Vault / notes, Follow-ups, Web lookups".**
   - Vault/notes: vault, note(s), Obsidian, "in my notes".
   - Follow-ups: item_done, mute_topic phrasing.
   - Web lookups: search, look up, latest, news.
   Everything else goes router-direct and streamed. Web lookups through Hermes stay blocked by navy local-only and NEXUS_OUTBOUND_DISABLED as today.

Settled by the assistant from facts (plan §10):
- Q3: 15-min ceiling on direct turns; Stop ends any turn.
- Q4: window close/focus/undo stay in the renderer.
- Q5: navy stays new-text-only.
- Q6: live Letta has one agent (checked read-only 2026-11-13).

Still frozen (0001): merge into `nexus-v2`, push, PR.
