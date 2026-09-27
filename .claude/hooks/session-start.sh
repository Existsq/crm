#!/bin/bash
# SessionStart hook (Claude Code on the web): project memory via graphify.
#
# 1. Installs the graphify CLI (PyPI package `graphifyy`, with SQL grammar)
#    when it is missing, so the PreToolUse `graphify hook-guard` entries in
#    .claude/settings.json resolve.
# 2. When the derived knowledge graph (graphify-out/graph.json, ~80 MB and
#    therefore not committed) is missing, rebuilds it in the background with
#    `graphify update .` (AST-only, no API key). Session start is not blocked;
#    progress goes to graphify-out/update.log.
#
# Durable, hand-curated memory lives in git: graphify-out/memory/ (saved
# query results), .specs/ (feature specs + STATE.md), docs/crm/.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(pwd)}"

export PATH="$HOME/.local/bin:$PATH"
echo "export PATH=\"\$HOME/.local/bin:\$PATH\"" >> "${CLAUDE_ENV_FILE:-/dev/null}"

if ! command -v graphify >/dev/null 2>&1; then
  if command -v uv >/dev/null 2>&1; then
    uv tool install --quiet 'graphifyy[sql]' >/dev/null 2>&1 \
      || echo "graphify: uv install failed; knowledge graph unavailable this session" >&2
  else
    python3 -m pip install --quiet --user 'graphifyy[sql]' >/dev/null 2>&1 \
      || echo "graphify: pip install failed; knowledge graph unavailable this session" >&2
  fi
fi

if command -v graphify >/dev/null 2>&1 && [ ! -f graphify-out/graph.json ]; then
  mkdir -p graphify-out
  nohup graphify update . > graphify-out/update.log 2>&1 < /dev/null &
  echo "graphify: rebuilding knowledge graph in background (graphify-out/update.log)"
fi
