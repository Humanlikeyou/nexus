#!/usr/bin/env bash
# First-time setup: generates .env with cryptographically random secrets.
# Run ONCE before `./start.sh`.
set -euo pipefail

if [ -f .env ]; then
  echo "⚠  .env already exists. Delete it and re-run if you want to regenerate secrets."
  exit 0
fi

POSTGRES_PASSWORD=$(openssl rand -hex 24)
N8N_ENCRYPTION_KEY=$(openssl rand -hex 16)   # 32 hex chars = valid n8n key

cat > .env <<EOF
# ── Terrance Stack — Secrets ────────────────────────────────────────────────
# Generated: $(date)
# DO NOT commit. Covered by .gitignore.

# PostgreSQL (shared by n8n and Letta)
POSTGRES_USER=terrance
POSTGRES_PASSWORD=${POSTGRES_PASSWORD}

# n8n — workflow credential encryption key (32 chars)
N8N_ENCRYPTION_KEY=${N8N_ENCRYPTION_KEY}

# Timezone
TIMEZONE=America/Los_Angeles

# ── LLM API Keys ─────────────────────────────────────────────────────────────
# Fill in at least one. Leave the rest blank — services ignore empty keys.
ANTHROPIC_API_KEY=
OPENAI_API_KEY=

# ── Ollama (local LLM, running on host outside Docker) ───────────────────────
# Defaults point to host.docker.internal which resolves to the Mac host.
# Start Ollama on the Mac, pull models, then these just work.
OLLAMA_BASE_URL=http://host.docker.internal:11434
OLLAMA_URL=http://host.docker.internal:11434
OLLAMA_MODEL=llama3
OLLAMA_EMBED_MODEL=nomic-embed-text
EOF

echo "✓ .env created with generated secrets."
echo ""
echo "Next steps:"
echo "  1. Open .env and fill in ANTHROPIC_API_KEY and/or OPENAI_API_KEY"
echo "     (or leave blank if using local Ollama only)"
echo "  2. Run: ./start.sh"
