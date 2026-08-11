//! The three graph lifecycle hooks agents call: session start, post-edit, and
//! sync.
//!
//! Each has one job, and the constraint on all three is that they run inside
//! someone else's tool call. A hook that is slow makes the agent feel slow, and
//! a hook that fails must not fail the edit that triggered it — so every error
//! path here degrades to silence rather than propagating.
//!
//! - **session** emits `additional_context`, reusing the agent-memory payload
//!   contract so hosts need no new parsing.
//! - **postedit** refreshes the graph after a file changes, but only if a graph
//!   already exists.
//! - **sync** regenerates the committed markdown under `.tok/map/`.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::Value;

use crate::graph::{session, store::GraphPaths};
use crate::hooks::memory_payload::{MemoryHookAgent, MemoryHookEvent};
use crate::markdown;
use crate::query::map;

/// How much orientation to put in front of the model at session start.
///
/// Small on purpose: this is prepended to every session whether or not it turns
/// out to be relevant, so it competes with the user's actual request for
/// context. Enough to establish the graph exists and how to reach it.
const MAX_HUBS: usize = 5;
const MAX_DIRECTORIES: usize = 6;

/// Session start — emit repository orientation as `additional_context`.
pub fn run_session(agent: Option<&str>, json: bool, read_stdin: bool) -> Result<i32> {
    if read_stdin {
        // The payload carries no query the graph can use, but a host that
        // writes to a full pipe will block, so it still has to be drained.
        drain_stdin();
    }

    let agent = MemoryHookAgent::parse(agent);
    let root = repo_root();

    // Session start is on the critical path of the user's first prompt, so it
    // reads whatever is cached instead of waiting for a build.
    let Some(context) = orientation(&root) else {
        return Ok(0);
    };

    let payload = crate::hooks::memory_payload::format_retrieve_output(
        agent,
        MemoryHookEvent::SessionStart,
        &context,
        estimate_tokens(&context),
        1,
    );

    emit(&payload, json);
    Ok(0)
}

/// Post-edit — bring the graph back in step with a file that just changed.
///
/// Deliberately does nothing when no graph exists yet: a cold build on a large
/// repository would stall the first edit of every session, and the agent has no
/// way to know why its tool call hung.
pub fn run_postedit(read_stdin: bool) -> Result<i32> {
    if read_stdin {
        drain_stdin();
    }

    postedit_in(&repo_root())
}

fn postedit_in(root: &Path) -> Result<i32> {
    if !GraphPaths::new(root).exists() {
        return Ok(0);
    }

    // The refresh is best-effort. A failure here would otherwise surface as a
    // failed edit, which is a much worse outcome than a slightly stale graph —
    // the next query refreshes it anyway.
    let _ = session::open(root);

    Ok(0)
}

/// Sync — regenerate the committed markdown under `.tok/map/`.
pub fn run_sync() -> Result<i32> {
    let root = repo_root();
    if !GraphPaths::new(&root).exists() {
        return Ok(0);
    }

    let Ok(open) = session::open(&root) else {
        return Ok(0);
    };

    let _ = markdown::write::write_all(&root, &open.graph);
    Ok(0)
}

