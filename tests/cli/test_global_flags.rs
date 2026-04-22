use super::tok_cmd;

#[test]
fn ultra_compact_ls() {
    tok_cmd().args(["-u", "ls", "."]).assert().success();
}

#[test]
fn skip_env_npm_help() {
    skip_if_missing!("npm");
    tok_cmd()
        .args(["--skip-env", "npm", "--help"])
        .assert()
        .success();
}

#[test]
fn verbose_ls() {
    tok_cmd().args(["-v", "ls", "."]).assert().success();
}

#[test]
fn double_verbose_ls() {
    tok_cmd().args(["-vv", "ls", "."]).assert().success();
}
