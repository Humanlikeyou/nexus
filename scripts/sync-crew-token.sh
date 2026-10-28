#!/usr/bin/env bash
# Write CREW_SERVICE_TOKEN from stack/.env into <app data dir>/crew_service_token
# (mode 0600) so a Finder-launched Amara OS can reach the crew service.
# Never prints the token: output is "written" or "unchanged".
#
# Usage: scripts/sync-crew-token.sh [--env <stack/.env>] [--dest <token file>]
# Defaults: stack/.env in this repo; $NEXUS_DATA_DIR/crew_service_token, else
# ~/Library/Application Support/com.taisun.nexus/crew_service_token.
set -euo pipefail
umask 077

repo="$(cd "$(dirname "$0")/.." && pwd)"
env_file="$repo/stack/.env"
data_dir="${NEXUS_DATA_DIR:-$HOME/Library/Application Support/com.taisun.nexus}"
dest="$data_dir/crew_service_token"

while [ $# -gt 0 ]; do
  case "$1" in
    --env) env_file="$2"; shift 2 ;;
    --dest) dest="$2"; shift 2 ;;
    *) echo "sync-crew-token: unknown argument: $1" >&2; exit 2 ;;
  esac
done

[ -f "$env_file" ] || { echo "sync-crew-token: env file not found: $env_file" >&2; exit 1; }

# Last CREW_SERVICE_TOKEN= line wins (dotenv semantics); strip `export`,
# surrounding quotes and an unquoted trailing comment.
line="$(grep -E '^[[:space:]]*(export[[:space:]]+)?CREW_SERVICE_TOKEN=' "$env_file" | tail -n 1 || true)"
value="${line#*=}"
case "$value" in
  \"*) value="${value#\"}"; value="${value%%\"*}" ;;
  \'*) value="${value#\'}"; value="${value%%\'*}" ;;
  *) value="${value%%[[:space:]]#*}"; value="${value%"${value##*[![:space:]]}"}" ;;
esac
[ -n "$value" ] || { echo "sync-crew-token: CREW_SERVICE_TOKEN is missing or empty in $env_file" >&2; exit 1; }

if [ -L "$dest" ]; then
  echo "sync-crew-token: refusing to write through a symlink: $dest" >&2
  exit 1
fi

if [ -f "$dest" ] && [ "$(cat "$dest")" = "$value" ]; then
  chmod 600 "$dest"
  echo "unchanged"
  exit 0
fi

mkdir -p "$(dirname "$dest")"
tmp="$(mktemp "$(dirname "$dest")/.crew_service_token.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
printf '%s' "$value" > "$tmp"
chmod 600 "$tmp"
mv -f "$tmp" "$dest"
trap - EXIT
echo "written"
