---
paths:
  - "src-tauri/**"
  - "amara-router/**"
---
# Rust standards

- `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` for every crate touched.
- File IO in async commands uses `tokio::fs` (or `spawn_blocking`). Paths from IPC go through `safe_join` / app_paths; no `..`, no symlink escape.
- Loopback only. No new listeners or outbound hosts. Honour `NEXUS_OUTBOUND_DISABLED`.
- Never log secrets, prompt bodies or health values. String slicing must be char-boundary safe.
- Register new commands in `commands/mod.rs`, `lib.rs` `generate_handler!`, and `test/commands.registered.test.ts`; capability changes need security review.
