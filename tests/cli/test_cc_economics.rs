use super::tok_cmd;

#[test]
fn cc_economics_default() {
    tok_cmd().args(["cc-economics"]).assert().success();
}
