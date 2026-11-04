# Stack security notes

## Accepted risk — Letta 0.16.8 (pinned in docker-compose.yml)

`letta/letta:0.16.8` is the last Docker image of the Python Letta server. Tags after it
(0.17+/0.34.x) ship "Letta Code" / the new App Server, which cannot mount the old
PostgreSQL data and does not support `SECURE`. The upstream fix release 0.16.9
(letta-ai/letta PR #3407) was **not published on Docker Hub** as of 2026-10-21.

Known vulnerabilities affecting `letta <= 0.16.8` (published 2026-10-20):

| CVE | Severity | Issue | Our mitigation |
|---|---|---|---|
| CVE-2026-77936 | Critical (CVSS 4.0 9.3) | Default config leaves the API unauthenticated (`/v1/tools/run` runs arbitrary Python, `/v1/admin`) and the shipped compose exposes Postgres with default superuser creds | `SECURE=true` + `LETTA_SERVER_SECURE=true` + `LETTA_SERVER_PASSWORD` (random, from `.env`); verified unauthenticated calls return 401. Letta and Postgres host ports bound to `127.0.0.1` only; Postgres creds are non-default (from `.env`). |
| CVE-2026-77937 | High (8.6) | Any authenticated caller can impersonate another user via a self-asserted `user_id` header | Single-user deployment: only one user exists, and only local services (n8n, Nexus) hold the password. |
| CVE-2026-77938 | High (7.1) | Tool sandbox subprocesses inherit the full server env (`LETTA_PG_URI`, `POSTGRES_PASSWORD`, provider keys) | Only the password holder can run tools; no untrusted custom tools are installed. Residual risk: a prompt-injected agent tool run could read these env vars. |

Rechecked 2026-11-04: Docker Hub still has no 0.16.9 tag, 0.34.x remains incompatible, and unauthenticated
calls return 401. Keep the pin and the password.

Residual risk accepted for a localhost, single-user setup on Taisun's Mac.
Do **not** expose port 8283 beyond 127.0.0.1, add users, or install third-party Letta tools.

**Revisit after launch:** migrate to the new Letta self-hosted App Server
(https://docs.letta.com/self-hosting/) or to 0.16.9 if it is ever published, and drop this pin.
