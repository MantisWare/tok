use super::tok_cmd;

#[test]
fn gain_default() {
    tok_cmd().args(["gain"]).assert().success();
}

#[test]
fn gain_history() {
    tok_cmd().args(["gain", "--history"]).assert().success();
}
