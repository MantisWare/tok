use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn proxy_echo_hello() {
    tok_cmd()
        .args(["proxy", "echo", "hello"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"));
}

#[test]
fn proxy_passthrough() {
    tok_cmd()
        .args(["proxy", "echo", "world"])
        .assert()
        .success()
        .stdout(predicate::str::contains("world"));
}
