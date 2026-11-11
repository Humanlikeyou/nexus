# Inventory: n8n workflows and Docker containers (2026-11-11, read-only)

Decision 0007 #3: **list only.** Nothing here was deleted, stopped or modified.

## n8n workflows (`docker exec stack-n8n-1 n8n list:workflow`)

| ID | Name | Active | Note |
|---|---|---|---|
| `eYdwlwZKTMV89z2J` | Terrance Ingress Gateway | **yes** | The live Ingress (export in `stack/n8n/workflows/`) |
| `sMtd9mnMhXid8MQn` | Terrance Ingress Gateway | no | Duplicate, inactive copy |
| `laKQPqbtCQcWT3XL` | Terrance Ingress Gateway | no | Duplicate, inactive copy |
| `WxCNKfPXqcn6YY4Q` | OpenClaw Health Monitor | no | Keep off: its Telegram node ignores NEXUS_OUTBOUND_DISABLED; OpenClaw is removed |
| `envProbeStage5E1` | ZZ envprobe (Stage 5E test, inactive) | no | Old test workflow |

Candidates for Taisun to remove when he wants: the two duplicate Ingress copies, the OpenClaw monitor, the envprobe.

## Containers (`docker ps -a`)

| Name | Image | State | Note |
|---|---|---|---|
| stack-n8n-1 | n8nio/n8n:2.44.5 | up, healthy | |
| stack-postgres-1 | pgvector 0.8.7-pg17 | up, healthy | |
| stack-crew-1 | stack-crew | up, healthy | Still the pre-session build: needs the rebuild in TAISUN-STEPS §9.9 |
| stack-letta-1 | letta/letta:0.16.8 | up | CVE-2026-77937, see `letta-upgrade-plan.md` |
| stack-verba-1, stack-weaviate-1 | | up, healthy | |
| homeassistant | home-assistant 2026.10.4 | up | |
| freqtrade-live | freqtrade stable_freqai | up | |
| **freqtrade** | freqtrade stable_freqai | exited (137) 2 weeks ago | Stale |
| **a7f1d68c3eee_nexus5etest-postgres-1** | pgvector 0.8.7-pg17 | created, never started | Stale test DB |
| **(unnamed) a7f1d68c3eee** | image `47af0b65960b` (no longer present) | exited 3 weeks ago | Likely the old openclaw-sandbox; `docker inspect` no longer resolves it |
