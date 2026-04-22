use super::tok_cmd;

#[test]
fn hook_audit_default() {
    tok_cmd().args(["hook-audit"]).assert().success();
}

#[test]
fn hook_audit_since_days() {
    tok_cmd()
        .args(["hook-audit", "--since", "30"])
        .assert()
        .success();
}
