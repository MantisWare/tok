#!/usr/bin/env bash
# TOK agent memory — post-turn extraction (Stop / AfterAgent / afterAgentResponse).
set -euo pipefail
: "${TOK_CLIENT:=auto}"

if ! command -v tok &>/dev/null; then
  exit 0
fi

INPUT="$(cat)"
if [ -z "$INPUT" ]; then
  exit 0
fi

printf '%s' "$INPUT" | tok hook memory-extract --stdin --agent "$TOK_CLIENT"
