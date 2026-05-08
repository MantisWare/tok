#!/usr/bin/env bash
# tok-hook-version: 2
# TOK Cursor Agent hook — rewrites shell commands to use tok for token savings.
# Works with both Cursor editor and cursor-cli (they share ~/.cursor/hooks.json).
# Cursor preToolUse hook format: receives JSON on stdin, returns JSON on stdout.
# Requires: tok, jq
#
# This is a thin delegating hook: all rewrite logic lives in `tok rewrite`,
# which is the single source of truth (src/discover/registry.rs).
# To add or change rewrite rules, edit the Rust registry — not this file.
#
# Exit code protocol for `tok rewrite`:
#   0 + stdout  Rewrite found, no deny/ask rule matched → auto-allow
#   1           No TOK equivalent → pass through unchanged
#   2           Deny rule matched → pass through unchanged
#   3 + stdout  Ask rule matched → rewrite but let the agent prompt the user

if ! command -v jq &>/dev/null; then
  echo "[tok] WARNING: jq is not installed. Hook cannot rewrite commands. Install jq: https://jqlang.github.io/jq/download/" >&2
  exit 0
fi

if ! command -v tok &>/dev/null; then
  echo "[tok] WARNING: tok is not installed or not in PATH. Hook cannot rewrite commands. Install: https://github.com/MantisWare/tok#installation" >&2
  exit 0
fi

# Verify tok has the rewrite subcommand (added in 0.1.9).
if ! tok rewrite --help &>/dev/null; then
  echo "[tok] WARNING: tok $(tok --version 2>/dev/null) does not support 'rewrite'. Upgrade: cargo install tok" >&2
  exit 0
fi

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

if [ -z "$CMD" ]; then
  echo '{}'
  exit 0
fi

# Delegate all rewrite + permission logic to the Rust binary.
REWRITTEN=$(tok rewrite "$CMD" 2>/dev/null)
RC=$?

case $RC in
  0)
    # Rewrite found, no permission rules matched — safe to auto-allow.
    [ "$CMD" = "$REWRITTEN" ] && { echo '{}'; exit 0; }
    ;;
  3)
    # Ask rule matched — rewrite the command, allow it (Cursor handles
    # its own permission model; there is no "ask" passthrough like Claude Code).
    ;;
  *)
    # 1 = no TOK equivalent, 2 = deny, other = unexpected — pass through.
    echo '{}'
    exit 0
    ;;
esac

jq -n --arg cmd "$REWRITTEN" '{
  "permission": "allow",
  "updated_input": { "command": $cmd }
}'
