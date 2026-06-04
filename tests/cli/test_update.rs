use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn update_check_runs() {
    tok_cmd()
        .args(["update", "--check"])
        .assert()
        .code(predicate::in_iter([0, 1]));
}

#[test]
fn update_is_meta_command() {
    tok_cmd()
        .args(["update", "--bad-flag-xyz"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}
