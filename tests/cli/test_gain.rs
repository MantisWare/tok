use super::tok_cmd;

#[test]
fn gain_default() {
    tok_cmd().args(["gain"]).assert().success();
}

#[test]
fn gain_history() {
    tok_cmd().args(["gain", "--history"]).assert().success();
}

#[test]
fn gain_by_client() {
    tok_cmd().args(["gain", "--by-client"]).assert().success();
}
