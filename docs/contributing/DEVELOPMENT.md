# Development guide

How to build, run, test, and ship **tok** from a clone of this repository. For architecture and module patterns, see [ARCHITECTURE.md](ARCHITECTURE.md) and [TECHNICAL.md](TECHNICAL.md). End-user install options live in [INSTALL.md](../../INSTALL.md).

## Prerequisites

- **Rust**: stable toolchain via [rustup](https://rustup.rs/) (`rustc`, `cargo`, `rustfmt`, `clippy`).
- **OS**: Linux, macOS, or Windows (CI exercises all three).
- **Optional** (smoke tests / local packaging):
  - `bash` for `scripts/*.sh`
  - [cargo-deb](https://github.com/kornelski/cargo-deb) for `.deb` packages
  - [cargo-generate-rpm](https://github.com/cat-in-136/cargo-generate-rpm) for `.rpm` packages

## Build

From the repository root:

```bash
# Fast iteration (debug binary, larger and slower than release)
cargo build

# Binary path
./target/debug/tok --version
```

```bash
# Production-like binary (matches what users get from releases)
cargo build --release

./target/release/tok --version
```

If your shell wraps `cargo` or prints errors like `command not found: -e`, run builds in a plain subshell:

```bash
bash -lc 'cd /path/to/tok && cargo build'
```

## Run without installing

```bash
# Preferred while hacking: run the crate’s binary with arguments after `--`
cargo run -- --help
cargo run -- git status
cargo run -- gain
```

Or invoke the built artifact directly:

```bash
./target/debug/tok git log --oneline -5
```

## Install locally (put `tok` on your PATH)

Useful so scripts and hooks can call `tok` by name:

```bash
cargo install --path .
tok --version
```

To refresh after code changes:

```bash
cargo install --path --force .
```

> **Crates.io note:** The name `tok` on crates.io may not refer to this project. Prefer `cargo install --git https://github.com/MantisWare/tok` or install from this path. If `tok gain` does not show token stats, you are not running this binary—see [README.md](../../README.md#verify-installation).

## Test

### Unit and integration tests (required before pushing)

```bash
cargo test --all
```

Run a subset:

```bash
cargo test module_name::
cargo test exact_test_name -- --nocapture
```

### CI parity (what GitHub runs)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets
cargo test --all
```

Convenience (formats, lints, tests in one go):

```bash
cargo fmt --all && cargo clippy --all-targets && cargo test --all
```

### Smoke tests (`scripts/test-all.sh`)

The script expects a **`tok` binary on your `PATH`** (not only `cargo run`):

```bash
cargo install --path .
bash scripts/test-all.sh
```

Other scripts under `scripts/` (benchmarks, install checks, tracking) are documented in their file headers; run them with `bash scripts/<name>.sh` when needed.

### Performance spot-check (optional)

After changing hot paths or startup:

```bash
cargo build --release
hyperfine 'target/release/tok git log -10' --warmup 3
```

## Deploy and release (maintainers)

Shipping is automated in GitHub Actions; local steps are for verification or emergency builds. **Step-by-step:** [RELEASE.md](RELEASE.md) (release-please, secrets, Homebrew tap [MantisWare/homebrew-tap](https://github.com/MantisWare/homebrew-tap)).

### GitHub Actions

- **CI** (`.github/workflows/ci.yml`): on pull requests to `develop` / `master`—format, clippy, tests, security scans, and related gates.
- **CD** (`.github/workflows/cd.yml`): on push to `develop` / `master` or manual dispatch—pre-release / release orchestration (see workflow for branch rules).
- **Release** (`.github/workflows/release.yml`): builds cross-platform archives (`tar.gz` / `zip`), DEB and RPM packages, uploads assets to a GitHub Release, and can update the external Homebrew tap (invoked via workflow call or dispatch with a tag).

### Local packages (optional)

After `cargo build --release`:

```bash
cargo deb          # needs cargo-deb; outputs under target/debian/
cargo generate-rpm # needs cargo-generate-rpm; outputs under target/generate-rpm/
```

A template formula lives under `Formula/tok.rb` in this repo; the **published** formula for `brew install tok` is maintained in **[MantisWare/homebrew-tap](https://github.com/MantisWare/homebrew-tap)** by CI on stable releases.

### Environment variables (builds)

Release builds in CI may set telemetry-related variables (see `.github/workflows/release.yml`). Local builds do not require them unless you are testing that code path.

## Troubleshooting

- **Wrong `tok` binary**: Run `which tok` and `tok --version`; reinstall with `cargo install --path .` from this repo.
- **SQLite / tracking**: Tests and `tok gain` use a local database under the user’s data directory; see [docs/usage/TRACKING.md](../../usage/TRACKING.md) if paths or permissions fail.
- **Hook / init issues**: [docs/TROUBLESHOOTING.md](../../TROUBLESHOOTING.md) and [hooks/README.md](../../hooks/README.md).
