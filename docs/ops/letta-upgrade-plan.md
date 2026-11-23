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

## Recheck 2026-11-12: no patched tag exists; decision needed (Taisun)

Checked the public Docker Hub tag list (`letta/letta`) and GitHub releases (`letta-ai/letta`) on 2026-11-12: the newest Python-server release is still **0.16.8** (2026-05-14). The upstream fix **0.16.9 (PR #3407) was never published**. Tags after 0.16.8 are "Letta Code" / the new App Server, which can't mount the old Postgres data and don't support `SECURE` (`stack/SECURITY-NOTES.md`). So steps 2–3 above ("pinned patched tag") **can't be done as written**.

What CVE-2026-77937 is: an authenticated caller can impersonate another Letta user with a self-asserted `user_id` header. Our setup has one user, a password, and a loopback-only port, so the exposure is any local process that already holds the Letta password (n8n, Nexus, crews).

Options for decision 0014 #1 (Letta writes). Taisun picks one:

| Option | What it means | Cost / risk |
|---|---|---|
| **A. Accept the risk and build writes on 0.16.8** (recommended) | Keep the pin, password and loopback bind. Writes go through one reviewed Nexus command, with a `pg_dump` backup before any delete. Recheck for 0.16.9 monthly. | The CVE stays. It matters only if a second Letta user is ever added, which the notes already forbid. |
| B. Build 0.16.9 from source | Build a local image from the `letta-ai/letta` source at the fix commit and pin it by digest. | Unofficial build to maintain, and its migrations are untested on our data. Needs a backup and a rollback rehearsal. |
| C. Migrate to the new Letta App Server | The supported path going forward. | A data migration and API changes (Nexus, n8n `memory.query`, crews). This is a project of its own. |

Backup step (any option; Taisun runs it): `docker exec stack-postgres-1 pg_dump -U <letta role or superuser from stack/.env> -d letta_db -Fc > ~/Backups/letta-$(date +%Y%m%d).dump`. The DB is `letta_db` (compose `LETTA_PG_URI`), not `letta` as written in step 1 above.

Until Taisun answers, the read-only Memory list (`memory_list`, wiring class B) can ship on its own. Edit, delete and decide stay unwired.
