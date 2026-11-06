# Next Session Prompt — Copy/paste this to start the next Claude Code session

---

## Context

Amara OS is the Tauri app in this repo. Chat goes through Hermes (`127.0.0.1:8642`), and Hermes calls the Amara Router
(`127.0.0.1:8787`), which picks the model. The router, the Hermes switch-over, the Hermes route patch and the voice bridge
were applied on 2026-11-04. `TAISUN-STEPS.md` has what changed, how to verify it and how to roll it back.

Stage 4 is complete. Stage 5½ stays open until Mind-Body passes QA on live data. See `STAGES.md`.

The Terrance fold-in is scrapped, so don't plan around it. Code names such as `terrence_runtime` stay as they are.

## What to do next, in order

Check the live state before trusting this list. Update it at the end of each session.

1. **Live checks with Taisun.** Quit and reopen Amara OS. Check Mind-Body charts, the weekday in Chat, Home and End in an empty composer, Lists, the Fast/Deep badge and the Home heart rate.
   These fixes are committed but not proven in the app.
2. **Design calls.** Sidebar emerald tint or plain glass, and keep or drop the Light theme.
3. **Neurofeedback.** A real Muse session in the app, for latency, thresholds, artifacts and audio.
4. **Voice with headphones.** A live spoken reply with the lens pulsing, and smoothness on the M1 Pro. Keep the speakers quiet while Taisun is at work: read the volume first and only lower it.
5. **Telegram.** The Fujimi teacher question through the vault tools.
6. **Calendar.** The token expired and needs Taisun's login.
7. **Backup prune.** Follow `~/Backups/PRUNE-PLAN-2026-11-06.md`, one line at a time with Taisun's OK. The two newest snapshot sets wait until 2026-11-19 09:28 JST.
8. **Home control.** Needs home Wi-Fi and the Wi-Fi devices. Docker has no discovery, Bluetooth or USB. This is the largest step left before 90%.
9. **Last, in order.** Muse eyes-closed test (`cd src-tauri && cargo run --example muse_probe -- eyes`). `AUX_PROVEN` stays false until it passes. Then key rotation (`stack/ROTATION-RUNBOOK.md`), then the push near 90%.

Open decisions for Taisun: stay on Letta 0.16.8 or migrate, and keep Tauri packages on ~2.11 or raise them.

Don't ask about pushing before 90%. OpenClaw is undecided, so leave it alone.

## Key file locations

| File | Purpose |
|------|---------|
| `TAISUN-STEPS.md` | Taisun's steps: verify and roll back the router, Hermes, the voice bridge and the route patch |
| `amara-router/README.md` | Router install, config and client setup |
| `~/projects/hermes-patches/README.md` | Hermes route patch: apply, revert, what updates can break |
| `CLAUDE.md` | Architecture and runtime behaviour |
| `STAGES.md` | Project roadmap |
| `stack/kb/INGEST-PLAN.md` | Knowledge base ingest plan |
