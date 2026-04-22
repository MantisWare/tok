use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn version_flag() {
    tok_cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("tok"));
}

#[test]
fn help_flag() {
    tok_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn no_args_prints_version() {
    tok_cmd()
        .assert()
        .success()
        .stdout(predicate::str::contains("tok"));
}
