use super::tok_cmd;

#[test]
fn config_default() {
    tok_cmd().args(["config"]).assert().success();
}
