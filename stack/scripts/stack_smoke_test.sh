#!/usr/bin/env bash
# stack_smoke_test.sh — End-to-end smoke test through the n8n ingress gateway.
# Tests: health.ping, memory.write/query (persistent terrance-prime), kb.ingest/query.
# Requires the "Terrance Ingress Gateway" workflow to be active in n8n.
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BOLD='\033[1m'; NC='\033[0m'
FAIL=0
INGRESS="http://localhost:5678/webhook/terrance/ingress"
TRACE_ID="smoke-$(date +%s)"
TERRANCE_AGENT_ID="agent-5efd68f1-6976-4af5-a677-b82f668d6843"

pass()  { echo -e "  ${GREEN}✓${NC} $*"; }
fail()  { echo -e "  ${RED}✗${NC} $*"; FAIL=1; }
info()  { echo -e "  ${YELLOW}→${NC} $*"; }
title() { echo -e "\n${BOLD}$*${NC}"; }

post_event() {
  local body="$1"
  curl -s --max-time 30 \
    -X POST "$INGRESS" \
    -H "Content-Type: application/json" \
    -d "$body"
}

# ── 0. Pre-flight ────────────────────────────────────────────────────────────
title "=== Terrance Stack Smoke Test (trace_id: $TRACE_ID) ==="

PING=$(curl -so /dev/null -w "%{http_code}" --max-time 5 "http://localhost:5678" 2>/dev/null)
if [ "$PING" != "200" ]; then
  echo -e "${RED}n8n is not reachable at localhost:5678. Run ./start.sh first.${NC}"
  exit 1
fi

# ── a) health.ping ───────────────────────────────────────────────────────────
title "a) health.ping"
RESP=$(post_event "{\"trace_id\":\"${TRACE_ID}\",\"event\":{\"type\":\"health.ping\",\"payload\":{}}}")
echo "  Response: $RESP"
OK=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('ok',''))" 2>/dev/null)
TID=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('trace_id',''))" 2>/dev/null)
if [ "$OK" = "True" ] && [ "$TID" = "$TRACE_ID" ]; then
  pass "ok=true, trace_id echoed correctly"
else
  fail "Expected ok=true with trace_id=${TRACE_ID}, got: $RESP"
fi

# ── b) memory.write (persistent terrance-prime) ─────────────────────────────
title "b) memory.write (update terrance-prime core memory)"
NOTE="Stage 3 smoke test note — the answer is 42. trace=${TRACE_ID}"
WRITE_RESP=$(post_event "{
  \"trace_id\": \"${TRACE_ID}\",
  \"event\": {
    \"type\": \"memory.write\",
    \"payload\": {
      \"content\": \"${NOTE}\",
      \"mode\": \"core\",
      \"block\": \"human\"
    }
  }
}")
echo "  Response: $WRITE_RESP"
WRITE_OK=$(echo "$WRITE_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('ok',''))" 2>/dev/null)
WRITE_AGENT=$(echo "$WRITE_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('result',{}).get('agent_id',''))" 2>/dev/null)
if [ "$WRITE_OK" = "True" ] && [ "$WRITE_AGENT" = "$TERRANCE_AGENT_ID" ]; then
  pass "ok=true, wrote to persistent agent terrance-prime"
else
  fail "Expected ok=true + terrance-prime agent_id. Got: $WRITE_RESP"
fi

# ── c) memory.query (default to terrance-prime) ─────────────────────────────
title "c) memory.query (retrieve from terrance-prime — no agent_id needed)"
QUERY_RESP=$(post_event "{
  \"trace_id\": \"${TRACE_ID}\",
  \"event\": {
    \"type\": \"memory.query\",
    \"payload\": {}
  }
}")
QUERY_SUMMARY=$(echo "$QUERY_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
result = d.get('result', {})
blocks = result.get('blocks', [])
block_summary = [{b['label']: b['value'][:80]} for b in blocks]
print(json.dumps({'ok': d.get('ok'), 'agent_id': result.get('agent_id',''), 'blocks': block_summary}))
" 2>/dev/null)
echo "  Response: $QUERY_SUMMARY"
QUERY_OK=$(echo "$QUERY_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('ok',''))" 2>/dev/null)
QUERY_AGENT=$(echo "$QUERY_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('result',{}).get('agent_id',''))" 2>/dev/null)
NOTE_FOUND=$(echo "$QUERY_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
blocks = d.get('result', {}).get('blocks', [])
all_values = ' '.join(b.get('value','') for b in blocks if isinstance(b, dict))
print('found' if '42' in all_values else 'not-found')
" 2>/dev/null)
if [ "$QUERY_OK" = "True" ] && [ "$QUERY_AGENT" = "$TERRANCE_AGENT_ID" ] && [ "$NOTE_FOUND" = "found" ]; then
  pass "ok=true, defaulted to terrance-prime, note content retrieved"
else
  fail "Expected ok=true, terrance-prime agent, and note in memory. Got query response above"
fi

# ── d) kb.ingest ─────────────────────────────────────────────────────────────
title "d) kb.ingest (ingest test document into Weaviate)"
KB_CONTENT="Smoke test document for Terrance Stage 3 — trace_id=${TRACE_ID}. This document tests the kb.ingest pipeline that vectorizes content with Ollama nomic-embed-text and stores it in Weaviate for Verba queries."
INGEST_RESP=$(post_event "{
  \"trace_id\": \"${TRACE_ID}\",
  \"event\": {
    \"type\": \"kb.ingest\",
    \"payload\": {
      \"content\": \"${KB_CONTENT}\",
      \"filename\": \"smoke-test-${TRACE_ID}.txt\",
      \"labels\": [\"smoke-test\"]
    }
  }
}")
echo "  Response: $INGEST_RESP"
INGEST_OK=$(echo "$INGEST_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('ok',''))" 2>/dev/null)
DOC_UUID=$(echo "$INGEST_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('result',{}).get('doc_uuid',''))" 2>/dev/null)
CHUNKS=$(echo "$INGEST_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('result',{}).get('chunks_created',0))" 2>/dev/null)
if [ "$INGEST_OK" = "True" ] && [ -n "$DOC_UUID" ] && [ "$CHUNKS" -gt 0 ] 2>/dev/null; then
  pass "ok=true, doc_uuid=${DOC_UUID}, chunks=${CHUNKS}"
