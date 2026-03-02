#!/usr/bin/env bash
# Run once on first postgres container start.
# Creates n8n_db and letta_db, enables pgvector in letta_db.
set -e

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    CREATE DATABASE n8n_db;
    CREATE DATABASE letta_db;
EOSQL

# pgvector extension must be created inside letta_db specifically
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "letta_db" <<-EOSQL
    CREATE EXTENSION IF NOT EXISTS vector;
EOSQL

echo "[init-dbs] n8n_db and letta_db created. pgvector enabled in letta_db."
