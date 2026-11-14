# 0031: Phase 9 approvals (Taisun, 2026-11-15, in session)

Question: the four Stage F asks (handoff §25): bundle the Ollama runtime, a model download host, the tier RAM thresholds, Ollama's own network access.

His words: **"Approved 1-4 up to you. Go"**

The agent's choices under that OK:

1. **Ollama runtime bundled by copying it from the installed Ollama.app.** Source `/Applications/Ollama.app/Contents/Resources/` at Ollama **0.40.14**: the `ollama` binary (universal x86_64 + arm64, signed by team 3MU9H2V9Y9, SHA-256 `1e293ef2336e6f6be84e005ed539c6d2f3176d66b6c13350126f14fe0182024c`) plus its sibling runtime libraries (libggml-*, Metal and CPU backends). The files sit in a git-ignored `src-tauri/binaries/` folder, a build step copies them into the app bundle, and nothing large is committed. MIT licence: the licence files are copied too. It is re-signed with the Developer ID in Phase 14.
2. **Download host: `registry.ollama.ai:443`, used by the bundled runtime's own `/api/pull`.** Our raw-file downloader keeps its allow-list empty, because an Ollama model is several blobs plus a manifest and the registry redirects to a CDN, which only Ollama's client handles. A pull is allowed only from the first-run click, with outbound and navy checks, a native approval, and an afterwards check that each model's digest matches the pinned manifest (digests taken from the models already installed on this Mac).
3. **Tier thresholds confirmed:** compact 8 GB (fast + embed), standard 32 GB (fast + deep + embed), large 64 GB (same models for now). Names unchanged (0030).
4. **Ollama egress containment:** the long-running bundled `ollama serve` runs under a `sandbox-exec` profile that denies outbound network except loopback. A model pull runs in a separate short-lived, unsandboxed `ollama serve` on another loopback port, sharing the same model folder, started only for an approved first-run pull and stopped when it ends. `SUPERVISOR_ACT_ENABLED` stays false until the bundled runtime passes the parity check (Taisun, plan §6).
