#!/usr/bin/env bash
# TOK agent memory — sessionStart / SessionStart injection.
# Requires: tok, jq (optional; only needed when composing with other shell hooks)
set -euo pipefail
: "${TOK_CLIENT:=auto}"

if ! command -v tok &>/dev/null; then
  echo '{"additional_context":""}'
  exit 0
fi

INPUT="$(cat)"
if [ -z "$INPUT" ]; then
  INPUT='{}'
fi

printf '%s' "$INPUT" | tok hook memory-retrieve --json --stdin --agent "$TOK_CLIENT" --event session_start
