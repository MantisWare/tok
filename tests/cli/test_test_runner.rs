use super::tok_cmd;

#[test]
fn test_runner_echo() {
    tok_cmd().args(["test", "echo", "ok"]).assert().success();
}
