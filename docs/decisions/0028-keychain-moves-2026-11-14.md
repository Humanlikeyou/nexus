# 0028: Keychain moves (Taisun, 2026-11-14, in session; see 0027 for his words)

Taisun: "Keychain moves: log each move you want in docs/decisions/."

Moves logged (copy file → Keychain under `com.taisun.nexus.<name>`; the file copy STAYS, removing it is a separate later decision):

| Name | From | Reader after the move |
|---|---|---|
| `ha_token` | `<data_dir>/.env` `HA_TOKEN` line | `ha.rs` Keychain first, `.env` fallback |
| `n8n_api_key` | `<data_dir>/n8n_api_key` | `n8n_status.rs` Keychain first, file fallback |
| `crew_service_token` | `<data_dir>/crew_service_token` | `crew.rs` env, then Keychain, then file |

Not moved: `~/.hermes/.env` (never touched), `~/projects/nexus/stack/.env` and `.env.crew` (compose needs env), the router's own Keychain/secrets file (already Keychain-first).

How: only through `keychain_migrate_one` from a click in the app, with the native approval dialog. `KEYCHAIN_MIGRATE_ENABLED` is flipped to true in the same commit that adds the Settings button, after Design answers the 2026-11-14 D4 design question. No shell copy of secret values (it would put them in argv or history).
