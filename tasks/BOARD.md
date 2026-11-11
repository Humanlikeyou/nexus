# Board (read this first when resuming)

Baseline at cd5e4cc: vitest 1140/9 skipped · neuro 21 · src-tauri 418/5 ignored · router 112 · fmt/clippy clean.
Status: `todo` · `active` · `done <hash>` · `blocked` (see blocked.md) · `waiting` (Taisun).

## 0. Dev system
| id | item | status |
|---|---|---|
| S00 | .claude/ agents, rules, hooks, skills; docs/decisions; tasks/ | done (this commit) |

## 11. Fix backlog (priority order)
| id | P | item | status |
|---|---|---|---|
| B01 | P1 | Router: stop evicting deep in restore_fast; grace/VRAM headroom via gpu_budget_gb; re-warm tests | todo |
| B02 | P1 | Router: auto_deep default false; gate classifier hard→deep paths | todo |
| B03 | P1 | Router stability (exit -9, "no first token", "fast stayed busy") | todo |
| B04 | P1 | Docs: fast_num_ctx is 65536 | todo |
| B05 | P1 | Router config: allow_remote_ollama; refuse anthropic_enabled without loopback bearer | todo |
| B06 | P1 | Hermes 8642 reachable from Docker? API_SERVER_KEY? → SECURITY-NOTES | todo |
| B07 | P1 | gemma_tools recovery: allowlist excludes vault/credential tools | todo |
| B08 | P1 | followups.rs ~/.hermes writes: approval gate + async IO; decision for Taisun | todo |
| B09 | P1 | strip_phrase char-boundary safe | todo |
| B10 | P2 | Don't persist/render model_loading or refused failures | todo |
| B11 | P2 | noop.rs live HR window seconds constant | todo |
| B12 | P2 | useLiveHeartRate NaN guard + pause when hidden | todo |
| B13 | P2 | Pin tauri-action to a SHA | todo |
| B14 | P2 | Fail release on APPLE_API_KEY without APPLE_API_KEY_P8 + TAISUN step for secret name | todo |
| B15 | P2 | Reduced-transparency e2e reads real app state | todo |
| B16 | P2 | Unit tests: NeuroFlow, LandingScreen, useRouteNavigation, MindBodyCharts, Quiet | todo |
| B17 | P2 | body_context_capture test runs on Linux CI | todo |
| B18 | P3 | Docs: Amara Voice not running; TAISUN step to load it | todo |
| B19 | P3 | README nav: Neuro history, Today, "Mind–Body", Agents | todo |
| B20 | P3 | Hermes route passthrough → ~/projects/hermes-patches + re-apply step | todo |
| B21 | — | route-motion e2e flakes (deterministic wait) | todo |

## 12. Wiring
| id | item | status |
|---|---|---|
| W12a | Health leftovers: Muse samples, score scale, voice stop-speaking, night awake/bed/wake, resting HR, gaps | todo |
| W12b | Crews + Business live adapters | todo |
| W12c | Muse recordings: record all streams, window/list/series/contact/body/liveAux, retention compute (no delete) | todo |
| W12d | Amara's take backend + useAmaraTake | todo |

## 13. Local multi-agent runtime (after 11 + 12)
| id | item | status |
|---|---|---|
| R13-A…F | Phases A–F | todo |
