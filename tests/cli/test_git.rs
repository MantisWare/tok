use predicates::prelude::*;

use super::tok_cmd;

// --- git status ---

#[test]
fn git_status() {
    tok_cmd().args(["git", "status"]).assert().success();
}

#[test]
fn git_status_short() {
    tok_cmd()
        .args(["git", "status", "--short"])
        .assert()
        .success();
}

#[test]
fn git_status_short_flag() {
    tok_cmd().args(["git", "status", "-s"]).assert().success();
}

#[test]
fn git_status_porcelain() {
    tok_cmd()
        .args(["git", "status", "--porcelain"])
        .assert()
        .success();
}

// --- git log ---

#[test]
fn git_log() {
    tok_cmd().args(["git", "log"]).assert().success();
}

#[test]
fn git_log_limited() {
    tok_cmd()
        .args(["git", "log", "--", "-5"])
        .assert()
        .success();
}

// --- git diff ---

#[test]
fn git_diff() {
    tok_cmd().args(["git", "diff"]).assert().success();
}

#[test]
fn git_diff_stat() {
    tok_cmd().args(["git", "diff", "--stat"]).assert().success();
}

// --- git branch ---

#[test]
fn git_branch() {
    tok_cmd().args(["git", "branch"]).assert().success();
}

// --- git fetch ---

#[test]
fn git_fetch() {
    tok_cmd().args(["git", "fetch"]).assert().success();
}

// --- git stash ---

#[test]
fn git_stash_list() {
    tok_cmd().args(["git", "stash", "list"]).assert().success();
}

// --- git worktree ---

#[test]
fn git_worktree_list() {
    tok_cmd()
        .args(["git", "worktree", "list"])
        .assert()
        .success();
}

// --- passthrough: subcommands tok doesn't filter ---

#[test]
fn git_tag_list() {
    tok_cmd().args(["git", "tag", "--list"]).assert().success();
}

#[test]
fn git_remote_verbose() {
    tok_cmd().args(["git", "remote", "-v"]).assert().success();
}

#[test]
fn git_rev_parse_head() {
    tok_cmd()
        .args(["git", "rev-parse", "HEAD"])
        .assert()
        .success()
        .stdout(predicate::str::is_match("[0-9a-f]{40}").unwrap());
}

// --- global git flags ---

#[test]
fn git_no_pager_log() {
    tok_cmd()
        .args(["git", "--no-pager", "log"])
        .assert()
        .success();
}
