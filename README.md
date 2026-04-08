<p align="center">
  <img src="https://avatars.githubusercontent.com/u/258253854?v=4" alt="TOK - Token Optimization Kit" width="500">
</p>

<p align="center">
  <strong>CLI proxy that shaves 60–90% off the tokens your LLM eats — same commands, less wallpaper</strong>
</p>

<p align="center">
  <a href="https://github.com/tok-ai/tok/actions"><img src="https://github.com/tok-ai/tok/workflows/Security%20Check/badge.svg" alt="CI"></a>
  <a href="https://github.com/tok-ai/tok/releases"><img src="https://img.shields.io/github/v/release/tok-ai/tok" alt="Release"></a>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  <a href="https://discord.gg/RySmvNF5kF"><img src="https://img.shields.io/discord/1470188214710046894?label=Discord&logo=discord" alt="Discord"></a>
  <a href="https://formulae.brew.sh/formula/tok"><img src="https://img.shields.io/homebrew/v/tok" alt="Homebrew"></a>
</p>

<p align="center">
  <a href="https://www.tok-ai.app">Website</a> &bull;
  <a href="#installation">Install</a> &bull;
  <a href="docs/TROUBLESHOOTING.md">Troubleshooting</a> &bull;
  <a href="docs/contributing/DEVELOPMENT.md">Development</a> &bull;
  <a href="docs/contributing/ARCHITECTURE.md">Architecture</a> &bull;
  <a href="https://discord.gg/RySmvNF5kF">Discord</a>
</p>

---

**tok** sits between your shell and your model: it filters, groups, and squashes command output so the assistant sees signal, not noise. One Rust binary, 100+ commands, under ~10ms of overhead — basically a bouncer for your terminal.

## Token Savings (30-min Claude Code Session)

| Operation | Frequency | Standard | tok | Savings |
|-----------|-----------|----------|-----|---------|
| `ls` / `tree` | 10x | 2,000 | 400 | -80% |
| `cat` / `read` | 20x | 40,000 | 12,000 | -70% |
| `grep` / `rg` | 8x | 16,000 | 3,200 | -80% |
| `git status` | 10x | 3,000 | 600 | -80% |
| `git diff` | 5x | 10,000 | 2,500 | -75% |
| `git log` | 5x | 2,500 | 500 | -80% |
| `git add/commit/push` | 8x | 1,600 | 120 | -92% |
| `cargo test` / `npm test` | 5x | 25,000 | 2,500 | -90% |
| `ruff check` | 3x | 3,000 | 600 | -80% |
| `pytest` | 4x | 8,000 | 800 | -90% |
| `go test` | 3x | 6,000 | 600 | -90% |
| `docker ps` | 3x | 900 | 180 | -80% |
| **Total** | | **~118,000** | **~23,900** | **-80%** |

> Estimates based on medium-sized TypeScript/Rust projects. Actual savings vary by project size.

## Installation

### Homebrew (recommended)

```bash
brew install tok
```

### Quick Install (Linux/macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/tok-ai/tok/refs/heads/master/install.sh | sh
```

> Installs to `~/.local/bin`. Add to PATH if needed:
> ```bash
> echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc  # or ~/.zshrc
> ```

### Cargo

```bash
cargo install --git https://github.com/tok-ai/tok
```

### Pre-built Binaries

Download from [releases](https://github.com/tok-ai/tok/releases):
- macOS: `tok-x86_64-apple-darwin.tar.gz` / `tok-aarch64-apple-darwin.tar.gz`
- Linux: `tok-x86_64-unknown-linux-musl.tar.gz` / `tok-aarch64-unknown-linux-gnu.tar.gz`
- Windows: `tok-x86_64-pc-windows-msvc.zip`

### Verify Installation

```bash
tok --version   # Should show "tok 0.1.0"
tok gain        # Should show token savings stats
```

> **Name collision warning**: Another project named "tok" (Rust Type Kit) exists on crates.io. If `tok gain` fails, you have the wrong package. Use `cargo install --git` above instead.

## Quick Start

```bash
# 1. Install for your AI tool
tok init -g                     # Claude Code / Copilot (default)
tok init -g --gemini            # Gemini CLI
tok init -g --codex             # Codex (OpenAI)
tok init -g --agent cursor      # Cursor
tok init --agent windsurf       # Windsurf
tok init --agent cline          # Cline / Roo Code

