use predicates::prelude::*;

use super::tok_cmd;

// `tok rewrite` exits with the count of rewrites performed (non-zero = rewrites found).
// We check stdout content instead of asserting .success().

// --- Basic rewrites ---

#[test]
fn rewrite_git_status() {
    tok_cmd()
        .args(["rewrite", "git status"])
        .assert()
        .stdout(predicate::str::contains("tok git status"));
}

#[test]
fn rewrite_cargo_test() {
    tok_cmd()
        .args(["rewrite", "cargo test"])
        .assert()
        .stdout(predicate::str::contains("tok cargo test"));
}

#[test]
fn rewrite_compound_and() {
    tok_cmd()
        .args(["rewrite", "git status && cargo test"])
        .assert()
        .stdout(predicate::str::contains("tok git status"));
}

#[test]
fn rewrite_pipe_preserved() {
    tok_cmd()
        .args(["rewrite", "git log | head"])
        .assert()
        .stdout(predicate::str::contains("| head"));
}

// --- TOK_DISABLED skip (#345) ---

#[test]
fn rewrite_tok_disabled_skips() {
    tok_cmd()
        .args(["rewrite", "TOK_DISABLED=1 git status"])
        .assert()
        .failure();
}

#[test]
fn rewrite_env_tok_disabled_skips() {
    tok_cmd()
        .args(["rewrite", "FOO=1 TOK_DISABLED=1 cargo test"])
        .assert()
        .failure();
}

// --- 2>&1 preserved (#346) ---

#[test]
fn rewrite_stderr_redirect_preserved() {
    tok_cmd()
        .args(["rewrite", "cargo test 2>&1 | head"])
        .assert()
        .stdout(predicate::str::contains("2>&1"));
}

// --- gh --json skip (#196) ---

#[test]
fn rewrite_gh_json_skips() {
    tok_cmd()
        .args(["rewrite", "gh pr list --json number"])
        .assert()
        .failure();
}

#[test]
fn rewrite_gh_jq_skips() {
    tok_cmd()
        .args(["rewrite", "gh api /repos --jq .name"])
        .assert()
        .failure();
}

#[test]
fn rewrite_gh_template_skips() {
    tok_cmd()
        .args(["rewrite", "gh pr view 1 --template '{{.title}}'"])
        .assert()
        .failure();
}

#[test]
fn rewrite_gh_normal_works() {
    tok_cmd()
        .args(["rewrite", "gh pr list"])
        .assert()
        .stdout(predicate::str::contains("tok gh pr list"));
}
