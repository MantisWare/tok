# TOK Plugin for OpenClaw

Transparently rewrites shell commands executed via OpenClaw's `exec` tool to their TOK equivalents, achieving 60-90% LLM token savings.

This is the OpenClaw equivalent of the Claude Code hooks in `hooks/tok-rewrite.sh`.

## How it works

The plugin registers a `before_tool_call` hook that intercepts `exec` tool calls. When the agent runs a command like `git status`, the plugin delegates to `tok rewrite` which returns the optimized command (e.g. `tok git status`). The compressed output enters the agent's context window, saving tokens.

All rewrite logic lives in TOK itself (`tok rewrite`). This plugin is a thin delegate -- when new filters are added to TOK, the plugin picks them up automatically with zero changes.

## Installation

### Prerequisites

TOK must be installed and available in `$PATH`:

```bash
brew install tok
# or
curl -fsSL https://raw.githubusercontent.com/MantisWare/tok/refs/heads/master/install.sh | sh
```

### Install the plugin

```bash
# Copy the plugin to OpenClaw's extensions directory
mkdir -p ~/.openclaw/extensions/tok-rewrite
cp openclaw/index.ts openclaw/openclaw.plugin.json ~/.openclaw/extensions/tok-rewrite/

# Restart the gateway
openclaw gateway restart
```

### Or install via OpenClaw CLI

```bash
openclaw plugins install ./openclaw
```

## Configuration

In `openclaw.json`:

```json5
{
  plugins: {
    entries: {
      "tok-rewrite": {
        enabled: true,
        config: {
          enabled: true,    // Toggle rewriting on/off
          verbose: false     // Log rewrites to console
        }
      }
    }
  }
}
```

## What gets rewritten

Everything that `tok rewrite` supports (30+ commands). See the [full command list](https://github.com/MantisWare/tok#commands).

## What's NOT rewritten

Handled by `tok rewrite` guards:
- Commands already using `tok`
- Piped commands (`|`, `&&`, `;`)
- Heredocs (`<<`)
- Commands without an TOK filter

## Measured savings

| Command | Token savings |
|---------|--------------|
| `git log --stat` | 87% |
| `ls -la` | 78% |
| `git status` | 66% |
| `grep` (single file) | 52% |
| `find -name` | 48% |

## License

MIT -- same as TOK.
