use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn grep_basic_pattern() {
    tok_cmd()
        .args(["grep", "pub fn", "src/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pub fn"));
}

#[test]
fn grep_with_file_type() {
    tok_cmd()
        .args(["grep", "pub fn", "src/", "-t", "rust"])
        .assert()
        .success();
}

#[test]
fn grep_case_insensitive() {
    tok_cmd()
        .args(["grep", "fn", "src/", "-i"])
        .assert()
        .success();
}

#[test]
fn grep_context_lines() {
    tok_cmd()
        .args(["grep", "fn run", "src/", "-A", "2"])
        .assert()
        .success();
}

#[test]
fn grep_no_match() {
    tok_cmd()
        .args(["grep", "zzz_nonexistent_pattern_zzz", "src/"])
        .assert()
        .failure();
}
