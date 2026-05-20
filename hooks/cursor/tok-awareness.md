# TOK - Token Optimization Kit (Cursor)

**Usage**: Token-optimized CLI proxy (60-90% savings on dev operations)

## Meta Commands (always use tok directly)

```bash
tok gain              # Show token savings analytics
tok gain --history    # Show command usage history with savings
tok proxy <cmd>       # Execute raw command without filtering (for debugging)
```

## Hook-Based Usage

All shell commands are automatically rewritten by the Cursor `preToolUse` hook.
Example: `git status` → `tok git status` (transparent, zero tokens overhead)

The hook intercepts Shell tool calls, delegates to `tok rewrite`, and returns
Cursor's `updated_input` JSON so the rewritten command runs silently.

At **session start**, `sessionStart` runs `tok hook memory-retrieve --json` to inject
agent memory (rules, preferences, project facts) into `additional_context`.

## Agent Memory (`tok memory` — not `tok mem`)

| Command | Purpose |
|---------|---------|
| `tok mem …` | Structural **code** memory (symbols, callers, impact) |
| `tok memory …` | **Agent** memory (rules, prefs, facts you told the IDE) |

Enabled by default after `tok init -g --agent cursor`.

```bash
tok memory status
tok memory add "Use Cursor-ready markdown for specs" --type rule
tok memory search "markdown"
tok memory inspect-context "current task"
tok memory list --type rule
tok memory on / off
tok memory extraction false   # disable auto-learn, keep inject
tok hook memory-retrieve --json
tok hook memory-extract       # post-turn: {"user":"...","assistant":"..."}
```

## Installation Verification

```bash
tok --version         # Should show: tok X.Y.Z
tok gain              # Should work (not "command not found")
tok memory status     # Agent memory DB (enabled after tok init -g)
which tok             # Verify correct binary
```

> **Different tool**: If `tok gain` fails, confirm you installed Token Optimization Kit (MantisWare/tok). Rust Type Kit is a different project (`reachingforthejack/rtk`, usually the `rtk` command).
