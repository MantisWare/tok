# Contributing to tok

Thanks for hanging out with the codebase. For build, run, test, and release workflows from a clone, start with [docs/contributing/DEVELOPMENT.md](docs/contributing/DEVELOPMENT.md). **Cutting a stable release or updating the Homebrew tap?** Follow [docs/contributing/RELEASE.md](docs/contributing/RELEASE.md). For how things are wired, what we expect in tests, and the big picture, peek at [docs/contributing/TECHNICAL.md](docs/contributing/TECHNICAL.md) and [docs/contributing/ARCHITECTURE.md](docs/contributing/ARCHITECTURE.md).

## Building from source

```bash
cargo build --release
```

## `cargo install tok` and crates.io

Yeah, crates.io is messy: the name `tok` might not be **this** tok. Until we’re sure the registry entry is unambiguous, prefer:

```bash
cargo install --git https://github.com/MantisWare/tok
```

or the [installation guide](INSTALL.md) (binaries, Homebrew, the works). If `tok gain` doesn’t show savings stats, you’ve got the wrong binary.

## Pre-commit checks

```bash
cargo fmt --all && cargo clippy --all-targets && cargo test --all
```
