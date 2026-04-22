use super::tok_cmd;

#[test]
fn diff_two_files() {
    tok_cmd()
        .args(["diff", "Cargo.toml", "LICENSE"])
        .assert()
        .success();
}

#[test]
fn diff_identical_files() {
    tok_cmd()
        .args(["diff", "Cargo.toml", "Cargo.toml"])
        .assert()
        .success();
}
