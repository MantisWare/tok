# Cursor IDE Hooks

> Part of [`hooks/`](../README.md) — see also [`src/hooks/`](../../src/hooks/README.md) for installation code

## Files

| File | Hook Event | Purpose |
|------|-----------|---------|
| `tok-rewrite.sh` | `preToolUse` | Rewrites shell commands to use tok (e.g. `git status` → `tok git status`) |
| `tok-session.sh` | `sessionStart` | Injects tok awareness into conversation context so the AI knows about meta commands |
| `tok-awareness.md` | — | Source content for awareness (embedded in `tok-session.sh` output) |

## Specifics

- Same delegating pattern as Claude Code hook but outputs Cursor's JSON format (`permission`/`updated_input` instead of `hookSpecificOutput`/`updatedInput`)
- Returns `{}` (empty JSON) when no rewrite applies -- Cursor requires JSON output for all code paths
- Requires `jq` and `tok` with the `rewrite` subcommand (capability-checked at runtime)
- `sessionStart` hook injects `additional_context` so the AI agent knows about `tok gain`, `tok proxy`, and other meta commands
