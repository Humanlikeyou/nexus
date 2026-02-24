# Nexus Stack — Service Reference

The backing services are managed by Docker Compose in a separate repo:
`/Users/thevisualdude/terrance-integrations/stack/`

All services are bound to `127.0.0.1` — **localhost only, never exposed to the network.**

---

## Services

| Service | Container | Port | Purpose |
|---------|-----------|------|---------|
| PostgreSQL + pgvector | `stack-postgres-1` | `127.0.0.1:5432` | Primary database (n8n, Letta) |
| n8n | `stack-n8n-1` | `127.0.0.1:5678` | Automation workflows / webhook ingress |
| Letta | `stack-letta-1` | `127.0.0.1:8283` | Long-term memory agent (REST API + ADE UI) |
| Weaviate | `stack-weaviate-1` | `127.0.0.1:8080` | Vector database |
| Verba | `stack-verba-1` | `127.0.0.1:8001` | RAG UI over Weaviate |

### Non-Docker services

| Service | Port | Manager |
|---------|------|---------|
| OpenClaw AI gateway | `127.0.0.1:18789` | macOS LaunchAgent `ai.openclaw.gateway` |

---

## Start / Stop

```bash
cd /Users/thevisualdude/terrance-integrations/stack

./start.sh   # bring all containers up (detached)
./stop.sh    # bring all containers down
```

To restart a single service:
```bash
docker compose restart n8n
```

To tail logs:
```bash
docker compose logs -f n8n
docker compose logs -f postgres
```

To start/stop OpenClaw independently:
```bash
launchctl start  ai.openclaw.gateway
launchctl stop   ai.openclaw.gateway
```

---

## Health Status (OpenClaw)

Nexus reports three states for the OpenClaw gateway, matching the logic in
`src/components/system/OpenClawStatusCard.tsx` and `src-tauri/src/commands/openclaw.rs`:

| Status | Meaning |
|--------|---------|
| **healthy** | Process running, HTTP `/health` responds 200, config valid, local embedding model found in `~/.node-llama-cpp/models/` |
| **degraded** | Process is up but one sub-check failed (e.g. model file missing, config key unreadable) |
| **offline** | Process not running or HTTP unreachable |

Sub-checks performed by `get_openclaw_health()`:
1. `process` — LaunchAgent PID is alive
2. `http` — GET `http://127.0.0.1:18789/health` returns 200
3. `config` — `~/.openclaw/config.json` is parseable
4. `memory_search` — at least one `.gguf` file exists in `~/.node-llama-cpp/models/`

---

## n8n Workflows

Workflow JSON files live in:
`/Users/thevisualdude/terrance-integrations/stack/integrations/n8n/`

| File | Purpose |
|------|---------|
| `openclaw_health_monitor.json` | Receives health payloads from Nexus → sends Telegram alert on status change |
| `terrance_ingress_workflow.json` | General Terrance AI agent ingress |

To re-import a workflow after editing:
```bash
docker exec stack-n8n-1 n8n import:workflow \
  --input=/path/to/workflow.json
```