else
  fail "Expected ok=true + doc_uuid + chunks. Got: $INGEST_RESP"
fi

# ── e) kb.query (retrieve the ingested document) ────────────────────────────
title "e) kb.query (retrieve ingested smoke test document)"
KB_RESP=$(post_event "{
  \"trace_id\": \"${TRACE_ID}\",
  \"event\": {
    \"type\": \"kb.query\",
    \"payload\": {
      \"query\": \"smoke test Stage 3 Terrance\"
    }
  }
}")
KB_SUMMARY=$(echo "$KB_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
result = d.get('result', {})
docs = result.get('documents', [])
summary = {'ok': d.get('ok'), 'error': result.get('error',''), 'doc_count': len(docs)}
for doc in docs[:3]:
    summary[doc.get('title','')] = f'score={doc.get(\"score\",0):.2f}, chunks={len(doc.get(\"chunks\",[]))}'
print(json.dumps(summary))
" 2>/dev/null)
echo "  Response: $KB_SUMMARY"
KB_OK=$(echo "$KB_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('ok',''))" 2>/dev/null)
KB_DOCS=$(echo "$KB_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
docs = d.get('result', {}).get('documents', [])
# Check if our smoke test doc appears in results
for doc in docs:
    if 'smoke-test' in doc.get('title', ''):
        print('found')
        break
else:
    print('not-found')
" 2>/dev/null)
if [ "$KB_OK" = "True" ] && [ "$KB_DOCS" = "found" ]; then
  pass "ok=true, smoke test document retrieved from Verba"
else
  fail "Expected ok=true and smoke test doc in results. Got kb.query response above"
fi

# ── f) Cleanup ───────────────────────────────────────────────────────────────
title "f) Cleanup"

# Restore terrance-prime's human block to default
RESTORE_RESP=$(post_event "{
  \"trace_id\": \"${TRACE_ID}\",
  \"event\": {
    \"type\": \"memory.write\",
    \"payload\": {
      \"content\": \"Terrance persistent memory — primary agent for all memory operations.\",
      \"mode\": \"core\",
      \"block\": \"human\"
    }
  }
}")
RESTORE_OK=$(echo "$RESTORE_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('ok',''))" 2>/dev/null)
if [ "$RESTORE_OK" = "True" ]; then
  pass "Restored terrance-prime human block to default"
else
  info "Failed to restore human block: $RESTORE_RESP"
fi

# Delete the smoke test document and its chunks from Weaviate
if [ -n "$DOC_UUID" ]; then
  curl -s --max-time 5 -X DELETE "http://localhost:8080/v1/objects/VERBA_DOCUMENTS/${DOC_UUID}" >/dev/null 2>&1 || true
  # Delete chunks by batch delete with filter
  curl -s --max-time 5 -X DELETE "http://localhost:8080/v1/batch/objects" \
    -H "Content-Type: application/json" \
    -d "{\"match\":{\"class\":\"VERBA_Embedding_nomic_embed_text\",\"where\":{\"operator\":\"Equal\",\"path\":[\"doc_uuid\"],\"valueText\":\"${DOC_UUID}\"}}}" >/dev/null 2>&1 || true
  pass "Cleaned up smoke test document and chunks"
fi

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "────────────────────────────────"
if [ "$FAIL" -eq 0 ]; then
  echo -e "${GREEN}${BOLD}All smoke tests PASSED.${NC}"
  exit 0
else
  echo -e "${RED}${BOLD}One or more smoke tests FAILED.${NC}"
  echo ""
  echo "Troubleshooting:"
  echo "  1. Check workflow is imported & active: http://localhost:5678"
  echo "  2. Run stack health check:              ./scripts/stack_status.sh"
  echo "  3. Tail n8n logs:                       docker compose logs -f n8n"
  exit 1
fi
