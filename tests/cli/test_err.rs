use super::tok_cmd;

#[test]
fn err_echo_ok() {
    tok_cmd().args(["err", "echo", "ok"]).assert().success();
}
