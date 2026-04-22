use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn init_show() {
    tok_cmd().args(["init", "--show"]).assert().success();
}

#[test]
fn init_show_contains_version() {
    tok_cmd()
        .args(["init", "--show"])
        .assert()
        .success()
        .stdout(predicate::str::contains("version"));
}
