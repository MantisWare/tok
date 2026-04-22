use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn dotnet_help() {
    skip_if_missing!("dotnet");
    tok_cmd()
        .args(["dotnet", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn dotnet_build_help() {
    skip_if_missing!("dotnet");
    tok_cmd()
        .args(["dotnet", "build", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn dotnet_test_help() {
    skip_if_missing!("dotnet");
    tok_cmd()
        .args(["dotnet", "test", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
