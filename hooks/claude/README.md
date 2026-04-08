# Claude Code Hooks

> Part of [`hooks/`](../README.md) — see also [`src/hooks/`](../../src/hooks/README.md) for installation code

## Specifics

- Shell-based `PreToolUse` hook -- requires `jq` for JSON parsing
- Returns `updatedInput` JSON for transparent command rewrite (agent doesn't know TOK is involved)
- Exits silently (exit 0) on any failure: jq missing, tok missing, tok too old (< 0.23.0), no match
- Version guard checks `tok --version` against minimum 0.23.0
- `tok-awareness.md` is a slim 10-line instructions file embedded into CLAUDE.md by `tok init`

## Testing

```bash
# Run the full test suite (60+ assertions)
bash hooks/test-tok-rewrite.sh

# Test against a specific hook path
HOOK=/path/to/tok-rewrite.sh bash hooks/test-tok-rewrite.sh

# Enable audit logging during testing
TOK_HOOK_AUDIT=1 TOK_AUDIT_DIR=/tmp bash hooks/test-tok-rewrite.sh
```
