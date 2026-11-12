# n8n examples (inactive, not imported)

`crew-trigger.example.json` starts a Crews run through crew-service's signed trigger. It is a file only: do not
import or activate it from a script. Taisun imports it by hand if he wants it (n8n > Import from file), and
nothing in this repo creates, changes or deletes any n8n workflow.

## The trigger endpoint

`POST http://crew:8400/crews/trigger` (from n8n, on the compose network; host port stays 127.0.0.1).

| Header | Value |
|---|---|
| `X-Crew-Timestamp` | Unix seconds (must be within 5 minutes of the service clock) |
| `X-Crew-Signature` | `sha256=` + hex HMAC-SHA256 of `"<timestamp>." + <raw body>` using `CREW_TRIGGER_SECRET` |

Body (JSON, same rules as `/crews/start`, no `retry_of`): `{"name": "...", "task": "...", "agents": ["researcher"]}`.

* No secret configured on the service: the endpoint is off and every call is **403**.
* Missing, wrong, stale or replayed signature: **403** (the secret is compared as bytes in constant time).
* Valid signature, invalid body: 422. Crew already running: 429. Success: the new run as `CrewInfo`
  (visible in `crew_list`). The trigger secret is separate from `X-Crew-Token`, so n8n never holds the all-powers token.
* Setup (Taisun): generate a random 32+ byte value, put `CREW_TRIGGER_SECRET=...` in `stack/.env.crew`
  (git-ignored) and expose it to n8n yourself; compose passes it through when set. Not done by any script.

## Status probe

The app's `n8n_status` command is read-only: `GET /healthz` for connected. Workflow and 7-day failure counts need
n8n's API key; if you want them, put the key in the app data file `n8n_api_key` (mode 0600). Without it those
fields stay unwired in the Crews tile.
