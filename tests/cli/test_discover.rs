use super::tok_cmd;

#[test]
fn discover_default() {
    tok_cmd().args(["discover"]).assert().success();
}
