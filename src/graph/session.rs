//! Getting a usable graph for a query, refreshing it first if that is safe.
//!
//! Every read-side entry point — the `tok mem` retrieval commands and, later,
//! the MCP tools — needs the same thing: the graph as it stands *now*, not as
//! it stood at the last explicit `tok mem index`. An agent edits a file and
//! immediately asks about it; answering from a stale graph is worse than
//! answering slowly.
//!
//! The refresh is incremental, so the common case where nothing changed costs a
//! fingerprint scan rather than a reparse. Three rules keep it from becoming a
//! liability:
//!
//! - **Never block on a peer.** If another process holds the refresh lock, the
//!   query proceeds against the existing graph rather than queueing behind a
//!   full index.
//! - **Never fail a query because a refresh failed.** A build error degrades to
//!   the last good graph; only a completely absent graph is an error, and then
//!   the message says to run `tok mem index`.
//! - **Never refresh when told not to.** `TOK_GRAPH_NO_REFRESH=1` pins the
//!   graph for reproducible benchmarks and CI.

use std::path::Path;

#[cfg(feature = "graph")]
use anyhow::Context;
use anyhow::Result;

use crate::graph::fingerprint::DriftMode;
use crate::graph::store::GraphPaths;
use crate::graph::types::GraphV1;
use crate::graph::{load, refresh};

/// How a graph was obtained, so commands can explain themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Rebuilt during this call.
    Refreshed,
    /// Read from disk without refreshing.
    Cached,
    /// Read from disk because a refresh was skipped.
    Skipped(refresh::SkipReason),
}

#[derive(Debug)]
pub struct Session {
    pub graph: GraphV1,
    pub freshness: Freshness,
}

/// Load the graph for `repo_root`, refreshing first when permitted.
///
/// Returns an error only when no graph can be produced at all.
pub fn open(repo_root: &Path) -> Result<Session> {
    let paths = GraphPaths::new(repo_root);

    if !refresh::is_enabled() {
        return from_disk(
            &paths,
            repo_root,
            Freshness::Skipped(refresh::SkipReason::Disabled),
        );
    }

    let Some(_lock) = refresh::RefreshLock::acquire(&paths)? else {
        return from_disk(
            &paths,
            repo_root,
            Freshness::Skipped(refresh::SkipReason::Busy),
        );
    };

    match rebuild(repo_root) {
        Ok(graph) => Ok(Session {
            graph,
            freshness: Freshness::Refreshed,
        }),
        // A build failure must not take the query down with it; the previous
        // graph is stale but still far better than an error.
        Err(_) => from_disk(&paths, repo_root, Freshness::Cached),
    }
}

/// Load without any refresh attempt. For callers that already hold a graph
/// lock, or that explicitly want the on-disk state.
pub fn open_cached(repo_root: &Path) -> Result<Session> {
    let paths = GraphPaths::new(repo_root);
    from_disk(&paths, repo_root, Freshness::Cached)
}

fn from_disk(paths: &GraphPaths, repo_root: &Path, freshness: Freshness) -> Result<Session> {
    match load::load_usable(paths) {
        Some(graph) => Ok(Session { graph, freshness }),
        None => Err(missing_graph(repo_root)),
    }
}

#[cfg(feature = "graph")]
fn missing_graph(repo_root: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "No code graph for {}. Run `tok mem index` first.",
        repo_root.display()
    )
}

/// Telling someone to run `tok mem index` would waste their time: this build
/// cannot produce a graph at all, and the fix is a different binary.
#[cfg(not(feature = "graph"))]
fn missing_graph(_repo_root: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "This tok was built without the `graph` feature, so the graph-backed \
         commands (ask, skeleton, grep, map, cards, check, mcp) are unavailable. \
         Install a default build to enable them; `tok mem index`, `search`, and \
         `find` work as usual."
    )
}

#[cfg(feature = "graph")]
fn rebuild(repo_root: &Path) -> Result<GraphV1> {
    use crate::graph::{build, store, write};

    let repo_id = repo_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let output = build::build(&build::BuildOptions {
        repo_root,
        repo_id: &repo_id,
        incremental: true,
        drift: drift_mode(),
    })?;

    let paths = GraphPaths::new(repo_root);
    write::write_graph(&paths, &output.graph)?;
    store::write_json(
        &paths.fingerprints(&crate::graph::extractor_stamp()),
        &output.fingerprints,
    )
    .context("Failed to persist fingerprints")?;

    Ok(output.graph)
}

#[cfg(not(feature = "graph"))]
fn rebuild(_repo_root: &Path) -> Result<GraphV1> {
    Err(anyhow::anyhow!("Built without the `graph` feature"))
}

/// `TOK_GRAPH_REFRESH=hash` forces content hashing instead of the size+mtime
/// fast path. Slower, but immune to filesystems with coarse timestamps.
fn drift_mode() -> DriftMode {
    match std::env::var("TOK_GRAPH_REFRESH").as_deref() {
        Ok("hash") => DriftMode::Hash,
        _ => DriftMode::Fast,
    }
}

