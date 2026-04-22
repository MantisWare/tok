use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn curl_json_detect() {
    skip_if_missing!("curl");
    tok_cmd()
        .args(["curl", "https://httpbin.org/json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("string").or(predicate::str::contains("title")));
}

#[test]
fn curl_plain_text() {
    skip_if_missing!("curl");
    tok_cmd()
        .args(["curl", "https://httpbin.org/robots.txt"])
        .assert()
        .success();
}

#[test]
fn curl_help() {
    skip_if_missing!("curl");
    tok_cmd().args(["curl", "--help"]).assert().success();
}
