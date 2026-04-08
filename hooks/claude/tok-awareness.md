# TOK - Token Optimization Kit

**Usage**: Token-optimized CLI proxy (60-90% savings on dev operations)

## Meta Commands (always use tok directly)

```bash
tok gain              # Show token savings analytics
tok gain --history    # Show command usage history with savings
tok discover          # Analyze Claude Code history for missed opportunities
tok proxy <cmd>       # Execute raw command without filtering (for debugging)
```

## Installation Verification

```bash
tok --version         # Should show: tok X.Y.Z
tok gain              # Should work (not "command not found")
which tok             # Verify correct binary
```

⚠️ **Different tool**: If `tok gain` fails, confirm you installed Token Optimization Kit. Rust Type Kit is `reachingforthejack/rtk` (usually the `rtk` command).

## Hook-Based Usage

All other commands are automatically rewritten by the Claude Code hook.
Example: `git status` → `tok git status` (transparent, 0 tokens overhead)

Refer to CLAUDE.md for full command reference.
