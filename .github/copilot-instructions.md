# Copilot Instructions for tok

**tok (Token Optimization Kit)** is a CLI proxy that filters and compresses command outputs before they reach an LLM context, saving 60-90% of tokens. It wraps common tools (`git`, `cargo`, `grep`, `pnpm`, `go`, etc.) and outputs condensed summaries instead of raw output.

## Using tok in this session

**Always prefix commands with `tok` when running shell commands** — this reduces token consumption for every operation you perform.

```bash
# Instead of:              Use:
git status                 tok git status
git log -10                tok git log -10
cargo test                 tok cargo test
cargo clippy --all-targets tok cargo clippy --all-targets
grep -r "pattern" src/     tok grep -r "pattern" src/
```

**tok meta-commands** (always use these directly, no prefix needed):
```bash
tok gain              # Show token savings analytics
tok gain --history    # Full command history with per-command savings
tok discover          # Scan session history for missed tok opportunities
tok proxy <cmd>       # Run a command raw (no filtering) but still track it
```

**Verify tok is installed before starting:**
```bash
tok --version   # Should print: tok X.Y.Z
tok gain        # Should show a dashboard (not "command not found")
```

> If `tok gain` fails, verify `which tok` points to Token Optimization Kit (this tool). Rust Type Kit is a different project (`reachingforthejack/rtk`, usually the `rtk` command).

## Build, Test & Lint

```bash
cargo build                    # Development build
cargo test                     # All tests
cargo test test_name           # Single test
cargo test module::tests::     # Module tests
cargo test -- --nocapture      # With stdout

# Pre-commit gate (must all pass before any PR)
cargo fmt --all --check && cargo clippy --all-targets && cargo test

bash scripts/test-all.sh       # Smoke tests (requires installed binary)
```

PRs target the **`develop`** branch, not `main`. All commits require a DCO sign-off (`git commit -s`).

## Architecture

tok routes CLI commands via a Clap `Commands` enum in `main.rs` to specialized filter modules in `src/cmds/*/`, each executing the underlying command and compressing output. Token savings are tracked in SQLite via `src/core/tracking.rs`.

For full details see [ARCHITECTURE.md](../docs/contributing/ARCHITECTURE.md) and [docs/contributing/TECHNICAL.md](../docs/contributing/TECHNICAL.md). Module responsibilities are documented in each folder's `README.md` and each file's `//!` doc header.

## Key Conventions

- **Error handling**: `anyhow::Result` with `.context("description")?` — no bare `?`, no `unwrap()` in production. Filters must fall back to raw command on error.
- **Regex**: Always `lazy_static!`, never compile inside a function body.
- **Testing**: Unit tests inside modules (`#[cfg(test)] mod tests`). Fixtures in `tests/fixtures/`. Token savings assertions with `count_tokens()`.
- **Exit codes**: Preserve the underlying command's exit code via `std::process::exit(code)`.
- **Performance**: Startup <10ms (no async runtime), binary <5MB stripped.
- **Branch naming**: `fix(scope):`, `feat(scope):`, `chore(scope):` where scope is the affected component.

For the full contribution workflow, design philosophy, and new-filter checklist, see [CONTRIBUTING.md](../CONTRIBUTING.md).
