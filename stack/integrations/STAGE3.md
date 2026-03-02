# Stage 3 — n8n Gateway Contracts + Health Verification

**Goal:** A single localhost ingress endpoint that routes Nexus/Terrance events to
Letta (memory) and Verba/Weaviate (knowledge base) with a stable `trace_id` contract.

---

## Services Quick Reference

| Service    | Host URL                     | Docker-internal URL       | Purpose                |
|------------|------------------------------|---------------------------|------------------------|
| n8n        | http://localhost:5678        | n8n:5678                  | Workflow automation    |
| Letta      | http://localhost:8283        | letta:8283                | Agent memory (REST+UI) |
| Verba      | http://localhost:8001        | verba:8000                | RAG knowledge base     |
| Weaviate   | http://localhost:8080        | weaviate:8080             | Vector store           |
| PostgreSQL | localhost:5432               | postgres:5432             | Shared RDBMS           |

> **Note**: Port 8083 is unmapped. All Letta traffic goes through **8283**.

---

## Step 1 — Verify the Stack is Running

```bash
cd stack/
./scripts/stack_status.sh
```

Expected output (all green):
```
=== Terrance Stack Status ===

▶ Containers:
  stack-n8n-1        Up X minutes    127.0.0.1:5678->5678/tcp
  stack-letta-1      Up X minutes    127.0.0.1:8283->8283/tcp
  stack-verba-1      Up X minutes    127.0.0.1:8001->8000/tcp
  stack-weaviate-1   Up X minutes    127.0.0.1:8080->8080/tcp
  stack-postgres-1   Up X minutes (healthy)

▶ Local listeners:
  ✓ :5432  → ...
  ✓ :5678  → ...
  ✓ :8001  → ...
  ✓ :8080  → ...
  ✓ :8283  → ...

▶ Health checks:
  ✓ n8n             (http://localhost:5678)               → HTTP 200
  ✓ Letta REST      (http://localhost:8283/v1/agents/)    → HTTP 200
  ✓ Weaviate ready  (http://localhost:8080/v1/.well-known/ready) → HTTP 200
  ✓ Verba health    (http://localhost:8001/api/health)    → HTTP 200

▶ Gateway ping:
  ! Ingress gateway ... → HTTP 404 (workflow not imported yet)

All required checks passed.
```

The gateway ping shows 404 until Step 2 is complete — that's expected.

If any service is down:
```bash
./stop.sh && ./start.sh
docker compose logs -f --tail=50   # watch logs
```

---

## Step 2 — Import the n8n Workflow

The workflow JSON is at:
```
integrations/n8n/terrance_ingress_workflow.json
```

### Import via n8n UI

1. Open http://localhost:5678 (create an owner account on first visit)
2. In the top-left menu → **Workflows** → **New** → click the **⋯** menu (top-right) → **Import from file**
3. Select `integrations/n8n/terrance_ingress_workflow.json`
4. Click **Save** (top-right)
5. Toggle the workflow **Active** (top-right switch)

### Import via CLI (alternative)

```bash
curl -s -X POST http://localhost:5678/rest/workflows \
  -H "Content-Type: application/json" \
  -H "Cookie: $(cat ~/.n8n-cookie 2>/dev/null || echo '')" \
  -d @integrations/n8n/terrance_ingress_workflow.json
```

> n8n's REST API requires a session cookie after initial setup; use the UI import
> for simplicity on a fresh install.

### Verify Import

After activation, the webhook will respond at:
```
POST http://localhost:5678/webhook/terrance/ingress
```

Quick verify:
```bash
curl -s -X POST http://localhost:5678/webhook/terrance/ingress \
  -H "Content-Type: application/json" \
  -d '{"trace_id":"verify-01","event":{"type":"health.ping","payload":{}}}' | python3 -m json.tool
```

Expected:
```json
{
  "ok": true,
  "trace_id": "verify-01",
  "services": {
    "n8n": "up",
    "letta": "http://letta:8283",
    "verba": "http://verba:8000",
    "weaviate": "http://weaviate:8080"
  }
}
```

---

## Step 3 — Run the Smoke Test

```bash
cd stack/
./scripts/stack_smoke_test.sh
```

### What it tests

| Test              | Action                                      | Pass condition                          |
|-------------------|---------------------------------------------|-----------------------------------------|
| a) health.ping    | POST event.type=health.ping                 | ok=true, trace_id echoed                |
| b) memory.write   | Update terrance-prime core memory           | ok=true, agent_id matches terrance-prime|
| c) memory.query   | Retrieve terrance-prime memory (no agent_id)| ok=true, note content in response       |
| d) kb.ingest      | Ingest test doc via Weaviate + Ollama       | ok=true, doc_uuid + chunks returned     |
| e) kb.query       | Retrieve ingested doc via Verba             | ok=true, test doc found in results      |
| f) cleanup        | Restore memory, delete test doc             | Memory restored, doc cleaned up         |

### Expected output (all passing)

```
=== Terrance Stack Smoke Test (trace_id: smoke-1234567890) ===

a) health.ping
  Response: {"ok": true, "trace_id": "smoke-...", "services": {...}}
  ✓ ok=true, trace_id echoed correctly

b) memory.write (store note in Letta)
  Response: {"ok": true, "trace_id": "smoke-...", "result": {"agent_id": "agent-xxx", "name": "smoke-..."}}
  ✓ ok=true, agent_id=agent-xxx
  → agent_id: agent-xxx

c) memory.query (retrieve the note)
  Response: {"ok": true, "trace_id": "smoke-...", "result": [...memory blocks...]}
  ✓ ok=true, note content retrieved from core-memory

d) kb.query (Verba connectivity)
  Response: {"ok": true, "trace_id": "smoke-...", "result": {"documents": [], ...}}
  ✓ ok=true, Verba is reachable via gateway
  → Documents in KB: 0 (expected 0 on fresh stack)

e) Cleanup
  ✓ Test agent deleted: agent-xxx

────────────────────────────────
All smoke tests PASSED.
```

