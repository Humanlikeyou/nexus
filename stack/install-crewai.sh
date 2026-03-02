#!/usr/bin/env bash
# CrewAI runs as a Python library — no Docker container needed.
# This creates an isolated venv so it doesn't pollute your system Python.
set -euo pipefail

VENV=".venv-crewai"

if [ -d "$VENV" ]; then
  echo "venv already exists at $VENV. To reinstall, delete it and re-run."
  exit 0
fi

echo "▶ Creating Python venv at $VENV..."
python3.12 -m venv "$VENV"

echo "▶ Installing CrewAI + tools..."
"$VENV/bin/pip" install --upgrade pip --quiet
"$VENV/bin/pip" install 'crewai[tools]' anthropic openai --quiet

echo ""
echo "✓ CrewAI installed."
echo ""
echo "Usage:"
echo "  source $VENV/bin/activate"
echo "  python your_crew.py"
echo ""
echo "Or run a crew directly:"
echo "  $VENV/bin/python your_crew.py"
echo ""
echo "Letta REST base URL for crews:  http://localhost:8283"
echo "Verba REST base URL for RAG:    http://localhost:8001"
