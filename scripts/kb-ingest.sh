#!/usr/bin/env bash
# Safe knowledge-base ingest. Default is --dry-run (nothing written or sent); --apply waits
# for Taisun's OK. See stack/kb/INGEST-PLAN.md and scripts/kb-ingest.mjs.
set -euo pipefail
exec node "$(cd "$(dirname "$0")" && pwd)/kb-ingest.mjs" "$@"