---

## Ingress Contract

### Endpoint

```
POST http://localhost:5678/webhook/terrance/ingress
Content-Type: application/json
```

### Request Schema

```json
{
  "trace_id": "<uuid or short string — echoed back in every response>",
  "event": {
    "type": "health.ping | memory.write | memory.query | kb.ingest | kb.query",
    "payload": { }
  }
}
```

### Response Schema

```json
{
  "ok": true | false,
  "trace_id": "<echoed from request>",
  "result":  { },   // present when ok=true
  "error":   {      // present when ok=false
    "message": "...",
    "node":    "...",
    "details": "..."
  }
}
```

### Event Payloads

#### `health.ping`
```json
{ "payload": {} }
```
Returns `services` map with internal URLs for all downstream dependencies.

#### `memory.write`
```json
{
  "payload": {
    "content": "Note to remember.",       // required
    "mode":    "core",                    // "core" (replace) or "append" (default)
    "block":   "human"                    // memory block label (default: "human")
  }
}
```
Updates the persistent `terrance-prime` agent's core memory block.
Returns `{ agent_id, block, mode, value }`.

#### `memory.query`
```json
{
  "payload": {
    "agent_id": "agent-xxx"   // optional — defaults to terrance-prime
  }
}
```
Returns the agent's `core-memory` blocks as `{ agent_id, blocks: [{ label, value, ... }] }`.

#### `kb.ingest`
```json
{
  "payload": {
    "content":  "Document text to ingest.",   // required
    "filename": "my-doc.txt",                 // optional — auto-generated if omitted
    "labels":   ["topic1"]                    // optional
  }
}
```
Chunks content, vectorizes via Ollama `nomic-embed-text`, and stores in Weaviate
(`VERBA_DOCUMENTS` + `VERBA_Embedding_nomic_embed_text` collections).
Returns `{ doc_uuid, filename, chunks_created, chunk_ids }`.

#### `kb.query`
```json
{
  "payload": {
    "query":           "What do I know about X?",
    "rag_config":      {},    // optional — uses Verba defaults
    "labels":          [],    // optional — filter by labels
    "document_filter": []     // optional — filter by document title/uuid
  }
}
```
Calls Verba's `/api/query` endpoint. Returns `{ documents, context, error }`.
Empty `documents` is normal on a fresh stack with no ingested data.

---

## Workflow Architecture

```
POST /webhook/terrance/ingress
          │
          ▼
 ┌─────────────────────────┐
 │  Route by event.type    │  (Switch node — 5 rules + fallback)
 └────────┬────────────────┘
          │
    ┌─────┼──────────────────────────────────────┐
    │     │                                       │
health  memory.write  memory.query  kb.ingest  kb.query  unknown
    │     │               │             │          │         │
  Code  Code(+Letta)   Code(+Letta)  Code      Code(+Verba) Code
    │     │               │             │          │         │
    └─────┴───────────────┴─────────────┴──────────┴─────────┘
                                  │
                         ┌────────────────┐
                         │  Send Response │  (Respond to Webhook)
                         └────────────────┘
```

All Code nodes:
- Read `trace_id` from request body
- Call downstream service (if applicable) using Docker-internal service names
- Wrap response in `{ ok, trace_id, result }` or `{ ok:false, trace_id, error }`

---

## Troubleshooting

### "workflow not imported yet" (404 on gateway ping)
The workflow JSON has been checked in but must be manually imported and activated
in the n8n UI. See Step 2 above.

### memory.write returns ok=false — "INVALID_ARGUMENT: model handle..."
Letta requires `letta/letta-free` model handle format. If you see this error,
the Code node may be using an old format. Check the n8n workflow and re-import.

### kb.query returns ok=false — "Not allowed" / Origin error
Verba requires `Origin: http://localhost` on POST requests. The workflow Code node
already sets this header. If you see this error after re-import, check the Header
value in the Handle kb.query Code node.

### n8n can't reach letta:8283
All services must be on the same Docker network. Verify with:
```bash
docker network inspect stack_default | grep -E 'Name|IPv4'
```
The `stack_default` network should contain n8n, letta, verba, weaviate, postgres.

### Workflow shows "Error" in n8n execution history
Click the execution in n8n → select the failed node → read the error output.
Most common causes: Letta not running, bad model name, Verba origin check.

### After `docker compose restart n8n`
The `LETTA_URL`, `VERBA_URL`, `WEAVIATE_URL` env vars are injected at container
start. A restart picks them up automatically. No workflow changes needed.

---

## Stage 3 Completions

- **Persistent agent**: `terrance-prime` (`agent-5efd68f1-6976-4af5-a677-b82f668d6843`) — all memory ops use this single agent
- **kb.ingest wired**: Chunks content, vectorizes via Ollama nomic-embed-text, stores in Weaviate collections compatible with Verba queries
- **kb.query updated**: Passes correct RAG config (Ollama embedder + Advanced retriever) so Verba finds ingested documents

## Next Steps (Stage 4)

- Hook CrewAI crews to call `memory.write` and `kb.query` via the gateway
- Add n8n sub-workflow for scheduled Letta memory consolidation
- Add archival memory support (blocked on Letta-free model limitation)
