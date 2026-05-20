#!/usr/bin/env bash
# TOK agent memory — cache user prompt for Cursor beforeSubmitPrompt pairing.
set -euo pipefail
: "${TOK_CLIENT:=cursor}"

if ! command -v tok &>/dev/null; then
  exit 0
fi

INPUT="$(cat)"
if [ -z "$INPUT" ]; then
  exit 0
fi

printf '%s' "$INPUT" | tok hook memory-cache-prompt --stdin --agent "$TOK_CLIENT"
