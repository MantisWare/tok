# Cursor IDE Hooks

> Part of [`hooks/`](../README.md) — see also [`src/hooks/`](../../src/hooks/README.md) for installation code

## Files

| File | Hook Event | Purpose |
|------|-----------|---------|
| `tok-rewrite.sh` | `preToolUse` | Rewrites shell commands to use tok (e.g. `git status` → `tok git status`) |
| `tok-session.sh` | `sessionStart` | Injects tok awareness + **agent memory** context (`tok hook memory-retrieve`) |
| `tok-awareness.md` | — | Reference for session hook content and `tok memory` vs `tok mem` |

## Specifics

- Same delegating pattern as Claude Code hook but outputs Cursor's JSON format (`permission`/`updated_input` instead of `hookSpecificOutput`/`updatedInput`)
- Returns `{}` (empty JSON) when no rewrite applies -- Cursor requires JSON output for all code paths
- Requires `jq` and `tok` with the `rewrite` subcommand (capability-checked at runtime)
- `sessionStart` hook injects `additional_context`: tok awareness plus **agent memory** from `tok hook memory-retrieve --json` (rules, preferences, project facts)
- Agent memory is **on by default** after `tok init -g`; disable with `tok memory off`
- Post-turn extraction: `tok hook memory-extract` with JSON `{"user":"...","assistant":"..."}` on stdin (wire to `stop` / `sessionEnd` when available)
