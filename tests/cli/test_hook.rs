use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn hook_help() {
    tok_cmd()
        .args(["hook", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn hook_gemini_no_stdin() {
    // hook gemini reads from stdin; with empty stdin it should exit gracefully
    let output = tok_cmd()
        .args(["hook", "gemini"])
        .write_stdin("")
        .output()
        .expect("failed to execute tok hook gemini");

    assert!(
        output.status.code().is_some(),
        "tok hook gemini should exit gracefully (not signal-killed)"
    );
}

#[test]
fn hook_copilot_no_stdin() {
    let output = tok_cmd()
        .args(["hook", "copilot"])
        .write_stdin("")
        .output()
        .expect("failed to execute tok hook copilot");

    assert!(
        output.status.code().is_some(),
        "tok hook copilot should exit gracefully (not signal-killed)"
    );
}
