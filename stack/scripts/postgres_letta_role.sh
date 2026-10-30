#!/usr/bin/env bash
# Idempotent: create/refresh a dedicated login role for Letta that can only use letta_db.
#  - role ${LETTA_DB_USER:-letta_app}, password = LETTA_DB_PASSWORD from stack/.env (never printed)
#  - owns letta_db and every object in its public schema (Letta runs alembic at startup)
#  - CONNECT on letta_db only; PUBLIC loses CONNECT on letta_db, n8n_db, terrance
set -euo pipefail
cd "$(dirname "$0")/.."
PG="${PG_CONTAINER:-$(docker compose ps -q postgres)}"
[ -n "$PG" ] || { echo "postgres container is not running" >&2; exit 1; }
getenv() { grep -E "^$1=" .env | tail -1 | cut -d= -f2- ; }
SU=$(getenv POSTGRES_USER); ROLE=$(getenv LETTA_DB_USER); ROLE=${ROLE:-letta_app}
PW=$(getenv LETTA_DB_PASSWORD)
[ -n "$PW" ] || { echo "LETTA_DB_PASSWORD is empty in .env" >&2; exit 1; }
[[ "$ROLE" =~ ^[a-z_][a-z0-9_]*$ ]] || { echo "bad LETTA_DB_USER" >&2; exit 1; }
# Password goes over stdin only (not argv); psql :'var' quoting handles escaping.
{
  printf '\\set pw %s\n' "'$(printf %s "$PW" | sed "s/'/''/g")'"
  cat <<SQL
SELECT format('CREATE ROLE %I LOGIN', '$ROLE') WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '$ROLE') \gexec
ALTER ROLE "$ROLE" WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS PASSWORD :'pw';
ALTER DATABASE letta_db OWNER TO "$ROLE";
REVOKE CONNECT ON DATABASE letta_db, n8n_db, terrance FROM PUBLIC;
GRANT CONNECT, TEMPORARY ON DATABASE letta_db TO "$ROLE";
\connect letta_db
ALTER SCHEMA public OWNER TO "$ROLE";
SELECT format('ALTER TABLE %I.%I OWNER TO %I', schemaname, tablename, '$ROLE') FROM pg_tables WHERE schemaname = 'public' \gexec
SELECT format('ALTER SEQUENCE %I.%I OWNER TO %I', sequence_schema, sequence_name, '$ROLE') FROM information_schema.sequences WHERE sequence_schema = 'public' \gexec
SELECT format('ALTER VIEW %I.%I OWNER TO %I', schemaname, viewname, '$ROLE') FROM pg_views WHERE schemaname = 'public' \gexec
SELECT format('ALTER TYPE %I.%I OWNER TO %I', n.nspname, t.typname, '$ROLE') FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
  WHERE n.nspname = 'public' AND t.typtype IN ('e','d') \gexec
SQL
} | docker exec -i "$PG" psql -v ON_ERROR_STOP=1 -q -U "$SU" -d postgres >/dev/null
echo "letta DB role '$ROLE' ready (password not shown)."
