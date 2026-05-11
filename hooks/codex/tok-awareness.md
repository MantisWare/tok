# TOK - Token Optimization Kit (Codex CLI)

**Usage**: Token-optimized CLI proxy (60-90% savings on dev operations). Always prefix shell commands with `tok`.

## Golden Rule

**Always use `tok <command>`** instead of raw commands. If TOK has a filter, it compresses the output. If not, it passes through unchanged. TOK is always safe to use.

Even in command chains: `tok git add . && tok git commit -m "msg" && tok git push`

## Filters by Category

### Git & GitHub
```bash
tok git status / log / diff / show / add / commit / push / pull / branch / fetch / stash
tok gh pr view <n>         # Compact PR view
tok gh issue list          # Compact issue list
tok gh run list            # Compact workflow runs
```

### Build & Compile
```bash
tok cargo build / check / clippy   # Rust build, lints grouped
tok tsc                    # TypeScript errors grouped
tok lint                   # ESLint violations grouped
tok prettier --check .     # Files needing format only
tok next build             # Next.js build compact
tok dotnet build           # .NET build compact
tok go build / vet         # Go build, static checks
```

### Test
```bash
tok cargo test             # Failures only (90% savings)
tok vitest run             # Failures only (99% savings)
tok playwright test        # E2E failures only
tok pytest                 # Red tests first
tok go test ./...          # Go test compact
tok rspec                  # RSpec failures only
tok rake test              # Minitest compact
tok dotnet test            # xUnit compact
tok test <cmd>             # Generic wrapper — failures only
```

### Package Managers
```bash
tok pnpm install / list / outdated
tok npm run <script>       # Boilerplate stripped
tok npx <cmd>              # Smart routing to filters
tok pip install / list     # pip/uv compact
tok deps                   # Dependency overview
```

### Files, Search & Utilities
```bash
tok ls <path>              # Tree format, compact
tok tree                   # tree(1) compact
tok read <file>            # Smart-filtered file content
tok smart <file>           # Two-line file summary (local)
tok find . -name '*.rs'    # Compact tree-ish output
tok grep <pattern>         # Grouped by file
tok wc <file>              # Counts without padding
tok diff / json / env / log / err / summary
```

### Infrastructure & Network
```bash
tok docker ps / images / logs / compose
tok kubectl pods / services / logs
tok aws <cmd>              # AWS CLI compact
tok psql <cmd>             # Tidy tables
tok curl <url>             # JSON auto-detected
tok wget <url>             # Skip progress bars
```

### Linting & Formatting
```bash
tok ruff check .           # Python linting compact
tok mypy .                 # Type errors grouped
tok rubocop                # RuboCop compact
tok golangci-lint run      # Go linters compact
tok format                 # Auto-picks formatter
```

### Stacked PRs (Graphite)
```bash
tok gt log / submit / sync / restack / create / branch
```

## Code Intelligence (use when needed)

```bash
tok mem index <dir>        # Index symbols and structure
tok mem search <query>     # Full-text search (BM25)
tok mem find <symbol>      # Exact or fuzzy symbol lookup
tok mem context <symbol>   # Callers, callees, type refs
tok mem impact <symbol>    # Blast radius analysis
tok mem dead-code          # Zero-reference symbols
tok mem changes            # What changed since last session
tok forgemap init          # Annotate source files with headers
tok forgemap check         # Coverage report
tok forgemap manifest      # Generate project manifest
tok forgemap wiki bootstrap  # Emit Obsidian vault
```

## Security

```bash
tok --security <cmd>       # Obfuscate sensitive data in output
tok security-inspect <text>  # Dry-run: inspect for secrets
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
- `--security` / `--security-mode <mode>` — privacy/obfuscation layer
- `-v` / `-vv` / `-vvv` — increase verbosity

## Verification

```bash
tok --version              # Should show: tok X.Y.Z
tok gain                   # Should work (not "command not found")
which tok                  # Verify correct binary
```

> **Different tool**: If `tok gain` fails, confirm you installed Token Optimization Kit (MantisWare/tok). Rust Type Kit is `reachingforthejack/rtk` (usually the `rtk` command).
