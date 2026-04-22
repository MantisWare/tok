use super::tok_cmd;

#[test]
fn verify_default() {
    tok_cmd().args(["verify"]).assert().success();
}
