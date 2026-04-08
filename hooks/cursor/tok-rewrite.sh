#!/usr/bin/env bash
# tok-hook-version: 1
# TOK Cursor Agent hook — rewrites shell commands to use tok for token savings.
# Works with both Cursor editor and cursor-cli (they share ~/.cursor/hooks.json).
# Cursor preToolUse hook format: receives JSON on stdin, returns JSON on stdout.
# Requires: tok >= 0.23.0, jq
#
# This is a thin delegating hook: all rewrite logic lives in `tok rewrite`,
# which is the single source of truth (src/discover/registry.rs).
# To add or change rewrite rules, edit the Rust registry — not this file.

if ! command -v jq &>/dev/null; then
  echo "[tok] WARNING: jq is not installed. Hook cannot rewrite commands. Install jq: https://jqlang.github.io/jq/download/" >&2
  exit 0
fi

if ! command -v tok &>/dev/null; then
  echo "[tok] WARNING: tok is not installed or not in PATH. Hook cannot rewrite commands. Install: https://github.com/MantisWare/tok#installation" >&2
  exit 0
fi

# Version guard: tok rewrite was added in 0.23.0.
TOK_VERSION=$(tok --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
if [ -n "$TOK_VERSION" ]; then
  MAJOR=$(echo "$TOK_VERSION" | cut -d. -f1)
  MINOR=$(echo "$TOK_VERSION" | cut -d. -f2)
  if [ "$MAJOR" -eq 0 ] && [ "$MINOR" -lt 23 ]; then
    echo "[tok] WARNING: tok $TOK_VERSION is too old (need >= 0.23.0). Upgrade: cargo install tok" >&2
    exit 0
  fi
fi

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

if [ -z "$CMD" ]; then
  echo '{}'
  exit 0
fi

# Delegate all rewrite logic to the Rust binary.
# tok rewrite exits 1 when there's no rewrite — hook passes through silently.
REWRITTEN=$(tok rewrite "$CMD" 2>/dev/null) || { echo '{}'; exit 0; }

# No change — nothing to do.
if [ "$CMD" = "$REWRITTEN" ]; then
  echo '{}'
  exit 0
fi

jq -n --arg cmd "$REWRITTEN" '{
  "permission": "allow",
  "updated_input": { "command": $cmd }
}'
