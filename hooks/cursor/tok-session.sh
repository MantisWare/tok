#!/usr/bin/env bash
# TOK Cursor sessionStart hook — injects tok awareness into conversation context.
# Installed by `tok init -g --agent cursor`.

cat > /dev/null

if ! command -v tok &>/dev/null; then
  echo '{}'
  exit 0
fi

VERSION=$(tok --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)

cat <<EOF
{
  "additional_context": "TOK (Token Optimization Kit v${VERSION:-unknown}) is installed. Meta commands: tok gain (savings analytics), tok gain --history (command history), tok proxy <cmd> (raw output). All shell commands are automatically rewritten by the preToolUse hook (e.g. git status becomes tok git status). Verification: tok --version, tok gain, which tok."
}
EOF
