#!/usr/bin/env bash
set -euo pipefail
echo "■ Stopping Terrance stack (data volumes preserved)..."
docker compose down
echo "Done. Run ./start.sh to restart."
