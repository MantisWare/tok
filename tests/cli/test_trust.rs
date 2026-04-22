use super::tok_cmd;

#[test]
fn trust_list() {
    tok_cmd().args(["trust", "--list"]).assert().success();
}

#[test]
fn untrust_default() {
    // untrust with no interactive input should exit gracefully
    let output = tok_cmd()
        .args(["untrust"])
        .output()
        .expect("failed to execute tok untrust");

    assert!(
        output.status.code().is_some(),
        "tok untrust should exit gracefully (not signal-killed)"
    );
}
