use predicates::prelude::*;

use super::tok_cmd;

/// `session` discovers Claude Code transcripts, so the contract depends on the
/// machine: succeed when they exist, fail with the reason when not. CI runners
/// have no `~/.claude/projects`; developer machines do.
#[test]
fn session_default() {
    let projects = dirs::home_dir().map(|home| home.join(".claude").join("projects"));
    let assert = tok_cmd().args(["session"]).assert();
    match projects {
        Some(dir) if dir.exists() => assert.success(),
        _ => assert
            .failure()
            .stderr(predicate::str::contains("projects directory not found")),
    };
}
