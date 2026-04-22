use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn env_default() {
    tok_cmd().args(["env"]).assert().success();
}

#[test]
fn env_filter_path() {
    tok_cmd()
        .args(["env", "--filter", "PATH"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PATH"));
}
