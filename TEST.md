# TOK Testing Guide

TOK has three layers of testing: **unit tests** (inline `#[cfg(test)]` modules), **integration tests** (the `tests/cli/` harness), and the **bash smoke suite** (`scripts/test-all.sh`).

## Quick Start

```bash
# Run everything (unit + integration)
cargo test --all

# Run only the CLI integration test harness
cargo test --test cli

# Run only unit tests (inline modules)
cargo test --lib
```

## Integration Test Harness

The integration harness lives in `tests/cli/` and uses [`assert_cmd`](https://crates.io/crates/assert_cmd) + [`predicates`](https://crates.io/crates/predicates) to exercise the compiled `tok` binary end-to-end.

### How It Works

- `tests/cli.rs` is the Cargo integration test entry point (`mod cli;`).
- `tests/cli/mod.rs` declares shared helpers and imports every test module.
- Each `tests/cli/test_*.rs` file covers one command group.

`assert_cmd::Command::cargo_bin("tok")` locates the debug binary automatically — no `cargo install` needed.

### Running Specific Commands

```bash
# All git-related tests
cargo test --test cli test_git

# A single test by full name
cargo test --test cli git_status

# All tests whose name contains "rewrite"
cargo test --test cli rewrite

# Show stdout/stderr from tests (useful for debugging skipped tests)
cargo test --test cli -- --nocapture
```

### Conditional Tests

Tests for external tools (`gh`, `docker`, `tree`, `go`, `python`, `ruby`, `gt`, etc.) skip gracefully when the tool is not installed. With `--nocapture` you will see `SKIP: <tool> not found on PATH` messages.

### Test File Inventory

| File | Commands Covered |
|------|-----------------|
| `test_version_help.rs` | `--version`, `--help`, no-args |
| `test_aws.rs` | `aws` help, sts passthrough (conditional) |
| `test_ls.rs` | `ls` with flags, multiple paths |
| `test_tree.rs` | `tree` with depth, dirs-only |
| `test_read.rs` | `read` with levels, line numbers, stdin, max-lines |
| `test_git.rs` | `git status/log/diff/branch/fetch/stash/worktree` + passthrough |
| `test_gh.rs` | `gh pr/run/issue list` (conditional on auth) |
| `test_cargo.rs` | `cargo build/check/clippy/test` |
| `test_curl.rs` | `curl` JSON + plain text |
| `test_npm.rs` | `npm`, `npx` help |
| `test_pnpm.rs` | `pnpm` help, build, typecheck |
| `test_grep.rs` | `grep` patterns, file types, context |
| `test_find.rs` | `find` glob patterns |
| `test_json.rs` | `json` valid files, schema, TOML rejection |
| `test_deps.rs` | `deps` |
| `test_env.rs` | `env`, `env --filter` |
| `test_log.rs` | `log` with temp log file |
| `test_summary.rs` | `summary` |
| `test_err.rs` | `err` |
| `test_test_runner.rs` | `test` command |
| `test_gain.rs` | `gain`, `gain --history` |
| `test_config.rs` | `config` |
| `test_init.rs` | `init --show` |
| `test_wget.rs` | `wget` (conditional) |
| `test_js_tools.rs` | `tsc`, `lint`, `prettier`, `next`, `playwright` |
| `test_prisma.rs` | `prisma` |
| `test_psql.rs` | `psql` help, version (conditional) |
| `test_vitest.rs` | `vitest` |
| `test_docker.rs` | `docker`, `kubectl` |
| `test_python.rs` | `pytest`, `ruff`, `mypy`, `pip` |
| `test_go.rs` | `go`, `golangci-lint` |
| `test_graphite.rs` | `gt` |
| `test_ruby.rs` | `rspec`, `rubocop`, `rake` |
| `test_global_flags.rs` | `-v`, `-u`, `--skip-env` |
| `test_cc_economics.rs` | `cc-economics` |
| `test_learn.rs` | `learn` |
| `test_rewrite.rs` | `rewrite` + edge cases (TOK_DISABLED, 2>&1, gh --json) |
| `test_verify.rs` | `verify` |
| `test_proxy.rs` | `proxy` |
| `test_discover.rs` | `discover` |
| `test_diff.rs` | `diff` |
| `test_wc.rs` | `wc` |
| `test_smart.rs` | `smart` |
| `test_dotnet.rs` | `dotnet` (conditional) |
| `test_session.rs` | `session` |
| `test_trust.rs` | `trust --list`, `untrust` |
| `test_hook_audit.rs` | `hook-audit`, `hook-audit --since` |
| `test_format.rs` | `format` |
| `test_hook.rs` | `hook gemini`, `hook copilot` |

## Adding Tests for a New Command

1. Create `tests/cli/test_<command>.rs`.
2. Add `mod test_<command>;` to `tests/cli/mod.rs`.
3. Write `#[test]` functions using the shared helpers:

```rust
use super::tok_cmd;

#[test]
fn my_command_works() {
    tok_cmd()
        .args(["my-command", "--flag"])
        .assert()
        .success();
}
```

For commands requiring external tools, use the `skip_if_missing!` macro:

```rust
use super::{skip_if_missing, tok_cmd};

#[test]
fn external_tool_test() {
    skip_if_missing!("external-tool");
    tok_cmd()
        .args(["external-tool", "arg"])
        .assert()
        .success();
}
```

For tests needing temp files, use `assert_fs::TempDir`:

```rust
#[test]
fn test_with_temp_file() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let file = tmp.path().join("data.json");
    std::fs::write(&file, r#"{"key":"value"}"#).unwrap();

    tok_cmd()
        .args(["json", file.to_str().unwrap()])
        .assert()
        .success();
}
```

## Bash Smoke Suite

The legacy bash smoke suite tests against an **installed** `tok` binary:

```bash
cargo install --path .
bash scripts/test-all.sh
```

This is useful for final pre-release validation but is not part of `cargo test`.

## CI Integration

The existing `cargo test --all` in CI automatically picks up the integration harness. No additional configuration is needed. The tests that depend on external tools will skip gracefully in environments where those tools are not installed.

## Prerequisites

- **Required**: Git (tests run inside the tok git repository)
- **Optional**: `tree`, `gh` (authenticated), `docker`, `pnpm`, `npm`, `curl`, `wget`, `go`, `golangci-lint`, `gt`, `pytest`, `ruff`, `mypy`, `pip`, `rspec`, `rubocop`, `rake`, `tsc`, `prettier`, `next`, `playwright`, `prisma`, `vitest`, `dotnet`

Missing optional tools cause individual tests to skip, not fail.
