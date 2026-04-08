# TOK — Copilot Integration (VS Code Copilot Chat + Copilot CLI)

**Usage**: Token-optimized CLI proxy (60-90% savings on dev operations)

## What's automatic

The `.github/copilot-instructions.md` file is loaded at session start by both Copilot CLI and VS Code Copilot Chat.
It instructs Copilot to prefix commands with `tok` automatically.

The `.github/hooks/tok-rewrite.json` hook adds a `PreToolUse` safety net via `tok hook` —
a cross-platform Rust binary that intercepts raw bash tool calls and rewrites them.
No shell scripts, no `jq` dependency, works on Windows natively.

## Meta commands (always use directly)

```bash
tok gain              # Token savings dashboard for this session
tok gain --history    # Per-command history with savings %
tok discover          # Scan session history for missed tok opportunities
tok proxy <cmd>       # Run raw (no filtering) but still track it
```

## Installation verification

```bash
tok --version   # Should print: tok X.Y.Z
tok gain        # Should show a dashboard (not "command not found")
which tok       # Verify correct binary path
```

> ⚠️ **Different tool**: If `tok gain` fails, confirm you installed Token Optimization Kit. Rust Type Kit is `reachingforthejack/rtk` (usually `rtk`, not this CLI).
> (Rust Type Kit) installed instead. Check `which tok` and reinstall from tok-ai/tok.

## How the hook works

`tok hook` reads `PreToolUse` JSON from stdin, detects the agent format, and responds appropriately:

**VS Code Copilot Chat** (supports `updatedInput` — transparent rewrite, no denial):
1. Agent runs `git status` → `tok hook` intercepts via `PreToolUse`
2. `tok hook` detects VS Code format (`tool_name`/`tool_input` keys)
3. Returns `hookSpecificOutput.updatedInput.command = "tok git status"`
4. Agent runs the rewritten command silently — no denial, no retry

**GitHub Copilot CLI** (deny-with-suggestion — CLI ignores `updatedInput` today, see [issue #2013](https://github.com/github/copilot-cli/issues/2013)):
1. Agent runs `git status` → `tok hook` intercepts via `PreToolUse`
2. `tok hook` detects Copilot CLI format (`toolName`/`toolArgs` keys)
3. Returns `permissionDecision: deny` with reason: `"Token savings: use 'tok git status' instead"`
4. Copilot reads the reason and re-runs `tok git status`

When Copilot CLI adds `updatedInput` support, only `tok hook` needs updating — no config changes.

## Integration comparison

| Tool                  | Mechanism                               | Hook output              | File                               |
|-----------------------|-----------------------------------------|--------------------------|------------------------------------|
| Claude Code           | `PreToolUse` hook with `updatedInput`   | Transparent rewrite      | `hooks/tok-rewrite.sh`             |
| VS Code Copilot Chat  | `PreToolUse` hook with `updatedInput`   | Transparent rewrite      | `.github/hooks/tok-rewrite.json`   |
| GitHub Copilot CLI    | `PreToolUse` deny-with-suggestion       | Denial + retry           | `.github/hooks/tok-rewrite.json`   |
| OpenCode              | Plugin `tool.execute.before`            | Transparent rewrite      | `hooks/opencode-tok.ts`            |
| (any)                 | Custom instructions                     | Prompt-level guidance    | `.github/copilot-instructions.md`  |
