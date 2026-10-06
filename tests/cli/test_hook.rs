use predicates::prelude::*;

use super::tok_cmd;

#[test]
fn hook_help() {
    tok_cmd()
        .args(["hook", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn hook_gemini_no_stdin() {
    // hook gemini reads from stdin; with empty stdin it should exit gracefully
    let output = tok_cmd()
        .args(["hook", "gemini"])
        .write_stdin("")
        .output()
        .expect("failed to execute tok hook gemini");

    assert!(
        output.status.code().is_some(),
        "tok hook gemini should exit gracefully (not signal-killed)"
    );
}

#[test]
fn hook_copilot_no_stdin() {
    let output = tok_cmd()
        .args(["hook", "copilot"])
        .write_stdin("")
        .output()
        .expect("failed to execute tok hook copilot");

    assert!(
        output.status.code().is_some(),
        "tok hook copilot should exit gracefully (not signal-killed)"
    );
}

/// Graph hooks run inside someone else's tool call, so an unindexed or
/// unwritable repository has to be a quiet no-op rather than a failure that
/// surfaces as a broken edit.
#[test]
fn graph_hooks_succeed_on_an_unindexed_repository() {
    let dir = tempfile::tempdir().expect("tempdir");

    for args in [
        vec!["hook", "graph-session", "--json", "--stdin"],
        vec!["hook", "graph-postedit", "--stdin"],
        vec!["hook", "graph-sync"],
    ] {
        let output = tok_cmd()
            .args(&args)
            .current_dir(dir.path())
            .write_stdin("{}")
            .output()
            .unwrap_or_else(|_| panic!("failed to execute tok {}", args.join(" ")));

        assert_eq!(
            output.status.code(),
            Some(0),
            "tok {} should exit 0",
            args.join(" ")
        );
    }
}

/// The post-edit hook is registered on an event that fires constantly, and
/// anything it prints is parsed as a hook directive.
#[test]
fn the_post_edit_hook_prints_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");

    let output = tok_cmd()
        .args(["hook", "graph-postedit", "--stdin"])
        .current_dir(dir.path())
        .write_stdin("{}")
        .output()
        .expect("failed to execute tok hook graph-postedit");

    assert!(
        output.stdout.is_empty(),
        "unexpected stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
