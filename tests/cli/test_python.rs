use super::tok_cmd;

#[test]
fn pytest_help() {
    skip_if_missing!("pytest");
    tok_cmd().args(["pytest", "--help"]).assert().success();
}

#[test]
fn ruff_help() {
    skip_if_missing!("ruff");
    tok_cmd().args(["ruff", "--help"]).assert().success();
}

#[test]
fn mypy_help() {
    skip_if_missing!("mypy");
    tok_cmd().args(["mypy", "--help"]).assert().success();
}

#[test]
fn pip_help() {
    skip_if_missing!("pip");
    tok_cmd().args(["pip", "--help"]).assert().success();
}
