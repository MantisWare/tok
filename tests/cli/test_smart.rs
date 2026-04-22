use super::tok_cmd;

#[test]
fn smart_main_rs() {
    tok_cmd().args(["smart", "src/main.rs"]).assert().success();
}

#[test]
fn smart_cargo_toml() {
    tok_cmd().args(["smart", "Cargo.toml"]).assert().success();
}
