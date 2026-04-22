use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn cargo_build() {
    skip_if_missing!("cargo");
    tok_cmd().args(["cargo", "build"]).assert().success();
}

#[test]
fn cargo_check() {
    skip_if_missing!("cargo");
    tok_cmd().args(["cargo", "check"]).assert().success();
}

#[test]
fn cargo_clippy() {
    skip_if_missing!("cargo");
    tok_cmd().args(["cargo", "clippy"]).assert().success();
}

#[test]
fn cargo_test_runs() {
    skip_if_missing!("cargo");
    // cargo test may have failures in the project; we just verify tok handles it
    let output = tok_cmd()
        .args(["cargo", "test"])
        .output()
        .expect("failed to execute tok cargo test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    assert!(
        combined.contains("test result:")
            || combined.contains("passed")
            || combined.contains("FAILURES"),
        "cargo test output should contain test results, got: {}",
        &combined[..combined.len().min(500)]
    );
}

#[test]
fn cargo_help() {
    skip_if_missing!("cargo");
    tok_cmd()
        .args(["cargo", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
