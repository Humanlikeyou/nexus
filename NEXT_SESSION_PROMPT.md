# Next Session Prompt — Copy/paste this to start the next Claude Code session

---

## Context

Stage 4 (Activation) for the Amara OS project was just implemented and committed (`0db686f`). The production app was deployed to `/Applications/Electron.app` at the time (pre-rename; the app is now `Nexus`, `com.taisun.nexus`). Four features were added:

1. **Automatic Memory Capture** — TelegramChat.tsx has a 2-minute idle timer that auto-saves sessions with 4+ messages. Coordinates with the manual "Save to memory" button.
2. **Scheduled Memory Consolidation** — `src/terrance/system/scheduler.ts` runs every 6 hours, consolidates last 24h session logs into a single Letta memory write via n8n ingress.
3. **Autonomous Reasoning Loops** — Every 12 hours, Amara reviews goals vs recent sessions, generates a reflection stored to session logs + Letta.
4. **Gateway Authentication** — n8n ingress workflow validates `X-Nexus-Secret` header against `NEXUS_SECRET` env var. Open mode when unconfigured.

## Outstanding Issue

After deploying Stage 4, the production app shows a crash on the Amara page:
```
Something went wrong
This page encountered an error. Your data is safe.
undefined is not an object (evaluating 'e.length')
```

Defensive `?? []` null guards were added on all `.length` accesses and a new build was deployed. **The user has not yet confirmed if the fix works.** If it still crashes:

1. Run `cd /Users/thevisualdude/projects/nexus && npm run tauri -- dev`
2. Open the WebKit inspector (Cmd+Option+I in the Tauri window)
3. Check the console for the **unminified** error with full stack trace
4. The error is `e.length` in minified code — the stack trace will show the exact file and line
5. The most likely culprits are array fields from Rust deserialization being `null` instead of `[]`, or a component prop receiving `undefined`

Key context for debugging:
- Rust structs use `#[serde(rename_all = "camelCase")]` — all JSON from Tauri commands returns camelCase keys
- The `types.ts` file previously had snake_case field names for `TerranceContext` and `SessionLogEntry` — these were corrected to camelCase in this session
- The `ChatSession` type in `types.ts` still uses snake_case (`created_at`, `message_count`) — check if the Rust `ChatSession` struct also uses snake_case or if there's a mismatch

## What to do next

1. **Verify the crash is fixed** — Launch the app, confirm the Amara page loads without errors
2. **Test auto-save** — Send 4+ messages to Amara, wait 2 minutes, check `memory/session_logs/` for a new JSON file
3. **Test gateway auth** — Set `NEXUS_SECRET` in `stack/.env`, restart n8n (`docker compose restart n8n`), set the same secret in Nexus Settings > Webhook Secret, verify requests work with header and fail without
4. **Test consolidation** — Temporarily lower the interval in `scheduler.ts` (e.g., 30 seconds), verify Letta memory updated: `curl -s http://localhost:8283/v1/agents/agent-5efd68f1-6976-4af5-a677-b82f668d6843/memory`
5. **Test reasoning** — Same approach, check session logs for `reasoning-*` entries
6. **If all verified** — Begin Stage 5 planning (Autonomy: CrewAI crews, proactive summaries). See `STAGES.md` for the roadmap.

## Key file locations

| File | Purpose |
|------|---------|
| `src/terrance/system/scheduler.ts` | Consolidation + reasoning scheduler |
| `src/terrance/system/continuity.ts` | All memory functions (auto-save, consolidation, reasoning) |
| `src/components/terrance/TelegramChat.tsx` | Chat UI with idle timer auto-save |
| `src-tauri/src/commands/continuity.rs` | Rust context store + session logs |
| `stack/integrations/n8n/terrance_ingress_workflow.json` | n8n gateway with auth validation |
| `stack/docker-compose.yml` | NEXUS_SECRET env var for n8n |
| `STAGES.md` | Full project roadmap |
