#!/usr/bin/env bash
set -euo pipefail

if [ ! -f .env ]; then
  echo "✗ No .env found. Run ./setup.sh first."
  exit 1
fi

# Verify no secrets are still placeholder
if grep -q "CHANGE_ME\|your_key_here" .env 2>/dev/null; then
  echo "✗ .env contains placeholder values. Fill in real secrets before starting."
  exit 1
fi

echo "▶ Starting Terrance Intelligence Stack..."
docker compose up -d --build

echo ""
echo "╔═══════════════════════════════════════════════════════╗"
echo "║          Terrance Stack — Local Services              ║"
echo "╠═══════════════════════════════════════════════════════╣"
echo "║  n8n Automation    →  http://localhost:5678            ║"
echo "║  Letta REST + ADE  →  http://localhost:8283            ║"
echo "║  Verba RAG         →  http://localhost:8001            ║"
echo "║  Crew Service      →  http://localhost:8400            ║"
echo "║  Weaviate          →  http://localhost:8080  (internal)║"
echo "║  PostgreSQL        →  localhost:5432         (internal)║"
echo "╚═══════════════════════════════════════════════════════╝"
echo ""
echo "First run: n8n will prompt you to create an owner account at http://localhost:5678"
echo ""
echo "Logs:  docker compose logs -f [n8n|letta|verba|weaviate|postgres]"
echo "Stop:  ./stop.sh"
