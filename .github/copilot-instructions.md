# TOK — Token-Optimized CLI

**tok** is a CLI proxy that filters and compresses command outputs, saving 60-90% tokens. Always prefix shell commands with `tok`. If TOK has a filter, it compresses; if not, it passes through unchanged.

## Rule

Always prefix shell commands with `tok`:

```bash
# Instead of:              Use:
git status                 tok git status
git log -10                tok git log -10
cargo test                 tok cargo test
docker ps                  tok docker ps
kubectl get pods           tok kubectl pods
```

Even in chains: `tok git add . && tok git commit -m "msg" && tok git push`

## Filters (60+ tools)

```bash
# Git & GitHub
tok git status / log / diff / show / add / commit / push / pull / branch / fetch / stash
tok gh pr view <n> / gh issue list / gh run list

# Build & Compile
tok cargo build / check / clippy
tok tsc / lint / prettier --check . / next build
tok dotnet build / go build / go vet

# Test
tok cargo test / vitest run / playwright test / pytest
tok go test ./... / rspec / rake test / dotnet test
tok test <cmd>

# Package Managers
tok pnpm install / list / outdated
tok npm run <script> / npx <cmd> / pip install / deps

# Files & Search
tok ls / tree / read <file> / smart <file>
tok find / grep / wc / diff / json / env / log
tok err <cmd> / summary <cmd>

# Infrastructure & Network
tok docker ps / images / logs / compose
tok kubectl pods / services / logs
tok aws <cmd> / psql <cmd>
tok curl <url> / wget <url>

# Linting & Formatting
tok ruff check . / mypy . / rubocop / golangci-lint run / format

# Stacked PRs (Graphite)
tok gt log / submit / sync / restack / create / branch
```

## Code Intelligence

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
tok discover               # Find missed tok opportunities
tok session                # Usage stats across sessions
tok learn                  # Learn CLI fixes from past mistakes
```

## Configuration

```bash
tok config                 # View or scaffold tok config
tok verify                 # Sanity-check hooks and filters
tok trust / untrust        # Manage local .tok filter trust
tok proxy <cmd>            # Raw passthrough (still tracks stats)
tok man                    # Full command manual (every command)
tok man <topic>            # Filter manual (e.g. tok man security)
```

## Useful Flags

- `-u` / `--ultra-compact` — maximum compression
- `--security` / `--security-mode <mode>` — privacy layer
- `-v` / `-vv` / `-vvv` — increase verbosity

## Verification

```bash
tok --version              # Should show: tok X.Y.Z
tok gain                   # Should work (not "command not found")
which tok                  # Verify correct binary
```

> **Different tool**: If `tok gain` fails, confirm you installed Token Optimization Kit (MantisWare/tok). Rust Type Kit is `reachingforthejack/rtk` (usually the `rtk` command).
