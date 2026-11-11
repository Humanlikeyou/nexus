# Letta upgrade plan (CVE-2026-77937): plan only, not applied

Decision 0007 #3: plan the upgrade; **don't apply it tonight; don't touch Letta data.**

- **Now:** `stack-letta-1` runs `letta/letta:0.16.8`, bound to localhost with a password. The CVE is reachable only from this Mac (and its containers).
- **Target:** the first Letta release whose notes list the CVE-2026-77937 fix. **Not verified here** (outbound is off for this work); confirm on Letta's GitHub releases page and pin that exact tag (never `latest`).
- **Steps for Taisun (when he says go):**
  1. Back up first: stop nothing yet; `docker exec stack-postgres-1 pg_dump -U terrance -d letta -Fc > ~/Backups/letta-$(date +%Y%m%d).dump` (check the DB name in `stack/docker-compose.yml`), and copy any Letta volume.
  2. Read that release's migration notes (0.16 → target) for schema or API changes, especially agent/memory block formats Nexus and crews read (`memory_query`).
  3. Change the image tag in `stack/docker-compose.yml` to the pinned version; `docker compose pull letta && docker compose up -d letta`.
  4. Check: Letta health endpoint, the n8n `memory.query` path, and Hermes/crew memory reads.
  5. Rollback: put back `0.16.8` and `docker compose up -d letta`; restore the dump only if the migration changed data.
- **Risk:** a schema migration on start. That's why step 1 is a backup and why nothing runs without his OK.
