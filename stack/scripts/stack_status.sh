#!/usr/bin/env bash
# stack_status.sh — Hard health verification for the Terrance stack.
# Exits 0 if all required services are healthy; exits 1 on any failure.
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
FAIL=0

pass() { echo -e "  ${GREEN}✓${NC} $*"; }
fail() { echo -e "  ${RED}✗${NC} $*"; FAIL=1; }
warn() { echo -e "  ${YELLOW}!${NC} $*"; }

# ── 1. Running containers ────────────────────────────────────────────────────
echo "=== Terrance Stack Status ==="
echo ""
echo "▶ Containers:"
docker ps --format "  {{.Names}}\t{{.Status}}\t{{.Ports}}" 2>/dev/null \
  | column -t \
  || warn "docker not available or not running"
echo ""

# ── 2. Host listeners (lsof) ────────────────────────────────────────────────
echo "▶ Local listeners:"
for port in 5432 5678 8001 8080 8283; do
  line=$(lsof -iTCP:$port -sTCP:LISTEN -n -P 2>/dev/null | awk 'NR==2{print $1, $9}')
  if [ -n "$line" ]; then
    pass ":$port  → $line"
  else
    fail ":$port  → nothing listening"
  fi
done
echo ""

# ── 3. Service health checks (HTTP) ─────────────────────────────────────────
echo "▶ Health checks:"

http_check() {
  local name="$1"
  local url="$2"
  local http_code
  http_code=$(curl -so /dev/null -w "%{http_code}" --max-time 5 "$url" 2>/dev/null)
  if [ "$http_code" = "200" ]; then
    pass "$name ($url)  → HTTP $http_code"
  else
    fail "$name ($url)  → HTTP $http_code"
  fi
}

http_check "n8n            " "http://localhost:5678"
http_check "Letta REST     " "http://localhost:8283/v1/agents/"
http_check "Weaviate ready " "http://localhost:8080/v1/.well-known/ready"
http_check "Verba health   " "http://localhost:8001/api/health"
echo ""

# ── 4. Gateway reachability (n8n ingress) ───────────────────────────────────
echo "▶ Gateway ping:"
INGRESS="http://localhost:5678/webhook/terrance/ingress"
PING_CODE=$(curl -so /dev/null -w "%{http_code}" --max-time 5 \
  -X POST "$INGRESS" \
  -H "Content-Type: application/json" \
  -d '{"trace_id":"status-check","event":{"type":"health.ping","payload":{}}}' 2>/dev/null)

if [ "$PING_CODE" = "200" ]; then
  pass "Ingress gateway ($INGRESS)  → HTTP $PING_CODE"
elif [ "$PING_CODE" = "404" ]; then
  warn "Ingress gateway ($INGRESS)  → HTTP $PING_CODE (workflow not imported yet)"
else
  fail "Ingress gateway ($INGRESS)  → HTTP $PING_CODE"
fi
echo ""

# ── 5. Summary ───────────────────────────────────────────────────────────────
if [ "$FAIL" -eq 0 ]; then
  echo -e "${GREEN}All required checks passed.${NC}"
  exit 0
else
  echo -e "${RED}One or more checks FAILED.${NC}"
  echo "  • Check Docker Desktop is running:   open -a Docker"
  echo "  • Tail logs:                          docker compose logs -f --tail=50"
  echo "  • Restart stack:                      ./stop.sh && ./start.sh"
  exit 1
fi
