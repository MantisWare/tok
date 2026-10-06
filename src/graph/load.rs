//! Reading a stored graph back.
//!
//! Every failure mode here resolves to `None`, meaning "rebuild", rather than
//! to an error. A graph is a derived artifact: if it is missing, truncated,
//! written by an incompatible format version, or produced by a different
//! extractor, the correct response is to regenerate it, not to fail the
//! command the user actually asked for.
//!
//! The extractor check is the subtle one. A format-compatible graph produced by
//! older extraction logic parses perfectly and is silently wrong — different
//! ids, different spans, missing edge kinds. Comparing stamps turns that into a
//! rebuild instead of a wrong answer.

use anyhow::Result;

use crate::graph::store::{self, GraphPaths};
use crate::graph::types::GraphV1;

/// Why a stored graph could not be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadFailure {
    Missing,
    /// Unreadable or not valid JSON.
    Corrupt,
    /// Written by a format version this build cannot interpret.
    IncompatibleVersion,
    /// Written by different extraction logic.
    StaleExtractor,
}

/// Load the graph for a repository, if it is usable as-is.
pub fn load_graph(paths: &GraphPaths) -> std::result::Result<GraphV1, LoadFailure> {
    if !paths.graph().exists() {
        return Err(LoadFailure::Missing);
    }

    let graph: GraphV1 = store::read_json(&paths.graph()).ok_or(LoadFailure::Corrupt)?;

    if !graph.is_compatible() {
        return Err(LoadFailure::IncompatibleVersion);
    }

    if graph.extractor != crate::graph::extractor_stamp() {
        return Err(LoadFailure::StaleExtractor);
    }

    Ok(graph)
}

/// Load the graph, or `None` if it needs rebuilding for any reason.
pub fn load_usable(paths: &GraphPaths) -> Option<GraphV1> {
    load_graph(paths).ok()
}

/// Whether a stored graph exists and can be used without a rebuild.
pub fn is_fresh(paths: &GraphPaths) -> bool {
    load_graph(paths).is_ok()
}

/// Load a graph written for another repository root.
pub fn load_from(repo_root: impl AsRef<std::path::Path>) -> Result<Option<GraphV1>> {
    Ok(load_usable(&GraphPaths::new(repo_root)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{NodeKind, NodeV1, Span, GRAPH_FORMAT_VERSION};
    use crate::graph::write::write_graph;
    use tempfile::TempDir;

    fn sample() -> GraphV1 {
        let mut g = GraphV1::new("r", crate::graph::extractor_stamp());
        g.nodes = vec![NodeV1::new(
            "a.rs::f".to_string(),
            NodeKind::Function,
            "f".to_string(),
            "a.rs".to_string(),
            Span::new(1, 3),
        )];
        g
    }

    #[test]
    fn round_trips_a_written_graph() {
        let dir = TempDir::new().expect("tempdir");
        let paths = GraphPaths::new(dir.path());

        let original = sample();
        write_graph(&paths, &original).expect("write");

        let loaded = load_graph(&paths).expect("load");
        assert_eq!(loaded.nodes, original.nodes);
        assert!(is_fresh(&paths));
    }

    #[test]
    fn a_missing_graph_is_reported_as_missing() {
        let dir = TempDir::new().expect("tempdir");
        let paths = GraphPaths::new(dir.path());

        assert_eq!(load_graph(&paths), Err(LoadFailure::Missing));
        assert!(!is_fresh(&paths));
    }

    #[test]
    fn a_truncated_graph_rebuilds_instead_of_failing() {
        let dir = TempDir::new().expect("tempdir");
        let paths = GraphPaths::new(dir.path());
        paths.ensure().expect("mkdir");

        std::fs::write(paths.graph(), b"{\"version\": 1, \"nod").expect("write");

        assert_eq!(load_graph(&paths), Err(LoadFailure::Corrupt));
        assert!(load_usable(&paths).is_none());
    }

    #[test]
    fn a_future_format_version_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        let paths = GraphPaths::new(dir.path());

        let mut g = sample();
        g.version = GRAPH_FORMAT_VERSION + 1;
        crate::graph::write::write_graph_to(&paths.graph(), &g).expect("write");

        assert_eq!(load_graph(&paths), Err(LoadFailure::IncompatibleVersion));
    }

    /// The silent-wrongness case: the file parses fine but was produced by
    /// different extraction logic.
    #[test]
    fn a_graph_from_older_extraction_logic_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        let paths = GraphPaths::new(dir.path());

        let mut g = sample();
        g.extractor = "tok-graph/0/previous".to_string();
        crate::graph::write::write_graph_to(&paths.graph(), &g).expect("write");

        assert_eq!(load_graph(&paths), Err(LoadFailure::StaleExtractor));
        assert!(!is_fresh(&paths), "stale extraction must force a rebuild");
    }

    #[test]
    fn loading_by_repo_root_finds_the_graph() {
        let dir = TempDir::new().expect("tempdir");
        write_graph(&GraphPaths::new(dir.path()), &sample()).expect("write");

        let loaded = load_from(dir.path()).expect("load").expect("present");
        assert_eq!(loaded.nodes.len(), 1);
    }

    #[test]
    fn loading_an_unindexed_repo_yields_nothing() {
        let dir = TempDir::new().expect("tempdir");
        assert!(load_from(dir.path()).expect("load").is_none());
    }
}
