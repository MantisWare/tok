use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn find_rs_files() {
    tok_cmd()
        .args(["find", "*.rs", "src/"])
        .assert()
        .success()
        .stdout(predicate::str::contains(".rs"));
}

#[test]
fn find_toml_files() {
    tok_cmd()
        .args(["find", "*.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cargo.toml"));
}
