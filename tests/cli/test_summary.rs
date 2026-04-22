use super::tok_cmd;

#[test]
fn summary_echo() {
    tok_cmd()
        .args(["summary", "echo", "hello"])
        .assert()
        .success();
}
