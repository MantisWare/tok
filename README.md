<p align="center">
  <img src="docs/logo.png" alt="TOK - Token Optimization Kit" width="500">
</p>

<p align="center">
  <strong>CLI proxy that shaves 60–90% off the tokens your LLM eats — same commands, less wallpaper</strong>
</p>

---

**tok** sits between your shell and your model: it filters, groups, and squashes command output so the assistant sees signal, not noise. One Rust binary, **100+ filtered commands**, under ~10ms of overhead — basically a bouncer for your terminal who also keeps receipts (`tok gain`), remembers your codebase (`tok mem`), and can redact secrets on the way out (`--security`).

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

### Homebrew (macOS)

```bash
brew tap MantisWare/tap
brew install tok
```

Tap: [github.com/MantisWare/homebrew-tap](https://github.com/MantisWare/homebrew-tap) — the formula is updated automatically on each **stable** release when CI is configured ([details](docs/contributing/RELEASE.md)).

### Cargo

```bash
cargo install --git https://github.com/MantisWare/tok
```

For more options, see **[INSTALL.md](INSTALL.md)**.

### Verify Installation

```bash
tok --version   # Should show "tok 0.1.18" (or newer)
tok gain        # Should show token savings stats
tok man         # Full command manual (filter: tok man git)
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

## What’s in the Box

TOK isn’t just “prettier `git status`.” Six layers, same binary:

| Layer | What it does | Jump in with |
|-------|----------------|----------------|
| **Proxy filters** | Run real CLI tools, return compressed output | `tok git status`, `tok cargo test` |
| **Auto-rewrite hooks** | Transparent `git` → `tok git` in your agent | `tok init -g` |
| **Analytics** | SQLite history, savings dashboards, missed-opportunity mining | `tok gain`, `tok discover` |
| **Security (optional)** | Obfuscate PII/secrets before context; restore on the way back | `tok --security proxy …` |
| **Code memory (`tok mem`)** | Index symbols, search, impact, dead-code — structural, not grep | `tok mem index .` |
| **ForgeMap** | Machine-readable headers + manifests for agent orientation | `tok forgemap init src/` |

Don’t know where to start? `tok man` prints the whole menu; `tok man mem` or `tok man security` narrows it down.

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

Unknown subcommand? **Passthrough** — run the real binary, log metrics, move on. Filters that choke fall back the same way (see `src/cmds/README.md`).

## Commands

> **Passthrough promise:** if TOK doesn’t recognize a subcommand, it runs the real tool unchanged and still logs the run. You can prefix everything with `tok` and never get stuck.

Run `tok man` for the full catalog, or `tok man <topic>` (e.g. `git`, `mem`, `security`). Deep dive: **[docs/usage/FEATURES.md](docs/usage/FEATURES.md)**.

### Files & search

```bash
tok ls .                        # Tree-style listing (native ls flags work)
tok tree src/                   # tree(1), filtered
tok read file.rs                # Smart read (replaces cat/head/tail)
tok read file.rs -l aggressive  # Signatures only — bodies vanish (~74% smaller)
tok read file.rs -m 200 -n      # Cap lines + line numbers
tok smart file.rs               # Two-line “what is this file?” (heuristic, offline)
tok find "*.rs" .               # Compact find output
tok fd -e rs                    # fd — same TOML path as other shell tools
tok grep "pattern" .            # Grouped ripgrep ( -m 50, -l 80 by default)
tok diff a.rs b.rs              # Condensed file diff
tok wc -l src/**/*.rs           # wc without the padding parade
tok jq . package.json           # Truncate huge JSON blobs
tok stat file.rs                # Drop inode/device noise
tok du -sh . / tok df -h        # Disk usage, line-capped
tok ps aux / tok lsof -i        # Process / open-file listings, capped
tok tar -tf archive.tar         # Archive listings without walls of paths
tok journalctl -u myapp -n 50   # Logs — blank lines stripped, line cap
tok dig example.com / tok host example.com
tok ss -tlnp / tok netstat -an  # Socket tables, capped
```

Hooks also rewrite bare `fd`, `jq`, `stat`, `tar`, `journalctl`, `lsof`, `dig`, `host`, `ss`, and `bat` → `tok read` when installed.

### Git & GitHub

```bash
tok git status                  # Branch + counts + changed paths (~80%)
tok git log -n 10               # One line per commit
tok git diff / show             # Stat-first diffs
tok git add                     # -> ok
tok git commit -m "msg"         # -> ok abc1234
tok git push                    # -> ok main
tok git pull                    # -> ok 3 files +10 -2
tok git branch / fetch / stash / worktree   # Same vibe — short answers
tok git blame / rev-parse / ls-files / describe / tag / remote / config  # Hook rewrites these too
# Other subcommands: passthrough via `tok git <cmd>` (merge, rebase, …)

tok gh pr list                  # PRs without the novella
tok gh pr view 42               # PR + checks, compressed
tok gh issue list               # Issues, compact
tok gh run list                 # Workflow runs
tok gt log                      # Graphite stack log (stacked PR workflows)
tok gt submit / sync / restack  # Other gt subcommands — same compression idea
```

### Tests & errors

```bash
tok test cargo test             # Generic wrapper — failures only
tok err npm run build           # Any command — errors & warnings only
tok cargo test                  # Rust tests (~90%)
tok cargo nextest run           # nextest, failures only
tok vitest run                  # Vitest (~99% on noisy runs)
tok playwright test             # E2E — failures rise to the top
tok pytest                      # Python (~90%)
tok go test ./...               # Go NDJSON stream (~90%)
tok rake test / tok rspec       # Ruby stacks
tok dotnet test                 # .NET TRX — compact failures
```

### Build, lint & format

```bash
tok cargo build / check / clippy / install   # Skip “Compiling…” ticker
tok lint                        # ESLint — grouped by rule + file
tok lint biome                  # Biome too
tok tsc                         # TS errors grouped by file + code
tok mypy .                      # Python types, grouped
tok next build                  # Next.js route metrics, less noise
tok prettier --check .          # Only files that need love
tok format .                    # Auto-detect formatter (prettier/black/ruff/rustfmt)
tok ruff check / ruff format    # Python (~80%)
tok golangci-lint run           # Go (~85%)
tok rubocop                     # Ruby (~60%+)
tok dotnet build                # MSBuild murmur, not shout
```

### Package managers

```bash
tok pnpm list / outdated / install
tok npm run build               # Strips npm progress-bar theater
tok npx tsc                     # Smart routing: npx → specialized tok filters
tok pip list / outdated         # Auto-prefers uv when installed
tok bundle install              # Gems without “Using …” spam
tok prisma generate             # No ASCII art, just facts
tok prisma migrate dev          # Migrations, compact
tok deps                        # One-screen dep summary (Cargo/npm/py/go/Gemfile…)
```

### Cloud, data & network

```bash
tok aws sts get-caller-identity # JSON in → human lines out (all AWS services)
tok aws ec2 describe-instances
tok psql -c 'SELECT 1'          # Tables without border wallpaper
tok json config.json            # Shape only (add --schema to strip values)
tok json package.json --depth 3
tok env -f AWS                  # Filtered env (secrets masked by default)
tok env --show-all              # YOLO mode — show everything
tok log app.log                 # Deduplicated logs ([ERROR] … x42)
tok curl https://api…           # JSON → schema when detected
tok wget <url>                  # No progress-bar light show
tok summary <long cmd…>         # Heuristic one-liner when no dedicated filter exists
tok proxy <command>             # Raw output, still tracked (0% “savings”, 100% honesty)
```

### Containers

```bash
tok docker ps / images / logs <c>
tok docker compose ps / logs / build
tok kubectl pods / services / logs <pod> [-n ns]
# Unlisted subcommands pass through to docker/kubectl
```

### Security & privacy

```bash
tok security-inspect file.txt            # Dry-run scan
tok security-inspect file.txt --report   # Entity types + confidence
echo "text" | tok security-inspect -
tok doctor                      # General health
tok doctor --slm                # Local SLM binary + model check

# Global flags (any subcommand)
tok --security proxy git status
tok --security --security-mode strict proxy cargo test
tok --no-security proxy echo "raw"
tok --slm --security proxy git log
```

See [Optional Security Mode](#optional-security-mode) below for modes, config, and restoration flow.

### Token savings & insights

Every filtered command writes to a local SQLite DB (`~/.local/share/tok/history.db` on Linux, `~/Library/Application Support/tok/` on macOS). **90-day retention**, no command args or file paths in telemetry.

```bash
tok gain                        # Dashboard: totals, top 10 commands, $ estimate
tok gain --top 25               # Show more commands in the leaderboard (max 100)
tok gain --rollup --top 25      # Aggregate by tool (cargo, grep, git, …)
tok gain --failures             # Commands that fell back to raw passthrough (0% savings)
tok gain --by-client            # Breakdown by cursor / claude / terminal / …
tok gain --graph                # ASCII chart — last 30 days
tok gain --history              # Per-command log
tok gain --daily / --weekly / --monthly / --all
tok gain -p                     # This project only
tok gain --quota -t pro         # “What if” against Claude quota tiers
tok gain --format json / csv    # Export includes by_command with top/rollup settings

tok discover                    # Mine agent history for commands that should’ve been tok
tok discover --all --since 7

tok session                     # TOK adoption across recent agent sessions

tok learn                       # Recurring CLI mistakes → suggested fixes
tok learn --write-rules         # Emit .claude/rules/cli-corrections.md

tok cc-economics                # Claude spend (ccusage) vs tok savings — receipts
tok cc-economics --daily --format json
```

Details: **[docs/usage/AUDIT_GUIDE.md](docs/usage/AUDIT_GUIDE.md)** · **[docs/usage/TRACKING.md](docs/usage/TRACKING.md)** · **[docs/usage/FEATURES.md](docs/usage/FEATURES.md)** (`tok gain` flags)

### Agent token playbook

Habits that multiply TOK beyond “install the hook”:

| Instead of | Prefer | Why |
|------------|--------|-----|
| Repeated `grep` for a symbol | `tok mem find` / `tok mem search` | Structural hits, not thousands of text lines |
| `cat` on large files | `tok read -l minimal` or `-m N` | Strips noise; caps lines |
| Orienting in a new repo | `tok mem index . --incremental`, `tok forgemap check` | Fewer full-file reads in chat context |
| Guessing missed savings | `tok discover --since 7` weekly | Shows proven commands still running unfiltered |
| Silent 0% savings | `tok gain --failures`, `tok verify` | Stale hooks or parse fallbacks |

Use **`-u` / `--ultra-compact`** on heavy git/cargo output when context is tight. Avoid `TOK_DISABLED=1` except when debugging — `tok gain` warns when bypass rate is high.

### Setup, config & trust

```bash
tok init -g                     # Hooks + TOK.md (see [Auto-Rewrite Hook](#auto-rewrite-hook))
tok init --all                  # Every supported agent at once (`-g` for global dirs)
tok init --show                 # “Did it actually install?”
tok config                      # Show or scaffold ~/.config/tok/config.toml
tok verify                      # Hook integrity (SHA-256) + filter smoke tests
tok trust                       # Trust this repo’s .tok TOML filter recipes
tok trust --list / tok untrust  # Manage trusted projects
tok rewrite "git status"        # What the hook runs — prints tok git status
tok hook gemini / tok hook copilot   # JSON stdin handlers for those agents
tok hook-audit                  # Rewrite stats (needs TOK_HOOK_AUDIT=1)
```

Project-local **`.tok/*.toml`** filters: trusted via `tok trust`, verified via `tok verify`. Custom recipes for commands we don’t ship yet.

## Global Flags

```bash
-u, --ultra-compact    # ASCII icons, inline fields — max squeeze
-v, --verbose          # -v / -vv / -vvv — filtering details on stderr
--skip-env             # SKIP_ENV_VALIDATION=1 for Next/tsc/lint/Prisma children
--security             # Enable security/privacy layer
--no-security          # Disable security (overrides config)
--security-mode <m>    # observe | balanced | strict | developer
--slm                  # Local SLM semantic scanning (with --security)
--no-slm               # Disable SLM (overrides config)
```

## Optional Security Mode

TOK can optionally scan and obfuscate sensitive data (PII, secrets, credentials) before it reaches your LLM context. The security layer **never blocks** -- it always obfuscates and continues. Your workflow is never interrupted.

### Enabling Security

**Per-command** (via CLI flag):
```bash
tok --security proxy git status
tok --security --security-mode strict proxy cargo test
```

**Permanently** (via config at `~/.config/tok/config.toml`):
```toml
[security]
enabled = true
mode = "balanced"
```

**Disabling** (override config for one command):
```bash
tok --no-security proxy echo "raw output"
```

### Security Modes

| Mode | Behavior |
|------|----------|
| `observe` | Scan and report only. No text modification. Useful for onboarding. |
| `balanced` | Obfuscate common PII and secrets per config. Recommended default. |
| `strict` | Obfuscate everything detected, regardless of per-entity config. |
| `developer` | Preserve code, stack traces, filenames, and URLs. Obfuscate secrets and internal identifiers only. |

### What Gets Detected

**PII** (regex-based):
- Email addresses (`john@example.com` → `{{TOK_EMAIL_001}}`)
- Phone numbers (`555-123-4567` → `{{TOK_PHONE_001}}`)
- IP addresses (`192.168.1.100` → `{{TOK_IP_001}}`, excludes `127.0.0.1`)
- Internal hostnames (`db-prod-01.internal` → `{{TOK_HOST_001}}`)
- URLs (`https://internal.api.com/v2` → `{{TOK_URL_001}}`)
- Money values (`$45,000` → `{{TOK_MONEY_001}}`)

**Secrets** (pattern-based, high confidence):
- API keys — Stripe (`sk_live_`), GitHub (`ghp_`, `github_pat_`), AWS (`AKIA`), OpenAI (`sk-`), Slack (`xoxb-`)
- JWT tokens (three-part base64url format)
- Private keys (`-----BEGIN RSA PRIVATE KEY-----`)
- Password assignments (`password=`, `DB_PASSWORD:`)
- Database URLs (`postgres://user:pass@host/db`)
- Credit card numbers (Luhn-validated, 13-19 digits)

### How It Works

```
Input text → Scanner → Classifier → Obfuscation → TOK Optimizer → Output
                                         ↓
                              In-memory map (never persisted)
                                         ↓
                          Response → Restoration → Final output
```

1. **Scanner** detects sensitive entities using regex + pattern matching
2. **Classifier** assigns severity (low/medium/high/critical) for reporting only
3. **Obfuscation** replaces values with `{{TOK_TYPE_NNN}}` placeholders
4. The placeholder map exists only in process memory -- never written to disk or sent externally
5. Responses are restored automatically using the local map

### Inspect Command

Scan text without modifying it. Shows what would be detected and at what confidence:

```bash
tok security-inspect ./prompt.txt --report
```

Output:
```
TOK Security Inspect

  Mode:     balanced
  Findings: 3
  Severity: High

  1. [email] "***REDACTED***" (confidence: 95%, action: Placeholder)
  2. [money] "***REDACTED***" (confidence: 90%, action: Placeholder)
  3. [apikey] "***REDACTED***" (confidence: 95%, action: Placeholder)
```

Supports stdin: `echo "text" | tok security-inspect - --report`

### Security Report (Verbose Mode)

When using `--security -v`, TOK prints a summary after command execution:

```
TOK Security Report

  Security: enabled
  Mode:     balanced
  Risk:     high

  Obfuscated: 4 sensitive values
    - apikey: 1
    - email: 2
    - hostname: 1
```

### Optional Local SLM

For semantic detection beyond regex (person names, company names, internal project names), TOK can use a local Small Language Model via embedded llama.cpp:

```bash
tok --security --slm proxy git log     # Enable SLM scanning
tok doctor --slm                       # Check SLM binary + model health
```

The SLM runs entirely on your machine (`127.0.0.1` only), is optional, and disabled by default. Deterministic scanner results always take precedence over SLM findings.

Configure in `~/.config/tok/config.toml`:
```toml
[slm]
enabled = true
runtime = "embedded-llamacpp"
model_path = "./models/tok-security-slm/model.gguf"
context_size = 8192
temperature = 0.1
```

Recommended model: **Qwen3-4B-Instruct GGUF Q4_K_M** (~2.5 GB).

### Configuration Reference

Full security config options in `~/.config/tok/config.toml`:

```toml
[security]
enabled = false
mode = "balanced"  # observe | balanced | strict | developer

[security.scan]
deterministic = true   # Regex + pattern scanning (always recommended)
slm = false            # Optional SLM semantic scanning

[security.actions]
# Per-entity action: "placeholder" (obfuscate) or "allow" (leave untouched)
email = "placeholder"
phone = "placeholder"
url = "placeholder"
hostname = "placeholder"
ip_address = "placeholder"
money = "placeholder"
api_key = "placeholder"
jwt = "placeholder"
private_key = "placeholder"
password = "placeholder"
database_url = "placeholder"
credit_card = "placeholder"

[security.restore]
enabled = true         # Restore placeholders in responses
exact = true           # Exact string matching for restoration

[security.logging]
store_original_prompts = false   # Never log original sensitive text
redact_logs = true               # Redact sensitive values from all log output
```

See [docs/SECURITY_LAYER.md](docs/SECURITY_LAYER.md) and [docs/SLM_RUNTIME.md](docs/SLM_RUNTIME.md) for complete documentation.

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
tok init --all              # Claude + Cursor + Gemini + Copilot + … in one go
tok init -g --all           # Same, global install dirs
tok init -g --opencode      # OpenCode plugin (instead of Claude Code)
tok init -g --auto-patch    # Non-interactive (CI/CD)
tok init -g --hook-only     # Hook only, no TOK.md
tok init --show             # Verify installation
tok verify                  # SHA-256 hook integrity + filter tests
```

After install, **restart your agent** (Claude Code, Cursor, etc.).

### Under the hood

The bash hook is ~50 lines — it delegates to **`tok rewrite`**, which looks up the command in Rust (`src/discover/registry.rs`). No rewrite? Exit 1, original command runs. Already `tok …`? Left alone. Heredocs (`<<`)? Sacred.

```bash
tok rewrite "git status"      # → tok git status
tok rewrite "terraform plan"  # → (no match, exit 1)
```

Set `TOK_HOOK_AUDIT=1` and run `tok hook-audit` to see what’s getting rewritten vs slipping through.

## Supported AI Tools

TOK supports 10 AI coding tools. Each integration transparently rewrites shell commands to `tok` equivalents for 60-90% token savings.

| Tool | Install | Method |
|------|---------|--------|
| **Claude Code** | `tok init -g` | PreToolUse hook (bash) |
| **GitHub Copilot (VS Code)** | `tok init -g --copilot` | PreToolUse hook (`tok hook copilot`) — transparent rewrite |
| **GitHub Copilot CLI** | `tok init -g --copilot` | PreToolUse deny-with-suggestion (CLI limitation) |
| **Cursor** | `tok init -g --agent cursor` | preToolUse rewrite + sessionStart awareness (hooks.json) |
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

Creates `~/.cursor/hooks/tok-rewrite.sh` (command rewriting) + `~/.cursor/hooks/tok-session.sh` (AI awareness) and patches `~/.cursor/hooks.json` with both `preToolUse` and `sessionStart` entries. Works with both Cursor editor and `cursor-agent` CLI.

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

Blocked on upstream BeforeToolCallback support ([mistral-vibe#531](https://github.com/mistralai/mistral-vibe/issues/531), [PR #533](https://github.com/mistralai/mistral-vibe/pull/533)). Tracked in [#800](https://github.com/MantisWare/tok/issues/800).

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
| `npm run …` | `tok npm …` |
| `npx tsc/eslint/…` | `tok npx …` (routes to specialized filters) |
| `mypy` | `tok mypy …` |
| `dotnet build/test` | `tok dotnet …` |
| `gt log/submit/…` | `tok gt …` |
| `psql` | `tok psql …` |

Commands already using `tok`, heredocs (`<<`), and unrecognized commands pass through unchanged.

## Configuration

```bash
tok config                  # Print or create starter config.toml
```

### Config file

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

[telemetry]
enabled = false         # opt out of daily anonymous metrics (on by default)
```

### Trusted local filters

Add **`.tok/my-command.toml`** in a repo, then:

```bash
tok trust                   # Allow this project’s TOML recipes
tok verify --filter my-command   # Run tests for one filter
tok untrust                 # Revoke
```

Useful when you want team-specific compression without upstreaming a filter yet.

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
```

## tok mem — Structural Code Memory

Grep finds *text*. **`tok mem`** finds *symbols* — who calls whom, what breaks if you rename a function, what’s been dead since the refactor. Local SQLite index, no cloud, no “send us your repo.”

**Workflow:** index once (or incrementally), then query instead of re-reading half the tree every session.

```bash
# Index & maintain
tok mem index .                 # Full index (symbols + relationships)
tok mem index . --incremental   # Only changed files
tok mem index . --clear         # Wipe repo data, re-index from scratch
tok mem repos                   # What’s indexed
tok mem status                  # Health + counts
tok mem forget my-repo          # Drop a repo from the DB

# Find & understand
tok mem search "token tracking" # BM25 full-text over symbols
tok mem find run_cli            # Exact or --fuzzy name match
tok mem context run_cli         # Callers, callees, type refs in one shot
tok mem relations run_cli --query-type find_callers --depth 3

# Change & risk
tok mem impact dispatch         # Blast radius — who breaks if this changes?
tok mem detect src/foo.rs       # Symbols touched by changed files
tok mem changes                 # Since last session (episode / timestamp)
tok mem evolution --from … --to …   # Hot symbols in a time window
tok mem timeline dispatch       # History of one symbol

# Architecture hygiene
tok mem central                 # Highest connectivity (hub symbols)
tok mem bridges                 # Symbols linking subgraphs
tok mem communities             # Connected components
tok mem dead-code               # Zero inbound refs ( --include-tests optional)
tok mem complexity              # Cyclomatic complexity ranking
```

Optional **`mem-ast`** feature flag in Cargo for richer parsing on supported languages. Index lives next to tracking data under the tok data directory.

`tok man mem` · implementation notes in **[docs/contributing/ARCHITECTURE.md](docs/contributing/ARCHITECTURE.md)**

## ForgeMap — Code Indexing and Annotation

ForgeMap is TOK’s other brain for *orientation*: machine-readable comment headers in source files, reverse dependency graphs, and project manifests. It implements the [CodeDNA](https://github.com/Larens94/codedna) protocol — breadcrumbs for agents, not wallpaper for humans.

**Why bother?** Your agent re-reads `src/` every Monday like it’s never met you. Headers spell out exports, callers, rules, and provenance so the first `tok read` actually means something.

### Quick Start

```bash
tok forgemap init src/       # Annotate all source files with ForgeMap headers
tok forgemap check src/      # Verify annotation coverage (exit 1 if incomplete)
tok forgemap refresh src/    # Update exports:/used_by: after code changes
tok forgemap manifest .      # Generate .forgemap project manifest
```

### Commands

| Command | Description |
|---------|-------------|
| `tok forgemap init <path>` | First-time annotation pass (inject headers) |
| `tok forgemap update <path>` | Annotate only files missing a header |
| `tok forgemap check <path>` | Coverage report (exit 1 if incomplete) |
| `tok forgemap refresh <path>` | Update `exports:`/`used_by:` only (structural refresh) |
| `tok forgemap manifest [path]` | Generate `.forgemap` project manifest |
| `tok forgemap wiki bootstrap [path]` | Emit per-file Obsidian vault |
| `tok forgemap wiki sync [path]` | Regenerate narrative project wiki |
| `tok forgemap install` | Install pre-commit hook + tool prompt files |

### Common Flags

| Flag | Description |
|------|-------------|
| `--repo-root <path>` | Repository root directory |
| `--exclude <patterns...>` | Glob patterns to exclude |
| `--extensions <exts...>` | File extensions to include |
| `--dry-run` | Preview changes without writing |
| `--force` | Re-annotate already-annotated files (init only) |
| `--model <id>` | Model ID for `agent:` line |
| `--tools <tools...>` | Tool prompts to install: `claude`, `cursor`, `copilot` |

### Supported Languages

Rust, TypeScript, JavaScript, Python, Go, Ruby, C#, Java — with language-aware comment prefixes (`//` or `#`).

For the full protocol specification, see **[docs/FORGEMAP.md](docs/FORGEMAP.md)**.

## How to Use TOK (cheat sheet)

| You want… | Do this |
|-----------|---------|
| Automatic savings in Claude/Cursor/Gemini | `tok init -g` (+ agent flag), restart agent |
| One-off compact output | `tok <same command you'd run anyway>` |
| Full output but still logged | `tok proxy <cmd>` |
| See if hooks are lying | `tok init --show` · `tok verify` |
| Dashboard / ROI | `tok gain` · `tok cc-economics` |
| Find commands you forgot to tok | `tok discover` |
| Map the codebase structurally | `tok mem index .` then `tok mem search …` |
| Agent-readable file headers | `tok forgemap init src/` |
| Redact secrets in context | `tok --security proxy …` or enable in config |
| Custom filter for your stack | `.tok/*.toml` + `tok trust` |
| Everything else | `tok man` |

**Implementing filters or hooks?** Start with **[docs/contributing/TECHNICAL.md](docs/contributing/TECHNICAL.md)** (flow + folder map), then **[docs/contributing/ARCHITECTURE.md](docs/contributing/ARCHITECTURE.md)** and **[src/cmds/README.md](src/cmds/README.md#adding-a-new-command-filter)**.

## Documentation

- **[FEATURES.md](docs/usage/FEATURES.md)** — Complete functional reference (every command)
- **[TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)** — Fix common issues
- **[INSTALL.md](INSTALL.md)** — Detailed installation guide
- **[RELEASE.md](docs/contributing/RELEASE.md)** — Stable releases, Homebrew tap, maintainer checklist
- **[TECHNICAL.md](docs/contributing/TECHNICAL.md)** — Request lifecycle, module map
- **[ARCHITECTURE.md](docs/contributing/ARCHITECTURE.md)** — Design depth, ADR-style detail
- **[DEVELOPMENT.md](docs/contributing/DEVELOPMENT.md)** — Build, test, release from a clone
- **[SECURITY.md](SECURITY.md)** — Security policy and PR review process
- **[SECURITY_LAYER.md](docs/SECURITY_LAYER.md)** — Optional security/privacy obfuscation layer
- **[SLM_RUNTIME.md](docs/SLM_RUNTIME.md)** — Local SLM (llama.cpp) setup guide
- **[AUDIT_GUIDE.md](docs/usage/AUDIT_GUIDE.md)** — Token savings analytics guide
- **[TRACKING.md](docs/usage/TRACKING.md)** — SQLite metrics schema & retention
- **[FORGEMAP.md](docs/FORGEMAP.md)** — ForgeMap / CodeDNA protocol specification

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

Contributions welcome! Please open an issue or PR on [GitHub](https://github.com/MantisWare/tok).

Join the community on [Discord](https://discord.gg/RySmvNF5kF).

## License

MIT License - see [LICENSE](LICENSE) for details.

## Disclaimer

See [DISCLAIMER.md](DISCLAIMER.md).
