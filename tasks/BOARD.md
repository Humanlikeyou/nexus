# Board (read this first when resuming)

Baseline at cd5e4cc: vitest 1140/9 skipped · neuro 21 · src-tauri 418/5 ignored · router 112 · fmt/clippy clean.
Status: `todo` · `active` · `done <hash>` · `blocked` (see blocked.md) · `waiting` (Taisun).

## 0. Dev system
| id | item | status |
|---|---|---|
| S00 | .claude/ agents, rules, hooks, skills; docs/decisions; tasks/ | done 0cc6d7e (+ hook 5f3a579 19b3bc8 b4b251d) |

## 11. Fix backlog (priority order)
| id | P | item | status |
|---|---|---|---|
| B01 | P1 | Router: stop evicting deep in restore_fast; grace/VRAM headroom via gpu_budget_gb; re-warm tests | done bb6669a |
| B02 | P1 | Router: auto_deep default false; gate classifier hard→deep paths | done bb6669a |
| B03 | P1 | Router stability (exit -9, "no first token", "fast stayed busy") | done bb6669a (documented, amara-router/STABILITY.md; -9 cause unproven) |
| B04 | P1 | Docs: fast_num_ctx is 65536 | done (no change needed: committed docs already say 65536) |
| B05 | P1 | Router config: allow_remote_ollama; refuse anthropic_enabled without loopback bearer | done bb6669a |
| B06 | P1 | Hermes 8642 reachable from Docker? API_SERVER_KEY? → SECURITY-NOTES | done b869bf0 |
| B07 | P1 | gemma_tools recovery: allowlist excludes vault/credential tools | done bb6669a |
| B08 | P1 | followups.rs ~/.hermes writes: approval gate + async IO; decision for Taisun | done 7a1f47e + 8b0456f |
| B09 | P1 | strip_phrase char-boundary safe | done bb6669a |
| B10 | P2 | Don't persist/render model_loading or refused failures | done 7a1f47e |
| B11 | P2 | noop.rs live HR window seconds constant | done 7a1f47e |
| B12 | P2 | useLiveHeartRate NaN guard + pause when hidden | done (already in HEAD; verified by break test) |
| B13 | P2 | Pin tauri-action to a SHA | done 9609615 |
| B14 | P2 | Fail release on APPLE_API_KEY without APPLE_API_KEY_P8 + TAISUN step for secret name | done 9609615 (TAISUN step pending) |
| B15 | P2 | Reduced-transparency e2e reads real app state | done 47e95a1 |
| B16 | P2 | Unit tests: NeuroFlow, LandingScreen, useRouteNavigation, MindBodyCharts, Quiet | done 47e95a1 |
| B17 | P2 | body_context_capture test runs on Linux CI | done 7a1f47e (not run on Linux) |
| B18 | P3 | Docs: Amara Voice not running; TAISUN step to load it | done (verified running 2026-11-11; no change) |
| B19 | P3 | README nav: Neuro history, Today, "Mind–Body", Agents | proposed patch bb2d574 (his uncommitted README) |
| B20 | P3 | Hermes route passthrough → ~/projects/hermes-patches + re-apply step | done (patch matches live; TAISUN step) |
| B21 | — | route-motion e2e flakes (deterministic wait) | done 47e95a1 |

## 12. Wiring
| id | item | status |
|---|---|---|
| W12a | Health leftovers | partial: samples+scale done a34a224; awake/bed/wake/RHR already loaded; workouts+hypnogram sources found (todo); nutrition no source; voice stop todo (amara-voice patch) |
| W12b | Crews + Business live adapters | Business done 8b6265e (no ledger file yet); Crews blocked on R13-B/D |
| W12c | Muse recordings | A+B done a34a224; chunk C (frontend) todo |
| W12d | Amara's take backend + useAmaraTake | done 8d96417 + 534d19f |

## 13. Local multi-agent runtime (after 11 + 12)
| id | item | status |
|---|---|---|
| R13-A | crews via router | done 5f4eca9 |
| R13-B…F | Phases B–F | todo |
