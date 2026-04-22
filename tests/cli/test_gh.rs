use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn gh_help() {
    skip_if_missing!("gh");
    tok_cmd()
        .args(["gh", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn gh_pr_list() {
    skip_if_gh_unauthed!();
    tok_cmd().args(["gh", "pr", "list"]).assert().success();
}

#[test]
fn gh_run_list() {
    skip_if_gh_unauthed!();
    tok_cmd().args(["gh", "run", "list"]).assert().success();
}

#[test]
fn gh_issue_list() {
    skip_if_gh_unauthed!();
    tok_cmd().args(["gh", "issue", "list"]).assert().success();
}
