use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn read_cargo_toml() {
    tok_cmd()
        .args(["read", "Cargo.toml"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[package]"));
}

#[test]
fn read_level_none() {
    tok_cmd()
        .args(["read", "--level", "none", "Cargo.toml"])
        .assert()
        .success();
}

#[test]
fn read_level_aggressive() {
    tok_cmd()
        .args(["read", "--level", "aggressive", "Cargo.toml"])
        .assert()
        .success();
}

#[test]
fn read_with_line_numbers() {
    tok_cmd()
        .args(["read", "-n", "Cargo.toml"])
        .assert()
        .success();
}

#[test]
fn read_max_lines() {
    tok_cmd()
        .args(["read", "--max-lines", "5", "Cargo.toml"])
        .assert()
        .success();
}

#[test]
fn read_stdin_pipe() {
    tok_cmd()
        .args(["read", "-"])
        .write_stdin("fn main() {}\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("fn main()"));
}

#[test]
fn read_nonexistent_file() {
    tok_cmd()
        .args(["read", "nonexistent_file_that_does_not_exist.xyz"])
        .assert()
        .failure();
}
