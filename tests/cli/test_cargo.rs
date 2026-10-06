use predicates::prelude::*;

use super::tok_cmd;

/// A minimal cargo project in a tempdir.
///
/// Running `tok cargo build`/`test` in tok's own repository makes cargo relink
/// `target/debug/tok(.exe)` while sibling test processes still have it mapped —
/// a hard failure on Windows (os error 5) and wasted work everywhere else. A
/// fixture project exercises the proxy just as well.
fn fixture_project() -> assert_fs::TempDir {
    let tmp = assert_fs::TempDir::new().expect("tempdir");
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).expect("create src/");
    std::fs::write(
        src.join("main.rs"),
        "fn main() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {}\n}\n",
    )
    .expect("write main.rs");
    tmp
}

#[test]
fn cargo_build() {
    skip_if_missing!("cargo");
    let project = fixture_project();
    tok_cmd()
        .args(["cargo", "build"])
        .current_dir(project.path())
        .assert()
        .success();
}

#[test]
fn cargo_check() {
    skip_if_missing!("cargo");
    tok_cmd().args(["cargo", "check"]).assert().success();
}

#[test]
fn cargo_clippy() {
    skip_if_missing!("cargo");
    tok_cmd().args(["cargo", "clippy"]).assert().success();
}

#[test]
fn cargo_test_runs() {
    skip_if_missing!("cargo");
    // The fixture has one passing test; we verify tok relays cargo's results.
    let project = fixture_project();
    let output = tok_cmd()
        .args(["cargo", "test"])
        .current_dir(project.path())
        .output()
        .expect("failed to execute tok cargo test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    assert!(
        combined.contains("test result:")
            || combined.contains("passed")
            || combined.contains("FAILURES"),
        "cargo test output should contain test results, got: {}",
        &combined[..combined.len().min(500)]
    );
}

#[test]
fn cargo_help() {
    skip_if_missing!("cargo");
    tok_cmd()
        .args(["cargo", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}
