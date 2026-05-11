# TOK - Token Optimization Kit

**Usage**: Token-optimized CLI proxy (60-90% savings on dev operations). Always prefix shell commands with `tok`.

## Golden Rule

**Always use `tok <command>`** instead of raw commands. If TOK has a filter, it compresses the output. If not, it passes through unchanged. TOK is always safe to use.

Even in command chains: `tok git add . && tok git commit -m "msg" && tok git push`

## Filters by Category

### Git & GitHub
```bash
tok git status             # Compact status
tok git log                # One-line-per-commit (all git flags work)
tok git diff               # Compact diff
tok git show / add / commit / push / pull / branch / fetch / stash / worktree
tok gh pr view <n>         # Compact PR view
tok gh issue list          # Compact issue list
tok gh run list            # Compact workflow runs
```

### Build & Compile
```bash
tok cargo build / check / clippy   # Rust build output, lints grouped
tok tsc                    # TypeScript errors grouped by file
tok lint                   # ESLint violations grouped by rule
tok prettier --check .     # Files needing format only
tok next build             # Next.js build with route metrics
tok dotnet build / test    # .NET build/test compact
tok go build / vet         # Go build, static checks compact
```

### Test
```bash
tok cargo test             # Failures only (90% savings)
tok vitest run             # Failures only (99% savings)
tok playwright test        # E2E failures only
tok pytest                 # Red tests first, fluff last
tok go test ./...          # Go test, ~90% fewer tokens
tok rspec                  # RSpec failures, not the whole sonnet
tok rake test              # Minitest compact
tok dotnet test            # xUnit without the XML wall
tok test <cmd>             # Generic wrapper — failures only
```

### Package Managers
```bash
tok pnpm install / list / outdated  # pnpm on quiet-room mode
tok npm run <script>       # Boilerplate stripped
tok npx <cmd>              # Smart routing to tsc/eslint/prisma filters
tok pip install / list     # pip/uv without the spam
tok deps                   # Dependency overview
```

### Files, Search & Utilities
```bash
tok ls <path>              # Tree format, compact
tok tree                   # tree(1) you can scroll past
tok read <file>            # Smart-filtered file content
tok smart <file>           # Two-line file summary (local, no cloud)
tok find . -name '*.rs'    # Compact tree-ish output
tok grep <pattern>         # Grouped by file, trimmed
tok wc <file>              # Counts without padding
tok diff <a> <b>           # Only lines that moved
tok json <file>            # Shrink values or --schema for shapes
tok env                    # Env vars filtered, secrets hidden
tok log <file>             # Dedupe repeats, keep the story
tok err <cmd>              # Run anything, print errors only
tok summary <cmd>          # Heuristic summary of output
```

### Infrastructure & Network
```bash
tok docker ps / images / logs       # Container info, compact
tok docker compose ps / logs        # Compose services at a glance
tok kubectl pods / services / logs  # K8s, fewer walls of YAML
tok aws <cmd>              # AWS CLI JSON, human-sized lines
tok psql <cmd>             # Tidy tables, fewer borders
tok curl <url>             # JSON auto-detected, schema mode
tok wget <url>             # Skip the progress bars
```

### Linting & Formatting
```bash
tok ruff check .           # Python linting, compact
tok mypy .                 # Type errors grouped for humans
tok rubocop                # RuboCop compact docket
tok golangci-lint run      # Many linters, one tight transcript
tok format                 # Auto-picks prettier / black / ruff format
```

### Stacked PRs (Graphite)
```bash
tok gt log / submit / sync / restack / create / branch
```

## Code Intelligence (use when needed)

```bash
tok mem index <dir>        # Index symbols, relationships, structure
tok mem search <query>     # Full-text search across indexed symbols
tok mem find <symbol>      # Exact or fuzzy symbol lookup
tok mem context <symbol>   # Callers, callees, type refs
tok mem impact <symbol>    # Blast radius — who breaks if this changes?
tok mem dead-code          # Symbols with zero inbound references
tok mem changes            # What changed since last session
tok mem detect             # Symbols affected by changed files
tok forgemap init          # Annotate source files with ForgeMap headers
tok forgemap check         # Coverage report for annotations
tok forgemap manifest      # Generate .forgemap project manifest
tok forgemap wiki bootstrap  # Emit Obsidian vault
```

## Security

```bash
tok --security <cmd>       # Obfuscate sensitive data in output
tok security-inspect <text>  # Dry-run: inspect text for secrets
tok doctor --slm           # Check SLM runtime health
```

## Analytics & Insights

```bash
tok gain                   # Token savings dashboard
tok gain --graph           # ASCII graph of daily savings
tok gain --history         # Per-command savings history
tok cc-economics           # Claude spend vs tok savings
tok discover               # Find missed TOK opportunities
tok session                # Usage stats across sessions
tok learn                  # Learn CLI fixes from past mistakes
```

## Configuration & Debugging

```bash
tok config                 # View or scaffold tok config
tok verify                 # Sanity-check hooks and filters
tok trust                  # Trust local .tok filter recipes
tok untrust                # Remove trusted filter recipes
tok proxy <cmd>            # Raw passthrough (still tracks stats)
tok man                    # Full command manual (every command)
tok man <topic>            # Filter manual (e.g. tok man security)
```

## Useful Flags

- `-u` / `--ultra-compact` — maximum compression mode
- `--security` / `--security-mode <mode>` — privacy/obfuscation layer (modes: observe, balanced, strict, developer)
- `-v` / `-vv` / `-vvv` — increase verbosity

## Installation Verification

```bash
tok --version              # Should show: tok X.Y.Z
tok gain                   # Should work (not "command not found")
which tok                  # Verify correct binary
```

> **Different tool**: If `tok gain` fails, confirm you installed Token Optimization Kit (MantisWare/tok). Rust Type Kit is `reachingforthejack/rtk` (usually the `rtk` command).

## Hook-Based Usage

All commands are automatically rewritten by the Claude Code hook.
Example: `git status` → `tok git status` (transparent, zero overhead)
