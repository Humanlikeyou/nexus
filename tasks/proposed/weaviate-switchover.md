# Weaviate API-key switch-over (D8) [GATE: Taisun logs it in docs/decisions/ first]

Status 2026-11-14: app side built and tested (default behaviour unchanged). Nothing below has been run on the live stack.
Rule: values live in `~/projects/nexus/stack/.env` and the Keychain only. The assistant never reads `.env`.

## What is already in the app (no action needed)
- `weaviate_api_key` is in `keychain::SECRETS`, so `secret_set` can store it (write-only; shows in Settings > keys once Design lists it).
- `kb_status` sends `Authorization: Bearer <key>` to Weaviate (:8080) only, only to a loopback base, only when the Keychain item exists. No item: anonymous, exactly as before. Verba (:8001) never gets it (Verba's HTTP API takes no key; it uses `WEAVIATE_API_KEY_VERBA` to reach Weaviate).

## Step 0. BLOCKER: n8n also calls Weaviate directly (outside the app)
Workflow `eYdwlwZKTMV89z2J` (repo copy `stack/n8n/workflows/eYdwlwZKTMV89z2J.json`), Code nodes:
- `Handle kb.ingest`: GET/POST `http://weaviate:8080/v1/schema`, `/v1/objects`, `/v1/.well-known/ready`
- `Handle kb.query`: Verba `/api/query` plus a Weaviate call (`url: WEAVIATE_URL`)
- `Handle health.ping`: Weaviate ready check (ready/live stay open, so this one survives)
With anonymous off, kb.ingest and the Weaviate part of kb.query return 401. n8n has `N8N_BLOCK_ENV_ACCESS_IN_NODE=true`, so the key must reach the nodes the same way the Letta token does: an n8n Header Auth credential (`scripts/n8n_sync_credentials.sh` pattern) and `helpers.httpRequestWithAuthentication`. That is a workflow edit + credential, owned by the n8n lane, not done here. Do not start step 2 until it is ready and exported.
Also anonymous (curl, no key), so update or accept breakage: `stack/scripts/stack_smoke_test.sh:192,194` (DELETE on :8080), `stack/scripts/stack_status.sh` (only the open ready endpoint, fine). Verba itself: needs the compose env below (WEAVIATE_API_KEY_VERBA); confirm the pinned Verba build reads that variable (Verba docs name it; not verified here).

## Steps (ordered)
1. Make a key and store it twice, without showing it:
   - `.env`: add `WEAVIATE_API_KEY=<random 32+ chars>` (optional `WEAVIATE_API_USER=amara`).
   - Keychain: `secret_set` (Settings) or `security add-generic-password -U -s com.taisun.nexus.weaviate_api_key -a amara -w` with no value after `-w`, so it prompts for the key (type or paste it at the prompt). Never put the key on a command line, in a `$(...)` expansion or in shell history.
2. Snapshot BEFORE (anonymous): `node tasks/proposed/kb-parity.mjs snapshot --out /tmp/kb-before.json` (not run yet; ids only).
3. Back up first: stop nothing; copy `~/projects/nexus/stack/data/weaviate` (cp -R to a dated folder) and the compose file.
4. Stage 2a: apply `tasks/proposed/weaviate-apikey-compose.diff` by hand (anonymous stays "true"), then `cd ~/projects/nexus/stack && docker compose up -d weaviate verba`. Verify: Home KB card still Running; `curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $KEY" http://127.0.0.1:8080/v1/schema` = 200.
5. Step 0 deliverable (n8n credential + edited nodes) imported and `kb.query` / `kb.ingest` tested with the key.
6. Snapshot AFTER with the key: `KB_PARITY_KEY=... node tasks/proposed/kb-parity.mjs snapshot --out /tmp/kb-after.json --key-env KB_PARITY_KEY`, then `compare /tmp/kb-before.json /tmp/kb-after.json` must print PARITY OK.
7. Stage 2b: set `AUTHENTICATION_ANONYMOUS_ACCESS_ENABLED: "false"`, `docker compose up -d weaviate`. Verify an anonymous `curl http://127.0.0.1:8080/v1/schema` now returns 401, KB card Running (the app sends the key), a crew `kb_query` works, Verba UI loads.
8. Log it in docs/decisions/ (date, what changed, key location).

## Rollback
- Fast: set `AUTHENTICATION_ANONYMOUS_ACCESS_ENABLED: "true"`, `docker compose up -d weaviate verba`. Data is untouched (auth does not change storage), so no restore is needed.
- Full: restore the compose file copy from step 3 and `up -d` again; a Weaviate with API-key auth disabled may reject a Bearer header it cannot validate, so also remove the Keychain item if you revert the full compose: `security delete-generic-password -s com.taisun.nexus.weaviate_api_key -a amara`).
- Data restore only if step 3 backup is needed (corruption): stop weaviate, replace `data/weaviate`, start.
