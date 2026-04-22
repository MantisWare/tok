use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn aws_help() {
    skip_if_missing!("aws");
    tok_cmd()
        .args(["aws", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn aws_sts_get_caller_identity() {
    skip_if_missing!("aws");
    // Just verify tok routes aws commands without crashing; actual auth may fail
    let output = tok_cmd()
        .args(["aws", "sts", "get-caller-identity"])
        .output()
        .expect("failed to execute tok aws");

    assert!(
        output.status.code().is_some(),
        "tok aws should exit gracefully (not signal-killed)"
    );
}
