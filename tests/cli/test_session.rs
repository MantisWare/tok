use super::tok_cmd;

#[test]
fn session_default() {
    tok_cmd().args(["session"]).assert().success();
}
