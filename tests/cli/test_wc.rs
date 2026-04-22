use super::tok_cmd;

#[test]
fn wc_cargo_toml() {
    tok_cmd().args(["wc", "Cargo.toml"]).assert().success();
}
