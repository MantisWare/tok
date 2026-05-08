# TOK - Token Optimization Kit (Cursor)

**Usage**: Token-optimized CLI proxy (60-90% savings on dev operations)

## Meta Commands (always use tok directly)

```bash
tok gain              # Show token savings analytics
tok gain --history    # Show command usage history with savings
tok proxy <cmd>       # Execute raw command without filtering (for debugging)
```

## Hook-Based Usage

All shell commands are automatically rewritten by the Cursor preToolUse hook.
Example: `git status` → `tok git status` (transparent, zero tokens overhead)

The hook intercepts Shell tool calls, delegates to `tok rewrite`, and returns
Cursor's `updated_input` JSON so the rewritten command runs silently.

## Installation Verification

```bash
tok --version         # Should show: tok X.Y.Z
tok gain              # Should work (not "command not found")
which tok             # Verify correct binary
```

> **Different tool**: If `tok gain` fails, confirm you installed Token Optimization Kit (MantisWare/tok). Rust Type Kit is a different project (`reachingforthejack/rtk`, usually the `rtk` command).
