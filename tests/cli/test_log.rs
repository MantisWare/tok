use super::tok_cmd;

#[test]
fn log_with_temp_file() {
    let tmp = assert_fs::TempDir::new().unwrap();
    let log_file = tmp.path().join("test.log");

    let mut content = String::new();
    for _ in 0..20 {
        content.push_str("[2025-01-01 12:00:00] INFO: repeated message\n");
    }
    content.push_str("[2025-01-01 12:00:01] ERROR: something failed\n");
    std::fs::write(&log_file, &content).unwrap();

    tok_cmd()
        .args(["log", log_file.to_str().unwrap()])
        .assert()
        .success();
}