/// A short orientation drawn from the cached graph, or `None` when there is
/// nothing indexed to describe.
fn orientation(root: &Path) -> Option<String> {
    let graph = session::open_cached(root).ok()?.graph;
    if graph.nodes.is_empty() {
        return None;
    }

    let overview = map::build(
        &graph,
        &map::MapOptions {
            max_hubs: MAX_HUBS,
            max_directories: MAX_DIRECTORIES,
            ..map::MapOptions::default()
        },
    );

    let mut out = String::from("## Repository (TOK code graph)\n\n");
    out.push_str(&format!(
        "{} files, {} symbols indexed.",
        overview.file_count, overview.symbol_count
    ));

    if !overview.languages.is_empty() {
        let languages: Vec<String> = overview
            .languages
            .iter()
            .map(|(name, count)| format!("{name} ({count})"))
            .collect();
        out.push_str(&format!(" Languages: {}.", languages.join(", ")));
    }
    out.push_str("\n\n");

    if !overview.directories.is_empty() {
        out.push_str("Layout:\n");
        for dir in &overview.directories {
            out.push_str(&format!(
                "- `{}` — {} files, {} symbols\n",
                dir.path, dir.files, dir.symbols
            ));
        }
        out.push('\n');
    }

    if !overview.hubs.is_empty() {
        out.push_str("Most depended upon:\n");
        for hub in &overview.hubs {
            out.push_str(&format!(
                "- `{}` ({}) — {} dependents\n",
                hub.node.name,
                hub.node.location(),
                hub.dependents
            ));
        }
        out.push('\n');
    }

    out.push_str(
        "Use `tok mem ask \"<question>\"` to find the symbols worth reading, \
         `tok mem skeleton <file>` to outline a file, and `tok mem grep <pattern>` \
         to search. These cost a fraction of reading files.\n",
    );

    Some(out)
}

/// Where the hook is running. Hooks are invoked with the workspace as the
/// working directory by every host TOK supports.
fn repo_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Read and discard stdin. Hosts write the tool payload to a pipe and a hook
/// that never reads it can leave them blocked on a full buffer.
fn drain_stdin() {
    let mut buffer = String::new();
    let _ = std::io::stdin().read_to_string(&mut buffer);
}

/// The usual four-characters-per-token approximation, matching what the memory
/// pack reports so the two numbers are comparable.
fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn emit(payload: &Value, json: bool) {
    if json {
        println!("{payload}");
        return;
    }

    // Without --json the hook is being run by a person, so show the context
    // rather than the envelope it would be wrapped in.
    let context = payload
        .get("additional_context")
        .or_else(|| payload.get("additionalContext"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    println!("{context}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (path, contents) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("mkdir");
            }
            std::fs::write(full, contents).expect("write");
        }
        dir
    }

    /// Build a graph so the cached-read paths have something to find.
    fn indexed(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = repo(files);
        session::open(dir.path()).expect("index");
        dir
    }

    #[test]
    fn orientation_describes_an_indexed_repository() {
        let dir = indexed(&[(
            "a.rs",
            "pub fn helper() {}\npub fn caller() { helper(); }\n",
        )]);

        let context = orientation(dir.path()).expect("orientation");

        assert!(context.contains("symbols indexed"));
        assert!(context.contains("tok mem ask"));
    }

    #[test]
    fn orientation_is_absent_when_nothing_is_indexed() {
        let dir = repo(&[("notes.txt", "no code here")]);

        assert!(orientation(dir.path()).is_none());
    }

    /// Session start sits in front of the user's first prompt, so the
    /// orientation has to stay small enough not to crowd it out.
    #[test]
    fn orientation_stays_within_a_modest_token_budget() {
        let dir = indexed(&[
            ("a.rs", "pub fn one() {}\npub fn two() {}\n"),
            (
                "src/b.rs",
                "pub struct Thing;\npub fn make() -> Thing { Thing }\n",
            ),
            ("src/c.rs", "pub fn third() {}\n"),
        ]);

        let context = orientation(dir.path()).expect("orientation");

        assert!(
            estimate_tokens(&context) < 400,
            "orientation too large: {} tokens",
            estimate_tokens(&context)
        );
    }

    #[test]
    fn postedit_on_an_unindexed_repo_does_nothing() {
        let dir = repo(&[("a.rs", "pub fn one() {}")]);

        // Takes the root explicitly rather than chdir'ing: the working
        // directory is process-global, and `cargo test` runs these in parallel
        // threads that would inherit it.
        let code = postedit_in(dir.path()).expect("postedit");

        assert_eq!(code, 0);
        assert!(!GraphPaths::new(dir.path()).exists(), "built a cold graph");
    }

    #[test]
    fn token_estimates_round_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abc"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }
}
