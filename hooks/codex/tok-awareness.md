# TOK - Token Optimization Kit (Codex CLI)

**Usage**: Token-optimized CLI proxy for shell commands.

## Rule

Always prefix shell commands with `tok`.

Examples:

```bash
tok git status
tok cargo test
tok npm run build
tok pytest -q
```

## Meta Commands

```bash
tok gain            # Token savings analytics
tok gain --history  # Recent command savings history
tok proxy <cmd>     # Run raw command without filtering
```

## Verification

```bash
tok --version
tok gain
which tok
```
