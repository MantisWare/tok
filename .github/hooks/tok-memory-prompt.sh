#!/usr/bin/env bash
export TOK_CLIENT=copilot
# TOK agent memory — per-turn retrieval (UserPromptSubmit / BeforeAgent).
set -euo pipefail
: "${TOK_CLIENT:=auto}"

if ! command -v tok &>/dev/null; then
  echo '{}'
  exit 0
fi

INPUT="$(cat)"
if [ -z "$INPUT" ]; then
  exit 0
fi

printf '%s' "$INPUT" | tok hook memory-retrieve --json --stdin --agent "$TOK_CLIENT" --event user_prompt
