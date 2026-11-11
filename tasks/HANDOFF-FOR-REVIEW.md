# Handoff for review — Claude Code, 2026-11-11

For Nexus Lead and Nexus Design Lead. Branch `amara-integration` in `~/projects/nexus-amara`, local commits only. **Nothing pushed, merged or PR'd** (`docs/decisions/0001`). Start: `cd5e4cc`. Design's own commits that landed during the session (never touched): `11a9d6d` 1110g, `d1f2168` 1110f, `c1fcd9a` 1110i, `046eedc` 1111a, `3ca4ea2` 1111r. No conflicts with them; 1110f's card uses this session's `useAmaraTake` hook as intended.

Taisun's 7 uncommitted doc edits (BUILD, CLAUDE, NEXT_SESSION_PROMPT, README, STAGES, TAISUN-STEPS, amara-router/README) are **still uncommitted and intact**. Two of my commits add a hunk to CLAUDE.md / TAISUN-STEPS.md via index-only patches; his lines stay unstaged.

## 1. Dev system (what was built)

```
.claude/
  settings.json            env NEXUS_OUTBOUND_DISABLED=1; permissions.deny (push, PRs, reset --hard, launchctl load, ~/.hermes, memory/, Muse folders); PreToolUse Bash hook
  hooks/guard-bash.sh      blocks push / merge into nexus-v2 / PRs / force & destructive git / add -A / rm -r / launchctl / ~/.hermes writes / any non-read command naming replays|sessions|muse; runs check:secrets before commits
  hooks/test-guard.py      13 regression cases (all ok)
  agents/                  architect (opus, plan only) · rust-executor (sonnet) · frontend-executor (sonnet/haiku) · researcher (haiku, read-only) · qa-reviewer (sonnet) · security-reviewer (sonnet)
  rules/                   hard-rules · git-and-commits · verification · design-boundary (path-scoped) · muse-recordings (path-scoped) · rust · frontend
  skills/                  run-gates (gates.sh) · task-cycle · local-commit · skill-improvement
docs/decisions/            0001 merge/push freeze · 0002 context management · 0003–0005 Taisun-delegated calls
docs/lessons-learned/      verified lessons only
tasks/                     BOARD.md · TEMPLATE.md · active/ · done/ · blocked.md · design-questions.md · skill-proposals/ · proposed/ · this file
```

- **Orchestration:** the main session plans, writes `tasks/active/<id>.md`, delegates to the cheapest capable agent in non-overlapping lanes, reviews each result itself, sends security-surface diffs to the security reviewer, fixes, runs gates, and is the only committer (explicit paths, one commit at a time).
- **Verification:** `.claude/skills/run-gates/gates.sh` (tsc, lint, vitest, neuro, check:secrets, fmt / clippy -D warnings / test for both crates). Fail-before / pass-after tests per fix. Counts never dropped.
- **Context management:** native auto-compaction at default; the 60% threshold was evaluated, and no documented setting exists, so none was added (`docs/decisions/0002`). Continuity lives in `tasks/BOARD.md`, task files and decisions.
- **Skill improvement:** `.claude/skills/skill-improvement` covers proposal → cause → verify → approve → apply. Hard rules, hooks, settings and agent tools need Taisun's logged OK. No proposals were made this session.
- **Known limits:** project agents load at session start, so this session ran them as general-purpose agents that read the agent files. The guard hook is pattern-based defence in depth (4 automated reviews hardened it); a real backstop (pre-push hook / `chflags`) is Taisun's call (TAISUN-STEPS §9.8).

## 2. Backlog status (section 11)

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | Deep eviction / gpu_budget_gb | done `bb6669a` | grace + headroom tests fail before |
| 2 | auto_deep default false + classifier gate | done `bb6669a` | 3 tests fail before |
| 3 | Router stability | done `bb6669a` (documented) | `amara-router/STABILITY.md`; −9 = SIGKILL, cause **not proven** |
| 4 | fast_num_ctx docs | done, no change needed | all committed docs already say 65536 |
| 5 | allow_remote_ollama + bearer when cloud on | done `bb6669a` | tests; /health exempt |
| 6 | Hermes 8642 from Docker | done `b869bf0` | containers reach it, get 401; key 24 bytes → rotation batch |
| 7 | gemma_tools vault | done `bb6669a` | followups-only `[a-z0-9_]` allowlist |
| 8 | followups IPC gate + async | done `7a1f47e`, `8b0456f` | 7 writes behind native dialog; dialog text can't be forged |
| 9 | strip_phrase char-safe | done `bb6669a` | panicked before on İ/ẞ |
| 10 | model_loading bubbles | done `7a1f47e` | Rust + vitest fail before |
| 11 | HR window seconds | done `7a1f47e` | |
| 12 | useLiveHeartRate NaN / hidden pause | already in HEAD | verified by temporary break (2 tests fail) |
| 13 | Pin tauri-action | done `9609615` | 84b9d35 = v0.6.2 |
| 14 | Fail release without P8 | done `9609615` | test 2 fail before → 4 pass; secret-name step in TAISUN-STEPS §9.2 |
| 15 | Reduced-transparency e2e | done `47e95a1` | CSS break → 6 fail; restored |
| 16 | Unit tests ×5 | done `47e95a1` | 33 tests |
| 17 | body_context_capture on Linux | done `7a1f47e` | launcher injected; **not run on Linux** |
| 18 | Voice docs | no change | verified 11-11: LaunchAgent loaded, 8791 listening |
| 19 | README nav names | proposed patch `bb2d574` | `git apply tasks/proposed/README-nav.patch` (his uncommitted README) |
| 20 | Hermes patch versioning | done | patch == live edits file-for-file; pytest step §9.3 |
| — | route-motion flakes | done `47e95a1` | real focus bug; 79/80 → 80/80 ×20 |

