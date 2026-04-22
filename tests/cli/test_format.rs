use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn format_help() {
    tok_cmd()
        .args(["format", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
