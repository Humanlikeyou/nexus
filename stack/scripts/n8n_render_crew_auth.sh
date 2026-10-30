#!/usr/bin/env bash
# Render the Ingress export for import with the crew key's SHA-256 in the "Crew Auth" node.
# The committed export keeps the placeholder __CREW_KEY_SHA256__, which never matches (fail closed).
# Output is git-ignored (stack/n8n/rendered/), mode 0600; the key and its hash are never printed.
#   CREW_INGRESS_SECRET unset/empty -> hash of a random value nobody holds (crew access revoked)
#   equal to NEXUS_SECRET or under 43 chars (32 bytes) -> refused
# Does not import anything: import the rendered file yourself (n8n import:workflow).
#
# Usage: stack/scripts/n8n_render_crew_auth.sh [--env stack/.env] [--in <export.json>] [--out <rendered.json>]
set -euo pipefail
umask 077
stack="$(cd "$(dirname "$0")/.." && pwd)"
env_file="$stack/.env"
in="$stack/n8n/workflows/eYdwlwZKTMV89z2J.json"
out="$stack/n8n/rendered/eYdwlwZKTMV89z2J.json"
while [ $# -gt 0 ]; do
  case "$1" in
    --env) env_file="$2"; shift 2 ;;
    --in) in="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "n8n_render_crew_auth: unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -f "$env_file" ] || { echo "n8n_render_crew_auth: env file not found: $env_file" >&2; exit 1; }
[ -L "$out" ] && { echo "n8n_render_crew_auth: refusing to write through a symlink: $out" >&2; exit 1; }
mkdir -p "$(dirname "$out")"
ENV_FILE="$env_file" IN="$in" OUT="$out" python3 - <<'PY'
import hashlib, os, secrets, sys, tempfile
env = {}
for line in open(os.environ['ENV_FILE']):
    line = line.strip()
    if line and not line.startswith('#') and '=' in line:
        k, v = line.split('=', 1)
        env[k.strip().removeprefix('export ').strip()] = v.strip().strip('"').strip("'")
crew = env.get('CREW_INGRESS_SECRET', '')
if not crew:
    crew = secrets.token_urlsafe(48)
    print('CREW_INGRESS_SECRET is empty; crew webhook rendered with a random key (crew access revoked)', file=sys.stderr)
elif crew == env.get('NEXUS_SECRET'):
    sys.exit('CREW_INGRESS_SECRET must differ from NEXUS_SECRET (it is the read-only key)')
elif len(crew) < 43:
    sys.exit('CREW_INGRESS_SECRET is shorter than 43 chars (32 bytes); generate one with: openssl rand -base64 48 | tr "+/" "-_" | tr -d "="')
src = open(os.environ['IN']).read()
if src.count('__CREW_KEY_SHA256__') != 1:
    sys.exit('placeholder __CREW_KEY_SHA256__ not found exactly once in the export')
rendered = src.replace('__CREW_KEY_SHA256__', hashlib.sha256(crew.encode()).hexdigest())
out = os.environ['OUT']
fd, tmp = tempfile.mkstemp(dir=os.path.dirname(out), prefix='.render.')
with os.fdopen(fd, 'w') as f:
    f.write(rendered)
os.chmod(tmp, 0o600)
os.replace(tmp, out)
print('rendered (key not shown):', out)
PY