# 2. Restart your AI tool, then test
git status  # Automatically rewritten to tok git status
```

The hook quietly rewrites Bash commands (e.g. `git status` → `tok git status`) before they run. Your agent never sees the swap — it just gets the skinny output.

**Heads-up:** the hook only touches **Bash** tool calls. Claude Code’s built-in `Read`, `Grep`, and `Glob` skip that path, so no auto-rewrite there. Want compact output anyway? Use shell (`cat`/`head`/`tail`, `rg`/`grep`, `find`) or call `tok read`, `tok grep`, or `tok find` yourself.

## How It Works

```
  Without tok:                                    With tok:

  Claude  --git status-->  shell  -->  git         Claude  --git status-->  TOK  -->  git
    ^                                   |            ^                      |          |
    |        ~2,000 tokens (raw)        |            |   ~200 tokens        | filter   |
    +-----------------------------------+            +------- (filtered) ---+----------+
```

Under the hood we mix four moves (pick what fits each command):

1. **Smart filtering** — drop comments, padding, and boilerplate
2. **Grouping** — pile similar lines together (dirs, error types, …)
3. **Truncation** — keep context, ditch repeats
4. **Deduplication** — “same line ×47” becomes one line + a count

## Commands

### Files
```bash
tok ls .                        # Token-optimized directory tree
tok read file.rs                # Smart file reading
tok read file.rs -l aggressive  # Signatures only (strips bodies)
tok smart file.rs               # 2-line heuristic code summary
tok find "*.rs" .               # Compact find results
tok grep "pattern" .            # Grouped search results
tok diff file1 file2            # Condensed diff
```

### Git
```bash
tok git status                  # Compact status
tok git log -n 10               # One-line commits
tok git diff                    # Condensed diff
tok git add                     # -> "ok"
tok git commit -m "msg"         # -> "ok abc1234"
tok git push                    # -> "ok main"
tok git pull                    # -> "ok 3 files +10 -2"
```

### GitHub CLI
```bash
tok gh pr list                  # Compact PR listing
tok gh pr view 42               # PR details + checks
tok gh issue list               # Compact issue listing
tok gh run list                 # Workflow run status
```

### Test Runners
```bash
tok test cargo test             # Show failures only (-90%)
tok err npm run build           # Errors/warnings only
tok vitest run                  # Vitest compact (failures only)
tok playwright test             # E2E results (failures only)
tok pytest                      # Python tests (-90%)
tok go test                     # Go tests (NDJSON, -90%)
tok cargo test                  # Cargo tests (-90%)
tok rake test                   # Ruby minitest (-90%)
tok rspec                       # RSpec tests (JSON, -60%+)
```

### Build & Lint
```bash
tok lint                        # ESLint grouped by rule/file
tok lint biome                  # Supports other linters
tok tsc                         # TypeScript errors grouped by file
tok next build                  # Next.js build compact
tok prettier --check .          # Files needing formatting
tok cargo build                 # Cargo build (-80%)
tok cargo clippy                # Cargo clippy (-80%)
tok ruff check                  # Python linting (JSON, -80%)
tok golangci-lint run           # Go linting (JSON, -85%)
tok rubocop                     # Ruby linting (JSON, -60%+)
```

### Package Managers
```bash
tok pnpm list                   # Compact dependency tree
tok pip list                    # Python packages (auto-detect uv)
tok pip outdated                # Outdated packages
tok bundle install              # Ruby gems (strip Using lines)
tok prisma generate             # Schema generation (no ASCII art)
```

### AWS
```bash
tok aws sts get-caller-identity # One-line identity
tok aws ec2 describe-instances  # Compact instance list
tok aws lambda list-functions   # Name/runtime/memory (strips secrets)
tok aws logs get-log-events     # Timestamped messages only
tok aws cloudformation describe-stack-events  # Failures first
tok aws dynamodb scan           # Unwraps type annotations
tok aws iam list-roles          # Strips policy documents
tok aws s3 ls                   # Truncated with tee recovery
```

### Containers
```bash
tok docker ps                   # Compact container list
tok docker images               # Compact image list
tok docker logs <container>     # Deduplicated logs
tok docker compose ps           # Compose services
tok kubectl pods                # Compact pod list
tok kubectl logs <pod>          # Deduplicated logs
tok kubectl services            # Compact service list
```

### Data & Analytics
```bash
tok json config.json            # Structure without values
tok deps                        # Dependencies summary
tok env -f AWS                  # Filtered env vars
tok log app.log                 # Deduplicated logs
tok curl <url>                  # Auto-detect JSON + schema
tok wget <url>                  # Download, strip progress bars
tok summary <long command>      # Heuristic summary
tok proxy <command>             # Raw passthrough + tracking
```

### Token Savings Analytics
```bash
tok gain                        # Summary stats
tok gain --graph                # ASCII graph (last 30 days)
tok gain --history              # Recent command history
tok gain --daily                # Day-by-day breakdown
tok gain --all --format json    # JSON export for dashboards

tok discover                    # Find missed savings opportunities
tok discover --all --since 7    # All projects, last 7 days