#[cfg(all(test, feature = "graph"))]
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

    /// Refresh behaviour is controlled by environment variables, which are
    /// process-global while `cargo test` runs these in parallel threads. Every
    /// test here therefore takes this lock: without it, one test setting
    /// `TOK_GRAPH_NO_REFRESH` silently disables refresh inside another.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serialized() -> std::sync::MutexGuard<'static, ()> {
        // A panicking test poisons the lock; the data it guards is the process
        // environment, which the guard below always restores, so recovering is
        // correct rather than merely convenient.
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_var<T>(key: &str, value: Option<&str>, body: impl FnOnce() -> T) -> T {
        let previous = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        let result = body();
        match previous {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        result
    }

    #[test]
    fn opening_an_unindexed_repo_builds_the_graph() {
        let _serial = serialized();
        let dir = repo(&[("a.rs", "pub fn one() {}")]);

        let session = open(dir.path()).expect("session");

        assert_eq!(session.freshness, Freshness::Refreshed);
        assert!(session.graph.nodes.iter().any(|n| n.name == "one"));
    }

    /// The reason auto-refresh exists: an agent edits, then immediately asks.
    #[test]
    fn an_edit_is_visible_without_an_explicit_index() {
        let _serial = serialized();
        let dir = repo(&[("a.rs", "pub fn one() {}")]);
        open(dir.path()).expect("first");

        std::fs::write(dir.path().join("a.rs"), "pub fn one() {}\npub fn two() {}").expect("edit");
        let session = open(dir.path()).expect("second");

        assert!(session.graph.nodes.iter().any(|n| n.name == "two"));
    }

    #[test]
    fn a_deleted_file_disappears_from_the_graph() {
        let _serial = serialized();
        let dir = repo(&[("a.rs", "pub fn one() {}"), ("b.rs", "pub fn two() {}")]);
        open(dir.path()).expect("first");

        std::fs::remove_file(dir.path().join("b.rs")).expect("remove");
        let session = open(dir.path()).expect("second");

        assert!(!session.graph.nodes.iter().any(|n| n.name == "two"));
    }

    #[test]
    fn refresh_can_be_disabled() {
        let _serial = serialized();
        let dir = repo(&[("a.rs", "pub fn one() {}")]);
        open(dir.path()).expect("seed the graph");

        with_var("TOK_GRAPH_NO_REFRESH", Some("1"), || {
            std::fs::write(dir.path().join("a.rs"), "pub fn one() {}\npub fn two() {}")
                .expect("edit");
            let session = open(dir.path()).expect("session");

            assert_eq!(
                session.freshness,
                Freshness::Skipped(refresh::SkipReason::Disabled)
            );
            assert!(!session.graph.nodes.iter().any(|n| n.name == "two"));
        });
    }

    #[test]
    fn a_pinned_repo_with_no_graph_reports_how_to_fix_it() {
        let _serial = serialized();
        let dir = repo(&[("a.rs", "pub fn one() {}")]);

        with_var("TOK_GRAPH_NO_REFRESH", Some("1"), || {
            let error = open(dir.path()).expect_err("no graph to load");
            assert!(error.to_string().contains("tok mem index"));
        });
    }

    /// A peer holding the lock must not make the query wait for its build.
    #[test]
    fn a_held_lock_falls_back_to_the_existing_graph() {
        let _serial = serialized();
        let dir = repo(&[("a.rs", "pub fn one() {}")]);
        open(dir.path()).expect("seed the graph");

        let paths = GraphPaths::new(dir.path());
        let _held = refresh::RefreshLock::try_acquire(&paths)
            .expect("acquire")
            .expect("lock was free");

        let session = open(dir.path()).expect("session");

        assert_eq!(
            session.freshness,
            Freshness::Skipped(refresh::SkipReason::Busy)
        );
        assert!(session.graph.nodes.iter().any(|n| n.name == "one"));
    }

    #[test]
    fn open_cached_never_rebuilds() {
        let _serial = serialized();
        let dir = repo(&[("a.rs", "pub fn one() {}")]);
        open(dir.path()).expect("seed the graph");

        std::fs::write(dir.path().join("a.rs"), "pub fn one() {}\npub fn two() {}").expect("edit");
        let session = open_cached(dir.path()).expect("session");

        assert_eq!(session.freshness, Freshness::Cached);
        assert!(!session.graph.nodes.iter().any(|n| n.name == "two"));
    }

    #[test]
    fn hash_drift_mode_is_selected_by_env() {
        let _serial = serialized();
        with_var("TOK_GRAPH_REFRESH", Some("hash"), || {
            assert_eq!(drift_mode(), DriftMode::Hash);
        });
        with_var("TOK_GRAPH_REFRESH", None, || {
            assert_eq!(drift_mode(), DriftMode::Fast);
        });
    }
}
