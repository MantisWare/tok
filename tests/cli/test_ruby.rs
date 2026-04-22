use super::tok_cmd;

#[test]
fn rspec_help() {
    skip_if_missing!("rspec");
    tok_cmd().args(["rspec", "--help"]).assert().success();
}

#[test]
fn rubocop_help() {
    skip_if_missing!("rubocop");
    tok_cmd().args(["rubocop", "--help"]).assert().success();
}

#[test]
fn rake_help() {
    skip_if_missing!("rake");
    tok_cmd().args(["rake", "--help"]).assert().success();
}