tok session                     # Show TOK adoption across recent sessions
```

## Global Flags

```bash
-u, --ultra-compact    # ASCII icons, inline format (extra token savings)
-v, --verbose          # Increase verbosity (-v, -vv, -vvv)
```

## Examples

**Directory listing:**
```
# ls -la (45 lines, ~800 tokens)        # tok ls (12 lines, ~150 tokens)
drwxr-xr-x  15 user staff 480 ...       my-project/
-rw-r--r--   1 user staff 1234 ...       +-- src/ (8 files)
...                                      |   +-- main.rs
                                         +-- Cargo.toml
```

**Git operations:**
```
# git push (15 lines, ~200 tokens)       # tok git push (1 line, ~10 tokens)
Enumerating objects: 5, done.             ok main
Counting objects: 100% (5/5), done.
Delta compression using up to 8 threads
...
```

**Test output:**
```
# cargo test (200+ lines on failure)     # tok test cargo test (~20 lines)
running 15 tests                          FAILED: 2/15 tests
test utils::test_parse ... ok               test_edge_case: assertion failed
test utils::test_format ... ok              test_overflow: panic at utils.rs:18
...
```

## Auto-Rewrite Hook

The most effective way to use tok. The hook transparently intercepts Bash commands and rewrites them to tok equivalents before execution.

**Result**: 100% tok adoption across all conversations and subagents, zero token overhead.

**Scope note:** this only applies to Bash tool calls. Claude Code built-in tools such as `Read`, `Grep`, and `Glob` bypass the hook, so use shell commands or explicit `tok` commands when you want TOK filtering there.

### Setup

```bash
tok init -g                 # Install hook + TOK.md (recommended)
tok init -g --opencode      # OpenCode plugin (instead of Claude Code)
tok init -g --auto-patch    # Non-interactive (CI/CD)
tok init -g --hook-only     # Hook only, no TOK.md
tok init --show             # Verify installation
```

After install, **restart Claude Code**.

## Supported AI Tools

TOK supports 10 AI coding tools. Each integration transparently rewrites shell commands to `tok` equivalents for 60-90% token savings.

| Tool | Install | Method |
|------|---------|--------|
| **Claude Code** | `tok init -g` | PreToolUse hook (bash) |
| **GitHub Copilot (VS Code)** | `tok init -g --copilot` | PreToolUse hook (`tok hook copilot`) — transparent rewrite |
| **GitHub Copilot CLI** | `tok init -g --copilot` | PreToolUse deny-with-suggestion (CLI limitation) |
| **Cursor** | `tok init -g --agent cursor` | preToolUse hook (hooks.json) |
| **Gemini CLI** | `tok init -g --gemini` | BeforeTool hook (`tok hook gemini`) |
| **Codex** | `tok init -g --codex` | AGENTS.md + TOK.md instructions |
| **Windsurf** | `tok init --agent windsurf` | .windsurfrules (project-scoped) |
| **Cline / Roo Code** | `tok init --agent cline` | .clinerules (project-scoped) |
| **OpenCode** | `tok init -g --opencode` | Plugin TS (tool.execute.before) |
| **OpenClaw** | `openclaw plugins install ./openclaw` | Plugin TS (before_tool_call) |
| **Mistral Vibe** | Planned (#800) | Blocked on upstream BeforeToolCallback |

### Claude Code (default)

```bash
tok init -g                 # Install hook + TOK.md
tok init -g --auto-patch    # Non-interactive (CI/CD)
tok init --show             # Verify installation
tok init -g --uninstall     # Remove
```

### GitHub Copilot (VS Code + CLI)

```bash
tok init -g --copilot         # Install hook + instructions
```

Creates `.github/hooks/tok-rewrite.json` (PreToolUse hook) and `.github/copilot-instructions.md` (prompt-level awareness).

The hook (`tok hook copilot`) auto-detects the format:
- **VS Code Copilot Chat**: transparent rewrite via `updatedInput` (same as Claude Code)
- **Copilot CLI**: deny-with-suggestion (CLI does not support `updatedInput` yet — see [copilot-cli#2013](https://github.com/github/copilot-cli/issues/2013))

### Cursor

```bash
tok init -g --agent cursor
```

Creates `~/.cursor/hooks/tok-rewrite.sh` + patches `~/.cursor/hooks.json` with preToolUse matcher. Works with both Cursor editor and `cursor-agent` CLI.

### Gemini CLI

```bash
tok init -g --gemini
tok init -g --gemini --uninstall
```

Creates `~/.gemini/hooks/tok-hook-gemini.sh` + patches `~/.gemini/settings.json` with BeforeTool hook.

### Codex (OpenAI)

```bash
tok init -g --codex
```

Creates `~/.codex/TOK.md` + `~/.codex/AGENTS.md` with `@TOK.md` reference. Codex reads these as global instructions.

### Windsurf

```bash
tok init --agent windsurf
```

Creates `.windsurfrules` in the current project. Cascade reads rules and prefixes commands with `tok`.

### Cline / Roo Code

```bash
tok init --agent cline
```

Creates `.clinerules` in the current project. Cline reads rules and prefixes commands with `tok`.

### OpenCode

```bash
tok init -g --opencode
```

Creates `~/.config/opencode/plugins/tok.ts`. Uses `tool.execute.before` hook.

### OpenClaw

```bash
openclaw plugins install ./openclaw
```

Plugin in `openclaw/` directory. Uses `before_tool_call` hook, delegates to `tok rewrite`.

### Mistral Vibe (planned)

Blocked on upstream BeforeToolCallback support ([mistral-vibe#531](https://github.com/mistralai/mistral-vibe/issues/531), [PR #533](https://github.com/mistralai/mistral-vibe/pull/533)). Tracked in [#800](https://github.com/tok-ai/tok/issues/800).

### Commands Rewritten

| Raw Command | Rewritten To |
|-------------|-------------|
| `git status/diff/log/add/commit/push/pull` | `tok git ...` |
| `gh pr/issue/run` | `tok gh ...` |
| `cargo test/build/clippy` | `tok cargo ...` |
| `cat/head/tail <file>` | `tok read <file>` |
| `rg/grep <pattern>` | `tok grep <pattern>` |
| `ls` | `tok ls` |
| `vitest/jest` | `tok vitest run` |
| `tsc` | `tok tsc` |
| `eslint/biome` | `tok lint` |
| `prettier` | `tok prettier` |
| `playwright` | `tok playwright` |
| `prisma` | `tok prisma` |
| `ruff check/format` | `tok ruff ...` |
| `pytest` | `tok pytest` |
| `pip list/install` | `tok pip ...` |
| `go test/build/vet` | `tok go ...` |
| `golangci-lint` | `tok golangci-lint` |
| `rake test` / `rails test` | `tok rake test` |
| `rspec` / `bundle exec rspec` | `tok rspec` |
| `rubocop` / `bundle exec rubocop` | `tok rubocop` |
| `bundle install/update` | `tok bundle ...` |
| `aws sts/ec2/lambda/...` | `tok aws ...` |
| `docker ps/images/logs` | `tok docker ...` |
| `kubectl get/logs` | `tok kubectl ...` |
| `curl` | `tok curl` |
| `pnpm list/outdated` | `tok pnpm ...` |

Commands already using `tok`, heredocs (`<<`), and unrecognized commands pass through unchanged.

## Configuration

### Config File

`~/.config/tok/config.toml` (macOS: `~/Library/Application Support/tok/config.toml`):

```toml
[tracking]
database_path = "/path/to/custom.db"  # default: ~/.local/share/tok/history.db

