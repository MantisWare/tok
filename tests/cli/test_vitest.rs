use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn vitest_help() {
    skip_if_missing!("vitest");
    tok_cmd()
        .args(["vitest", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
