use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn deps_current_dir() {
    tok_cmd()
        .args(["deps", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cargo").or(predicate::str::contains("dependencies")));
}

#[test]
fn deps_default_path() {
    tok_cmd().args(["deps"]).assert().success();
}
