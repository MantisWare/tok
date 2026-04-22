use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn json_valid_file() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let json_file = tmp.path().join("test.json");
    std::fs::write(&json_file, r#"{"name":"test","count":42,"items":[1,2,3]}"#).unwrap();

    tok_cmd()
        .args(["json", json_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("string").or(predicate::str::contains("name")));
}

#[test]
fn json_schema_flag() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let json_file = tmp.path().join("test.json");
    std::fs::write(&json_file, r#"{"a":1,"b":"two","c":[true]}"#).unwrap();

    tok_cmd()
        .args(["json", "--schema", json_file.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn json_rejects_toml() {
    tok_cmd().args(["json", "Cargo.toml"]).assert().failure();
}
