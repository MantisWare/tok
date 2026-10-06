# LLM Agent Hooks

## Scope

**Deployed hook artifacts** — the actual files installed on user machines by `tok init`. These are shell scripts, TypeScript plugins, and rules files that run outside the Rust binary. They are **thin delegates**: parse agent-specific JSON, call `tok rewrite` as a subprocess, format agent-specific response. Zero filtering logic lives here.

Owns: per-agent hook scripts and configuration files for 7 supported agents (Claude Code, Copilot, Cursor, Cline, Windsurf, Codex, OpenCode).

Does **not** own: hook installation/uninstallation (that's `src/hooks/init.rs`), the rewrite pattern registry (that's `discover/registry`), or integrity verification (that's `src/hooks/integrity.rs`).

Relationship to `src/hooks/`: that component **creates** these files; this directory **contains** them.

## Purpose

LLM agent integrations that intercept CLI commands and route them through TOK for token optimization. Each hook transparently rewrites raw commands (e.g., `git status`) to their TOK equivalents (e.g., `tok git status`), delivering 60-90% token savings without requiring the agent or user to change their workflow.

## How It Works

```
Agent runs command (e.g., "cargo test --nocapture")
  -> Hook intercepts (PreToolUse / plugin event)
  -> Reads JSON input, extracts command string
  -> Calls `tok rewrite "cargo test --nocapture"`
  -> Registry matches pattern, returns "tok cargo test --nocapture"
  -> Hook sends response in agent-specific JSON format
  -> Agent executes "tok cargo test --nocapture" instead
  -> Filtered output reaches LLM (~90% fewer tokens)
```

All rewrite logic lives in the Rust binary (`src/discover/registry.rs`). Hook scripts are **thin delegates** that handle agent-specific JSON formats and call `tok rewrite` for the actual decision. This ensures a single source of truth for all 70+ rewrite patterns.

**Agent memory** (`tok memory`, `src/agent_memory/`): separate from structural `tok mem`. After `tok init -g`, memory is enabled by default. `sessionStart` hooks call `tok hook memory-retrieve --json` to inject rules/preferences into context; post-turn hooks can call `tok hook memory-extract` with user/assistant JSON on stdin. See [docs/TOK_Memory_Gateway_Mem0_Inspired_Architecture.md](../docs/TOK_Memory_Gateway_Mem0_Inspired_Architecture.md).

**Code graph** (`src/graph/`, `src/query/`): also installed by `tok init`, and also on by default. Three hooks keep it useful without the agent having to ask:

| Hook | Event | What it does |
| --- | --- | --- |
| `tok hook graph-session` | `SessionStart` | Injects repo orientation — layout, hubs, entry points — using the same `additional_context` contract as memory |
| `tok hook graph-postedit` | `PostToolUse` on edits | Refreshes the graph so the next query sees the change. No-ops on a repo that was never indexed |
| `tok hook graph-sync` | manual / CI | Regenerates the committed `.tok/map/` cards and reports drift |

Alongside the hooks, `tok init` registers `tok mcp` as an MCP server so the agent can call the graph directly rather than shelling out. `tok init --no-graph` skips the hooks, the registration, and the instruction section; `tok init --uninstall` removes all three. Re-running `tok init` rewrites a registration that points at an old binary and leaves any other MCP server in the same config untouched.

`TOK_GRAPH_NO_REFRESH=1` stops the hooks and queries from rebuilding the graph, which is what you want in CI.

## Directory Structure

Each agent subdirectory has its own README with hook-specific details:

- **[`claude/`](claude/README.md)** — Shell hook, `PreToolUse` JSON format, `settings.json` patching, test script
- **[`copilot/`](copilot/README.md)** — Rust binary hook, dual format (VS Code Chat vs Copilot CLI), deny-with-suggestion fallback
- **[`cursor/`](cursor/README.md)** — Shell hooks (`preToolUse` rewrite + `sessionStart` awareness), Cursor JSON format
- **[`cline/`](cline/README.md)** — Rules file (prompt-level), `.clinerules` project-local installation
- **[`windsurf/`](windsurf/README.md)** — Rules file (prompt-level), `.windsurfrules` workspace-scoped
- **[`codex/`](codex/README.md)** — Awareness document, `AGENTS.md` integration, `~/.codex/` location
- **[`opencode/`](opencode/README.md)** — TypeScript plugin, `zx` library, `tool.execute.before` event, in-place mutation

## Supported Agents

| Agent | Mechanism | Hook Type | Can Modify Command? |
|-------|-----------|-----------|---------------------|
| Claude Code | Shell hook (`PreToolUse`) | Transparent rewrite | Yes (`updatedInput`) |
| VS Code Copilot Chat | Rust binary (`tok hook copilot`) | Transparent rewrite | Yes (`updatedInput`) |
| GitHub Copilot CLI | Rust binary (`tok hook copilot`) | Deny-with-suggestion | No (agent retries) |
| Cursor | Shell hooks (`preToolUse` + `sessionStart`) | Rewrite + agent memory inject | Yes (`updated_input` / `additional_context`) |
| Gemini CLI | Rust binary (`tok hook gemini`) | Transparent rewrite | Yes (`hookSpecificOutput`) |
| Cline / Roo Code | Custom instructions (rules file) | Prompt-level guidance | N/A |
| Windsurf | Custom instructions (rules file) | Prompt-level guidance | N/A |
| Codex CLI | AGENTS.md / instructions | Prompt-level guidance | N/A |
| OpenCode | TypeScript plugin (`tool.execute.before`) | In-place mutation | Yes |

## JSON Formats by Agent

### Claude Code (Shell Hook)

**Input** (stdin):
```json
{
  "tool_name": "Bash",
  "tool_input": { "command": "git status" }
}
```

**Output** (stdout, when rewritten):
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow",
    "permissionDecisionReason": "TOK auto-rewrite",
    "updatedInput": { "command": "tok git status" }
  }
}
```

### Cursor (Shell Hook)

**Input**: Same as Claude Code.

**Output** (stdout, when rewritten):
```json
{
  "permission": "allow",
  "updated_input": { "command": "tok git status" }
}
```

Returns `{}` when no rewrite (Cursor requires JSON for all paths).

### Copilot CLI (Rust Binary)

**Input** (stdin, camelCase, `toolArgs` is JSON-stringified):
```json
{
  "toolName": "bash",
  "toolArgs": "{\"command\": \"git status\"}"
}
```

**Output** (no `updatedInput` support -- uses deny-with-suggestion):
```json
{
  "permissionDecision": "deny",
  "permissionDecisionReason": "Token savings: use `tok git status` instead"
}
```

### VS Code Copilot Chat (Rust Binary)

**Input** (stdin, snake_case):
```json
{
  "tool_name": "Bash",
  "tool_input": { "command": "git status" }
}
```

**Output**: Same as Claude Code format (with `updatedInput`).

### Gemini CLI (Rust Binary)

**Input** (stdin):
```json
{
  "tool_name": "run_shell_command",
  "tool_input": { "command": "git status" }
}
```

**Output** (when rewritten):
```json
{
  "decision": "allow",
  "hookSpecificOutput": {
    "tool_input": { "command": "tok git status" }
  }
}
```

**No rewrite**: `{"decision": "allow"}`

### OpenCode (TypeScript Plugin)

Mutates `args.command` in-place via the zx library:
```typescript
const result = await $`tok rewrite ${command}`.quiet().nothrow()
const rewritten = String(result.stdout).trim()
if (rewritten && rewritten !== command) {
  (args as Record<string, unknown>).command = rewritten
}
```

## Command Rewrite Registry

The registry (`src/discover/registry.rs`) handles command patterns across these categories:

| Category | Examples | Savings |
|----------|----------|---------|
| Test Runners | vitest, pytest, cargo test, go test, playwright | 90-99% |
| Build Tools | cargo build, npm, pnpm, dotnet, make | 70-90% |
| VCS | git status/log/diff/show | 70-80% |
| Language Servers | tsc, mypy | 80-83% |
| Linters | eslint, ruff, golangci-lint, biome | 80-85% |
| Package Managers | pip, cargo install, pnpm list | 75-80% |
| File Operations | ls, find, grep, cat, head, tail | 60-75% |
| Infrastructure | docker, kubectl, aws, terraform | 75-85% |

### Compound Command Handling

The registry handles `&&`, `||`, `;`, `|`, and `&` operators:

- **Pipe** (`|`): Only the left side is rewritten (right side consumes output format)
- **And/Or/Semicolon** (`&&`, `||`, `;`): Both sides rewritten independently
- **find/fd in pipes**: Never rewritten (output format incompatible with xargs/wc/grep)

Example: `cargo fmt --all && cargo test` becomes `tok cargo fmt --all && tok cargo test`

### Override Controls

- **`TOK_DISABLED=1`**: Per-command override (`TOK_DISABLED=1 git status` runs raw)
- **`exclude_commands`**: In `~/.config/tok/config.toml`, list commands to never rewrite
- **Already-TOK**: `tok git status` passes through unchanged (no `tok tok git`)

## Exit Code Contract

Hooks must **never block command execution**. All error paths (missing binary, bad JSON, rewrite failure) must exit 0 so the agent's command runs unmodified. A hook that exits non-zero prevents the user's command from executing.

When there is no rewrite to apply, the hook must produce no output (or `{}` for Cursor, which requires JSON on all paths).

### Gaps (to be fixed)

- `hook_cmd.rs::run_gemini()` — exits 1 on invalid JSON input instead of exit 0

## Graceful Degradation

Hooks are **non-blocking** -- they never prevent a command from executing:

- jq not installed: warning to stderr, exit 0 (command runs raw)
- tok binary not found: warning to stderr, exit 0
- tok version too old (< 0.23.0): warning to stderr, exit 0
- Invalid JSON input: pass through unchanged
- `tok rewrite` crashes: hook exits 0 (subprocess error ignored)
- Filter logic error: fallback to raw command output

## Adding a New Agent Integration

New integrations must follow the [Exit Code Contract](#exit-code-contract) and [Graceful Degradation](#graceful-degradation) above, as well as the project's [Design Philosophy](../CONTRIBUTING.md#design-philosophy).

### Integration Tiers

| Tier | Mechanism | Maintenance | Examples |
|------|-----------|-------------|----------|
| **Full hook** | Shell script or Rust binary, intercepts commands via agent's hook API | High — must track agent API changes | Claude Code, Cursor, Copilot, Gemini |
| **Plugin** | TypeScript/JS plugin in agent's plugin system | Medium — agent manages loading | OpenCode |
| **Rules file** | Prompt-level instructions the agent reads | Low — no code to break | Cline, Windsurf, Codex |

### Eligibility

TOK supports AI coding assistants that developers actually use day-to-day. To add a new agent:

- Agent has a **documented, stable hook/plugin API** (not experimental/alpha)
- Agent is **actively maintained** (commit activity in last 3 months)
- Integration follows the **exit code contract** (exit 0 on all error paths)
- Hook output matches the **agent's expected JSON format** exactly

### Maintenance

If an agent's API changes and the hook breaks, the integration should be updated promptly. If the agent becomes unmaintained or the hook can't be fixed, the integration may be deprecated with a release note.

