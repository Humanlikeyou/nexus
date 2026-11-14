# 0029: Weaviate/Verba API key switch-over (Taisun, 2026-11-14, in session; see 0027 for his words)

Taisun: "Weaviate: log a decision first. Weaviate's anonymous access can't be turned off until n8n has its own credential, because n8n workflow eYdwlwZKTMV89z2J calls Weaviate directly."

Approved order (steps in `tasks/proposed/weaviate-switchover.md`):
0. n8n first: a Header Auth credential carrying the Weaviate key, and the `Handle kb.ingest` / `Handle kb.query` Code nodes in `eYdwlwZKTMV89z2J` send it (export updated in `stack/n8n/workflows/`). `N8N_BLOCK_ENV_ACCESS_IN_NODE=true` stays.
1. Key: generated locally, stored in `stack/.env` as `WEAVIATE_API_KEY` and in the Keychain as `weaviate_api_key` via a prompt (never argv or history).
2a. Compose: API-key auth on, anonymous still on; Verba gets `WEAVIATE_API_KEY_VERBA`. Parity snapshot before, compare after.
2b. Only after 2a's parity passes and kb.ingest / kb.query / crew kb_query work with the key: anonymous off, parity again.
Rollback: revert the compose lines and restart weaviate/verba.

Anonymous access stays on until step 2b. Each live step is run and checked by the agent in its own run, with a backup of the compose file and an n8n workflow export first.
