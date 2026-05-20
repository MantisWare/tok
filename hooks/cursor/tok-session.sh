#!/usr/bin/env bash
# TOK Cursor sessionStart hook — injects tok awareness + agent memory context.
# Installed by `tok init -g --agent cursor`.
# Requires: tok, jq

set -euo pipefail
export TOK_CLIENT=cursor

if ! command -v tok &>/dev/null; then
  echo '{}'
  exit 0
fi

INPUT="$(cat)"
if [ -z "$INPUT" ]; then
  INPUT='{}'
fi

VERSION=$(tok --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)

BASE_CONTEXT="TOK (Token Optimization Kit v${VERSION:-unknown}) is installed. All shell commands are automatically rewritten by the preToolUse hook (e.g. git status becomes tok git status). TOK filters 60+ tools: git, cargo, npm, pnpm, docker, kubectl, go, pytest, ruff, vitest, playwright, prisma, tsc, eslint, and more. Analytics: tok gain (savings stats), tok gain --graph (daily chart), tok gain --history (command log), tok discover (missed opportunities), tok session (cross-session stats), tok cc-economics (Claude spend vs savings), tok learn (past CLI fixes). Code intelligence: tok mem index/search/find/context/impact/dead-code/changes (structural code memory), tok memory status/list/search (agent memory for rules and preferences), tok forgemap init/check/manifest (source annotation engine). Security: tok --security <cmd> (obfuscate sensitive data), tok security-inspect <text> (dry-run), tok doctor --slm (SLM health). Config: tok config, tok verify, tok trust/untrust, tok proxy <cmd> (raw passthrough). Reference: tok man (full command manual), tok man <topic> (filtered). Flags: -u (ultra-compact), --security-mode <mode>. Verification: tok --version, tok gain, which tok."

MEMORY_BLOCK=""
if command -v jq &>/dev/null; then
  MEMORY_JSON=$(printf '%s' "$INPUT" | tok hook memory-retrieve --json --stdin --agent cursor --event session_start 2>/dev/null || echo '{}')
  MEMORY_BLOCK=$(printf '%s' "$MEMORY_JSON" | jq -r '.additional_context // empty' 2>/dev/null || true)
else
  MEMORY_BLOCK=$(printf '%s' "$INPUT" | tok hook memory-retrieve --stdin --agent cursor --event session_start 2>/dev/null || true)
fi

if [ -n "$MEMORY_BLOCK" ]; then
  COMBINED="${BASE_CONTEXT}

${MEMORY_BLOCK}"
else
  COMBINED="$BASE_CONTEXT"
fi

if command -v jq &>/dev/null; then
  jq -n --arg ctx "$COMBINED" '{additional_context: $ctx}'
else
  printf '{"additional_context":%s}\n' "$(printf '%s' "$COMBINED" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')"
fi
