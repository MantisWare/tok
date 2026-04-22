use super::tok_cmd;

#[test]
fn wget_stdout() {
    skip_if_missing!("wget");
    tok_cmd()
        .args(["wget", "https://httpbin.org/robots.txt", "-O", "-"])
        .assert()
        .success();
}
