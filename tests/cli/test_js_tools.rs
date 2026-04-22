use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn tsc_help() {
    skip_if_missing!("tsc");
    tok_cmd()
        .args(["tsc", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn lint_help() {
    tok_cmd()
        .args(["lint", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn prettier_help() {
    skip_if_missing!("prettier");
    tok_cmd()
        .args(["prettier", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn next_help() {
    skip_if_missing!("next");
    tok_cmd()
        .args(["next", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn playwright_help() {
    skip_if_missing!("playwright");
    tok_cmd()
        .args(["playwright", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
