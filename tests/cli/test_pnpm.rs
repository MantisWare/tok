use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn pnpm_help() {
    skip_if_missing!("pnpm");
    tok_cmd()
        .args(["pnpm", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn pnpm_build_help() {
    skip_if_missing!("pnpm");
    tok_cmd()
        .args(["pnpm", "build", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn pnpm_typecheck_help() {
    skip_if_missing!("pnpm");
    tok_cmd()
        .args(["pnpm", "typecheck", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
