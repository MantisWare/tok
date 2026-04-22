use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn prisma_help() {
    skip_if_missing!("prisma");
    tok_cmd()
        .args(["prisma", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
