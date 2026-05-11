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
  "additional_context": "TOK (Token Optimization Kit v${VERSION:-unknown}) is installed. All shell commands are automatically rewritten by the preToolUse hook (e.g. git status becomes tok git status). TOK filters 60+ tools: git, cargo, npm, pnpm, docker, kubectl, go, pytest, ruff, vitest, playwright, prisma, tsc, eslint, and more. Analytics: tok gain (savings stats), tok gain --graph (daily chart), tok gain --history (command log), tok discover (missed opportunities), tok session (cross-session stats), tok cc-economics (Claude spend vs savings), tok learn (past CLI fixes). Code intelligence: tok mem index/search/find/context/impact/dead-code/changes (structural code memory), tok forgemap init/check/manifest (source annotation engine). Security: tok --security <cmd> (obfuscate sensitive data), tok security-inspect <text> (dry-run), tok doctor --slm (SLM health). Config: tok config, tok verify, tok trust/untrust, tok proxy <cmd> (raw passthrough). Reference: tok man (full command manual), tok man <topic> (filtered). Flags: -u (ultra-compact), --security-mode <mode>. Verification: tok --version, tok gain, which tok."
}
EOF
