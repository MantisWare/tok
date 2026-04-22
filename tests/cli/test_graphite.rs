use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn gt_help() {
    skip_if_missing!("gt");
    tok_cmd().args(["gt", "--help"]).assert().success();
}

#[test]
fn gt_log_short() {
    skip_if_missing!("gt");
    tok_cmd()
        .args(["gt", "log", "short"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}
