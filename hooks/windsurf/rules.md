# TOK - Token Optimization Kit (Windsurf)

**Usage**: Token-optimized CLI proxy for shell commands.

## Rule

Always prefix shell commands with `tok` to minimize token consumption.

Examples:

```bash
tok git status
tok cargo test
tok ls src/
tok grep "pattern" src/
tok find "*.rs" .
tok docker ps
tok gh pr list
```

## Meta Commands

```bash
tok gain              # Show token savings
tok gain --history    # Command history with savings
tok discover          # Find missed TOK opportunities
tok proxy <cmd>       # Run raw (no filtering, for debugging)
```

## Why

TOK filters and compresses command output before it reaches the LLM context, saving 60-90% tokens on common operations. Always use `tok <cmd>` instead of raw commands.