[hooks]
exclude_commands = ["curl", "playwright"]  # skip rewrite for these

[tee]
enabled = true          # save raw output on failure (default: true)
mode = "failures"       # "failures", "always", or "never"
max_files = 20          # rotation limit
```

### Tee: Full Output Recovery

When a command fails, TOK saves the full unfiltered output so the LLM can read it without re-executing:

```
FAILED: 2/15 tests
[full output: ~/.local/share/tok/tee/1707753600_cargo_test.log]
```

### Uninstall

```bash
tok init -g --uninstall     # Remove hook, TOK.md, settings.json entry
cargo uninstall tok          # Remove binary
brew uninstall tok           # If installed via Homebrew
```

## Documentation

- **[TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)** - Fix common issues
- **[INSTALL.md](INSTALL.md)** - Detailed installation guide
- **[ARCHITECTURE.md](docs/contributing/ARCHITECTURE.md)** - Technical architecture
- **[SECURITY.md](SECURITY.md)** - Security policy and PR review process
- **[AUDIT_GUIDE.md](docs/AUDIT_GUIDE.md)** - Token savings analytics guide

## Privacy & Telemetry

TOK collects **anonymous, aggregate usage metrics** once per day, **enabled by default**. This helps prioritize development. See opt-out options below.

**What is collected:**
- Device hash (salted SHA-256 — per-user random salt stored locally, not reversible)
- TOK version, OS, architecture
- Command count (last 24h) and top command names (e.g. "git", "cargo" — no arguments, no file paths)
- Token savings percentage

**What is NOT collected:** source code, file paths, command arguments, secrets, environment variables, or any personally identifiable information.

**Opt-out** (any of these):
```bash
# Environment variable
export TOK_TELEMETRY_DISABLED=1

# Or in config file (~/.config/tok/config.toml)
[telemetry]
enabled = false
```

## Contributing

Contributions welcome! Please open an issue or PR on [GitHub](https://github.com/tok-ai/tok).

Join the community on [Discord](https://discord.gg/RySmvNF5kF).

## License

MIT License - see [LICENSE](LICENSE) for details.

## Disclaimer

See [DISCLAIMER.md](DISCLAIMER.md).
