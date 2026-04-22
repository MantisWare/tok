use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn ls_current_dir() {
    tok_cmd().args(["ls", "."]).assert().success();
}

#[test]
fn ls_long_format() {
    tok_cmd().args(["ls", "-la", "."]).assert().success();
}

#[test]
fn ls_human_readable() {
    tok_cmd().args(["ls", "-lh", "."]).assert().success();
}

#[test]
fn ls_src_directory() {
    tok_cmd().args(["ls", "-l", "src/"]).assert().success();
}

#[test]
fn ls_flag_after_path() {
    tok_cmd().args(["ls", "src/", "-l"]).assert().success();
}

#[test]
fn ls_multiple_paths() {
    tok_cmd()
        .args(["ls", "src/", "scripts/"])
        .assert()
        .success();
}

#[test]
fn ls_shows_hidden_files() {
    tok_cmd()
        .args(["ls", "-a", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains(".git"));
}

#[test]
fn ls_shows_sizes() {
    tok_cmd()
        .args(["ls", "src/"])
        .assert()
        .success()
        .stdout(predicate::str::contains("K").or(predicate::str::contains("B")));
}

#[test]
fn ls_shows_dirs_with_slash() {
    tok_cmd()
        .args(["ls", "."])
        .assert()
        .success()
        .stdout(predicate::str::contains("/"));
}
