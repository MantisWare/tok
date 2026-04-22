use super::tok_cmd;

#[test]
fn go_help() {
    skip_if_missing!("go");
    tok_cmd().args(["go", "--help"]).assert().success();
}

#[test]
fn go_test_help() {
    skip_if_missing!("go");
    tok_cmd().args(["go", "test", "-h"]).assert().success();
}

#[test]
fn go_build_help() {
    skip_if_missing!("go");
    tok_cmd().args(["go", "build", "-h"]).assert().success();
}

#[test]
fn go_vet_help() {
    skip_if_missing!("go");
    tok_cmd().args(["go", "vet", "-h"]).assert().success();
}

#[test]
fn golangci_lint_help() {
    skip_if_missing!("golangci-lint");
    tok_cmd()
        .args(["golangci-lint", "--help"])
        .assert()
        .success();
}
