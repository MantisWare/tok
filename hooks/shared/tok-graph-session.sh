#!/usr/bin/env bash
# TOK code graph — sessionStart orientation.
# Emits {"additional_context": "..."} describing the repository shape so the
# agent starts with a map instead of reading files to build one.
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

# Reads the cached graph only. Session start is in front of the user's first
# prompt, so it must not wait for an index build.
printf '%s' "$INPUT" | tok hook graph-session --json --stdin --agent "$TOK_CLIENT"
