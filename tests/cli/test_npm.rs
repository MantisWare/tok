use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn npm_help() {
    skip_if_missing!("npm");
    tok_cmd()
        .args(["npm", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:").or(predicate::str::contains("npm")));
}

#[test]
fn npx_help() {
    skip_if_missing!("npx");
    tok_cmd().args(["npx", "--help"]).assert().success();
}
