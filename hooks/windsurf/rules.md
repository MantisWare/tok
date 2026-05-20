# TOK - Token Optimization Kit (Windsurf)

**Usage**: Token-optimized CLI proxy (60-90% savings on dev operations).

## Rule

Always prefix shell commands with `tok` to minimize token consumption. If TOK has a filter, it compresses the output. If not, it passes through unchanged. TOK is always safe to use.

Even in command chains: `tok git add . && tok git commit -m "msg" && tok git push`

## Filters by Category

### Git & GitHub
```bash
tok git status / log / diff / show / add / commit / push / pull / branch / fetch / stash
tok gh pr view <n> / gh issue list / gh run list
```

### Build & Compile
```bash
tok cargo build / check / clippy
tok tsc / lint / prettier --check . / next build
tok dotnet build / go build / go vet
```

### Test
```bash
tok cargo test / vitest run / playwright test / pytest
tok go test ./... / rspec / rake test / dotnet test
tok test <cmd>             # Generic wrapper — failures only
```

### Package Managers
```bash
tok pnpm install / list / outdated
tok npm run <script> / npx <cmd>
tok pip install / list / deps
```

### Files, Search & Utilities
```bash
tok ls <path> / tree / read <file> / smart <file>
tok find . -name '*.rs' / grep <pattern> / wc <file>
tok diff / json <file> / env / log <file>
tok err <cmd> / summary <cmd> / curl <url> / wget <url>
```

### Infrastructure
```bash
tok docker ps / images / logs / compose ps
tok kubectl pods / services / logs
tok aws <cmd> / psql <cmd>
```

### Linting & Formatting
```bash
tok ruff check . / mypy . / rubocop / golangci-lint run / format
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
```

## Agent Memory (`tok memory`)

```bash
tok memory status
tok memory add "<text>" --type rule
tok memory search "<query>"
tok memory inspect-context "<query>"
tok memory list
```

## ForgeMap

```bash
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
tok trust / untrust        # Manage local .tok filter trust
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

## Why

TOK filters and compresses command output before it reaches the LLM context, saving 60-90% tokens on common operations. Always use `tok <cmd>` instead of raw commands.
