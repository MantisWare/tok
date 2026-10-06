//! Deterministic serialization of the graph.
//!
//! Byte-stability is a requirement rather than a nicety: `[graph] commit_graph
//! = true` puts `graph.json` under version control, and a graph that reorders
//! itself between runs would produce a large diff on every index even when the
//! code did not change.
//!
//! [`crate::graph::GraphV1::normalize`] supplies the ordering; this module only
//! guarantees the write itself is atomic.

use std::path::Path;

use anyhow::Result;

use crate::graph::store::{self, GraphPaths};
use crate::graph::types::GraphV1;

/// Normalize and write a graph to `.tok/graph/graph.json`.
pub fn write_graph(paths: &GraphPaths, graph: &GraphV1) -> Result<()> {
    paths.ensure()?;

    // Normalizing a clone rather than requiring `&mut` keeps callers from
    // having to remember the ordering contract.
    let mut normalized = graph.clone();
    normalized.normalize();

    write_graph_to(&paths.graph(), &normalized)
}

/// Write a graph to an explicit path, assuming it is already normalized.
pub fn write_graph_to(path: &Path, graph: &GraphV1) -> Result<()> {
    store::write_json(path, graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EdgeKind, EdgeV1, NodeKind, NodeV1, Span};
    use tempfile::TempDir;

    fn node(id: &str) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            NodeKind::Function,
            id.to_string(),
            "a.rs".to_string(),
            Span::new(1, 2),
        )
    }

    fn graph(order: &[&str]) -> GraphV1 {
        let mut g = GraphV1::new("r", "stamp");
        g.nodes = order.iter().map(|id| node(id)).collect();
        g.edges = vec![EdgeV1::new("b", "a", EdgeKind::Calls)];
        g
    }

    #[test]
    fn writes_a_graph_file() {
        let dir = TempDir::new().expect("tempdir");
        let paths = GraphPaths::new(dir.path());

        write_graph(&paths, &graph(&["a", "b"])).expect("write");
        assert!(paths.graph().exists());
    }

    /// The property that makes `commit_graph` viable: extraction order must not
    /// leak into the file.
    #[test]
    fn traversal_order_does_not_change_the_bytes() {
        let dir = TempDir::new().expect("tempdir");
        let paths = GraphPaths::new(dir.path());

        write_graph(&paths, &graph(&["c", "a", "b"])).expect("write");
        let first = std::fs::read(paths.graph()).expect("read");

        write_graph(&paths, &graph(&["b", "c", "a"])).expect("write");
        let second = std::fs::read(paths.graph()).expect("read");

        assert_eq!(first, second);
    }

    #[test]
    fn writing_creates_the_graph_directory() {
        let dir = TempDir::new().expect("tempdir");
        let paths = GraphPaths::new(dir.path().join("nested/repo"));

        write_graph(&paths, &graph(&["a"])).expect("write");
        assert!(paths.cache_dir().exists());
    }

    #[test]
    fn rewriting_replaces_the_previous_graph() {
        let dir = TempDir::new().expect("tempdir");
        let paths = GraphPaths::new(dir.path());

        write_graph(&paths, &graph(&["a", "b", "c"])).expect("write");
        write_graph(&paths, &graph(&["a"])).expect("rewrite");

        let text = std::fs::read_to_string(paths.graph()).expect("read");
        assert!(!text.contains("\"c\""), "stale nodes survived: {text}");
    }
}
