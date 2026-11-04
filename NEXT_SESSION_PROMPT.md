# Next Session Prompt — Copy/paste this to start the next Claude Code session

---

## Context

Amara OS is the Tauri app in this repo. Chat goes through Hermes (`127.0.0.1:8642`), and Hermes calls the Amara Router
(`127.0.0.1:8787`), which picks the model. The router, the Hermes switch-over, the Hermes route patch and the voice bridge
were applied on 2026-11-04. `TAISUN-STEPS.md` has what changed, how to verify it and how to roll it back.

Stage 4 is complete except for RAG content. Stage 5½ stays open until Mind-Body passes QA on live data. See `STAGES.md`.

The Terrance fold-in is scrapped, so don't plan around it. Code names such as `terrence_runtime` stay as they are.

## What to do next, in order

1. **Full gate and rebuild.** Run `npm test`, then `npm run bundle:local`. The bundle lands in `src-tauri/target/release/bundle/`.
   Replace the installed `/Applications/Amara OS.app`.
2. **Voice end to end.** Start Amara Voice from Chat & Agents › Voice. Check one spoken turn through the router and Hermes
   (`~/Library/Logs/amara-router/routes.jsonl` gets a line) and check that the Amara lens follows state and level.
   While the app is open, check that a Chat reply shows the right Fast/Deep badge. That check hasn't been done yet.
3. **Mind-Body QA on live NOOP data.** Test `src-console/screens/MindBodyScreen.tsx` against the real store. Stage 5½ closes
   when this passes.
4. **Knowledge base: done 2026-11-04.** Verba holds the 8 current docs (72 chunks). Weaviate goes read-only at 90% disk, so watch free space. Re-run `scripts/kb-ingest.sh --apply` after doc changes; it doesn't de-duplicate, so remove the old copies afterwards.
5. **Verba and Crew Auth: done, no action needed.** Verba already runs `gemma4:latest`. Live n8n's Crew Auth logic matches the export, and its hash matches the current crew key.
6. **Home control, last.** This is deliberately the final step. Don't start it before steps 1 to 5 are done.

## Key file locations

| File | Purpose |
|------|---------|
| `TAISUN-STEPS.md` | Taisun's steps: verify and roll back the router, Hermes, the voice bridge and the route patch |
| `amara-router/README.md` | Router install, config and client setup |
| `~/projects/hermes-patches/README.md` | Hermes route patch: apply, revert, what updates can break |
| `CLAUDE.md` | Architecture and runtime behaviour |
| `STAGES.md` | Project roadmap |
| `stack/kb/INGEST-PLAN.md` | Knowledge base ingest plan |
