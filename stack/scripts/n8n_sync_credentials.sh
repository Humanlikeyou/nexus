#!/usr/bin/env bash
# Create/update the n8n credentials used by the Terrance Ingress workflow from stack/.env.
# Values are never printed; the temp file lives only inside the n8n container and is removed.
#   nxIngressSecret1  Header Auth  X-Nexus-Secret: $NEXUS_SECRET          (Webhook node auth)
#   lettaBearerAuth1  Header Auth  Authorization: Bearer $LETTA_SERVER_PASSWORD (HTTP Request nodes)
# The crew webhook no longer uses a credential: its key is checked in the "Crew Auth" node, rendered by
# n8n_render_crew_auth.sh (n8n's Header Auth compares with !== and answers 403, not 401).
set -euo pipefail
cd "$(dirname "$0")/.."
N8N_CONTAINER="${N8N_CONTAINER:-$(docker compose ps -q n8n)}"
[ -n "$N8N_CONTAINER" ] || { echo "n8n container is not running" >&2; exit 1; }
python3 - <<'PY' | docker exec -i "$N8N_CONTAINER" sh -c 'umask 077; cat > /tmp/nexus-creds.json'
import json, sys
env = {}
for line in open('.env'):
    line = line.strip()
    if line and not line.startswith('#') and '=' in line:
        k, v = line.split('=', 1); env[k.strip()] = v.strip().strip('"').strip("'")
for k in ('NEXUS_SECRET', 'LETTA_SERVER_PASSWORD'):
    if not env.get(k):
        sys.exit(f'{k} is empty in .env; refusing to create a credential (fail closed)')
creds = [
    {"id": "nxIngressSecret1", "name": "Nexus Ingress Secret (X-Nexus-Secret)", "type": "httpHeaderAuth",
     "data": {"name": "X-Nexus-Secret", "value": env['NEXUS_SECRET']}},
    {"id": "lettaBearerAuth1", "name": "Letta API (Bearer)", "type": "httpHeaderAuth",
     "data": {"name": "Authorization", "value": "Bearer " + env['LETTA_SERVER_PASSWORD']}},
]
json.dump(creds, sys.stdout)
PY
docker exec "$N8N_CONTAINER" sh -c 'n8n import:credentials --input=/tmp/nexus-creds.json 2>&1 | grep -iv "migration lock"; rm -f /tmp/nexus-creds.json'
echo "n8n credentials synced (values not shown)."
