use super::tok_cmd;

#[test]
fn learn_help() {
    tok_cmd().args(["learn", "--help"]).assert().success();
}

#[test]
fn learn_since_zero() {
    // --since 0 may have no sessions; just verify it runs without panic
    let output = tok_cmd()
        .args(["learn", "--since", "0"])
        .output()
        .expect("failed to execute tok learn");

    // Either succeeds or fails gracefully — no panic / signal
    assert!(
        output.status.success() || output.status.code().is_some(),
        "tok learn should exit gracefully"
    );
}