## 3. Wiring (section 12)

- **12a:** Muse per-sample + score scale **done** `a34a224` (`get_session_samples`, `score_scale=100`). Awake / bed-wake / resting HR: **data already loaded** (Design uses it in 1110h). Workouts (`workout` table, 245 rows, incl. `zonesJSON`) and hypnogram (`sleepSession.stagesJSON` segments, 87 nights) — **sources found, commands not started**. Nutrition — **no source** (gap). Voice stop-speaking — **not started**: needs an amara-voice patch (only SIGTERM exists today) plus a Nexus command.
- **12b:** Business ledger **done** `8b6265e` (decision 0005; no file exists yet, so the app still shows Needs wiring). Crews approvals / steps / handoffs / runMetrics / n8n / roster / nextSchedule — **blocked on 13 B/D**.
- **12c:** chunks A+B (record all streams, read-only readers, retention computed, no delete path) **done** `a34a224`. Chunk C (frontend cache + adapter + AUX detector) **not started**. `removeConfirmed` stays unwired.
- **12d:** **done** `8d96417`, `534d19f` (local-only; rejects invented numbers).

## 4. Section 13 (local multi-agent runtime)

Phase A **done** `5f4eca9` (crews → router, URL allowlist, MiniMax fail-closed). B–F **not started**.

## 5. Gates at last run (HEAD `5f4eca9`)

vitest 1290 passed / 9 skipped (baseline 1140) · neuro 21 · src-tauri 466 / 5 ignored (418) · amara-router 125 (112) · crew-service 56 (Docker test image) · tsc, lint, fmt, clippy -D warnings, check:secrets clean.
**Not run:** full e2e suite (only touched specs), WebKit (not configured), Linux CI, anything live (router not rebuilt, crew container not rebuilt, app not relaunched, no headset). Commit `a34a224` was staged as a split of shared files and not separately built (the next commit was).

## 6. Questions for Taisun

1. Were the 8 pushes / 23 merges (4–5 Nov) approved? (Freeze stays.)
2. Commit or discard the 7 uncommitted docs?
3. Delete `/tmp/rev1110*` (4.5 GB)? Letta CVE, duplicate n8n workflows, stale containers?
4. May the confirm-only recording delete path exist at all?
5. `save_session` overwrites a session JSON on a duplicate id (pre-existing): refuse instead?
6. Follow-ups: one dialog per edit OK, or one per session / app-side copy?
7. Router grace 60 s OK? Cloud on now needs the router bearer on loopback (Hermes / Nexus would send it).
8. Crew runtime open questions (design doc §12): executor tools, approver, retry limit, cost column, SOUL.md access, MiniMax removal, concurrency 2→1, chat vs crew contention, where skill proposals live. 1110i's per-crew Files/terminal switch depends on the executor-tools answer.
9. Then the NEXT_SESSION_PROMPT order: home control, real Muse session, voice with headphones, slow first Chat turn, calendar, eyes-closed test, key rotation, backup prune, push.

## 7. Design questions (tasks/design-questions.md)

- Lists copy for a declined / busy follow-up approval.
- Ledger as-of date on the Money tab: `ledgerAsOf()` in `business/liveLedger.ts` is ready to show.
- New since the prompt: 1111r "Effort zone minutes" can read `workout.zonesJSON`; 1111a personal-OS adapters; 1110i `crew_tool_grant_*` — all need backend work, none started.

## 8. Next steps (in order)

12c chunk C → 12a workouts + hypnogram commands → voice stop-speaking (amara-voice patch + command) → 13 B (steps / handoffs) → 13 D (crew approvals; unblocks 12b Crews) → 13 C / E / F.
