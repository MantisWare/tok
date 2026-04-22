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
fn tree_shows_src() {
    skip_if_missing!("tree");
    tok_cmd()
        .args(["tree", "-L", "1", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("src"));
}
