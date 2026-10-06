use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn tree_current_dir() {
    skip_if_missing!("tree");
    tok_cmd().args(["tree", "."]).assert().success();
}

#[test]
fn tree_with_depth() {
    skip_if_missing!("tree");
    tok_cmd().args(["tree", "-L", "2", "."]).assert().success();
}

#[test]
fn tree_dirs_only() {
    skip_if_missing!("tree");
    tok_cmd()
        .args(["tree", "-d", "-L", "1", "."])
        .assert()
        .success();
}

#[test]
// On Windows, `tree` resolves to Microsoft's tree.com — a different tool with
// different flags and output than the unix tree this filter shapes.
#[cfg_attr(windows, ignore = "Windows tree.com is not the unix tree")]
fn tree_shows_src() {
    skip_if_missing!("tree");
    tok_cmd()
        .args(["tree", "-L", "1", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("src"));
}
