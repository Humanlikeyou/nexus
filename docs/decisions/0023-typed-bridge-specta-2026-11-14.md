# 0023: Typed bridge with tauri-specta (Taisun, 2026-11-14, in session)

Plan: `tasks/proposed/typed-bridge.md` (Stage B). His words: **"approve specta"**.

Approved, as listed in the plan's "Decision needed" section:
1. `tauri-specta = "=2.0.0-rc.29"` (features `derive`, `typescript`) and its `tauri-specta-macros =2.0.0-rc.29`.
2. `specta = "=2.0.0-rc.28"` (feature `derive`).
3. `specta-typescript = "=0.0.15"`, plus the transitive crates it pulls in (`specta-serde` / `specta-util`, `specta-macros`, and any of heck/paste/thiserror not already in Cargo.lock).
4. Enabling the `specta` feature on the existing `tauri` dependency.
5. npm: none.
6. One online `cargo fetch` (crates.io only) so the `--offline` builds can resolve them. Nothing else is downloaded.

Conditions (assistant, from the plan's risks):
- Versions are pinned exactly.
- Commit 1 must prove that rc.29 compiles and exports against crates.io tauri 2.11.6 for 5 low-risk commands plus one with State/AppHandle. If it doesn't, stop and report, and don't patch tauri to a git fork.
- 64-bit integers: each BigInt cast decision is logged in the commit that makes it.
- No behaviour change: bridge.ts keeps its exported functions, which call the generated bindings.

Still frozen (0001): merge into `nexus-v2`, push, PR.
