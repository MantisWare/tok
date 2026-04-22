use super::tok_cmd;

#[test]
fn psql_help() {
    skip_if_missing!("psql");
    // psql has help_flag disabled in clap, so test the underlying psql --help passthrough
    let output = tok_cmd()
        .args(["psql", "--help"])
        .output()
        .expect("failed to execute tok psql");

    assert!(
        output.status.code().is_some(),
        "tok psql --help should exit gracefully"
    );
}

#[test]
fn psql_version() {
    skip_if_missing!("psql");
    tok_cmd().args(["psql", "--version"]).assert().success();
}
