use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn security_inspect_detects_email_in_file() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.path().join("test_prompt.txt");
    std::fs::write(&file, "Please email john@example.com about the project.").unwrap();

    tok_cmd()
        .args(["security-inspect", file.to_str().unwrap(), "--report"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1"));
}

#[test]
fn security_inspect_detects_api_key() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.path().join("secrets.txt");
    let fake_key = format!("sk_test_{}", "FAKE00000000000000000000");
    std::fs::write(&file, format!("Use key {fake_key} for auth.")).unwrap();

    tok_cmd()
        .args(["security-inspect", file.to_str().unwrap(), "--report"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Findings: 1"));
}

#[test]
fn security_inspect_no_findings_for_clean_text() {
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.path().join("clean.txt");
    std::fs::write(
        &file,
        "This is a perfectly safe prompt about Rust programming.",
    )
    .unwrap();

    tok_cmd()
        .args(["security-inspect", file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("0 finding"));
}

#[test]
fn doctor_slm_runs_without_crash() {
    tok_cmd()
        .args(["doctor", "--slm"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TOK SLM Doctor"));
}

#[test]
fn doctor_without_slm_flag_shows_help() {
    tok_cmd()
        .args(["doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--slm"));
}

#[test]
fn security_flag_accepted_by_proxy() {
    tok_cmd()
        .args(["--security", "proxy", "echo", "test@example.com"])
        .assert()
        .success();
}

#[test]
fn no_security_flag_accepted_by_proxy() {
    tok_cmd()
        .args(["--no-security", "proxy", "echo", "hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn security_mode_flag_accepted() {
    tok_cmd()
        .args([
            "--security",
            "--security-mode",
            "observe",
            "proxy",
            "echo",
            "test",
        ])
        .assert()
        .success();
}
