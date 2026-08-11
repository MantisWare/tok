//! Tree-sitter code graph.
//!
//! Builds a symbol graph with real call, import, and inheritance edges, which
//! the regex indexer in [`crate::mem::parser_regex`] cannot produce. The graph
//! is the retrieval substrate for `tok mem ask` and the MCP server.
//!
//! Relationship to the existing `mem` subsystem:
//!
//! - [`crate::mem::graph`] traverses edges already in SQLite. It is unchanged.
//! - This module *produces* those edges, and writes a richer graph to
//!   `.tok/graph/` alongside the SQLite projection.
//!
//! Both stores are written on every index so existing `tok mem` commands keep
//! working untouched while new commands read the richer form.

#[cfg(feature = "graph")]
pub mod build;
pub mod cache;
pub mod config;
pub mod extract;
pub mod fingerprint;
#[cfg(all(test, feature = "graph"))]
mod fixture_tests;
pub mod lang;
pub mod llm;
pub mod load;
pub mod modpath;
pub mod project;
pub mod refresh;
#[cfg(feature = "graph")]
pub mod resolve;
pub mod scopes;
pub mod session;
pub mod store;
pub mod types;
pub mod workspace;
pub mod write;

// Re-exported for the extraction and retrieval layers. Kept unconditionally so
// the `--no-default-features` build compiles the same module tree.
#[allow(unused_imports)]
pub use lang::Language;
#[allow(unused_imports)]
pub use types::{EdgeKind, EdgeV1, GraphV1, NodeKind, NodeV1, Span, GRAPH_FORMAT_VERSION};

/// Identifies the extraction logic that produced a graph.
///
/// Cache entries and stored graphs are keyed partly on this string, so bumping
/// it invalidates them. Bump whenever extraction output changes for unchanged
/// input — new node kinds, different ids, changed spans — otherwise a stale
/// cache will serve results from the previous implementation.
pub fn extractor_stamp() -> String {
    format!("tok-graph/{}/ts", GRAPH_FORMAT_VERSION)
}

/// Whether this build can extract with tree-sitter at all.
pub fn is_available() -> bool {
    cfg!(feature = "graph")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extractor_stamp_includes_format_version() {
        assert!(extractor_stamp().contains(&GRAPH_FORMAT_VERSION.to_string()));
    }

    #[cfg(feature = "graph")]
    #[test]
    fn graph_is_available_in_default_build() {
        assert!(is_available(), "graph must ship on by default");
    }
}
