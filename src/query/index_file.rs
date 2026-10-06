//! The `ask-index.json` sidecar: a pre-tokenized, pre-weighted view of the graph.
//!
//! Ranking needs term frequencies, document frequencies, and an average
//! document length. Recomputing those means tokenizing every symbol name, path,
//! signature, and doc comment in the repo on every single query, which blows the
//! sub-10ms startup budget on any real codebase. So the work happens once at
//! index time and lands in a sidecar next to `graph.json`.
//!
//! The sidecar is a **derived cache, never a source of truth**. It is keyed by
//! the same extractor stamp as the graph, and a stale or unreadable sidecar is
//! treated as absent — callers rebuild it from the graph rather than serving
//! wrong results. That is why the loader returns `Option` instead of `Result`.
//!
//! Field weighting is folded in here rather than at query time. A name token
//! contributes [`NAME_MATCH_WEIGHT`] to the stored term frequency and a path
//! token [`PATH_MATCH_WEIGHT`], so the query path runs plain BM25 over an
//! already-weighted corpus. Doing it at write time keeps the hot loop free of
//! per-field branching.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graph::lang::Language;
use crate::graph::store::GraphPaths;
use crate::graph::types::{GraphV1, NodeKind};
use crate::query::constants::{NAME_MATCH_WEIGHT, PATH_MATCH_WEIGHT};
use crate::query::tokenize::{tokenize, tokenize_path};

/// Bumped when the sidecar layout or the weighting scheme changes. An older
/// sidecar then fails validation and is rebuilt instead of misread.
pub const ASK_INDEX_VERSION: u32 = 1;

/// One searchable symbol, with its field-weighted term frequencies already
/// summed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocEntry {
    /// Graph node id, the join key back to the full node record.
    pub id: String,
    /// Weighted term frequencies. Already includes the name and path boosts.
    pub tf: BTreeMap<String, f64>,
    /// Sum of all term frequencies, used for BM25 length normalization.
    pub len: f64,
    /// Whether this symbol lives in a test file, so the query path can apply
    /// the test penalty without re-examining the path.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_test: bool,
}

/// The complete sidecar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AskIndex {
    pub version: u32,
    /// Must match the graph's extractor stamp, otherwise the sidecar describes
    /// symbols that no longer exist in the shape recorded here.
    pub extractor: String,
    pub docs: Vec<DocEntry>,
    /// Number of documents containing each term.
    pub df: BTreeMap<String, u32>,
    /// Mean document length across the corpus.
    pub avg_len: f64,
}

impl AskIndex {
    pub fn doc_count(&self) -> usize {
        self.docs.len()
    }

    /// Inverse document frequency, in the BM25 probabilistic form with the +1
    /// smoothing that keeps the value positive for terms present in most
    /// documents. Without the smoothing a term appearing in over half the
    /// corpus scores negative and actively demotes its own matches.
    pub fn idf(&self, term: &str) -> f64 {
        let n = self.docs.len() as f64;
        let df = f64::from(self.df.get(term).copied().unwrap_or(0));
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// A sub-index over the documents a predicate keeps, with document
    /// frequencies and average length recomputed for that smaller corpus.
    ///
    /// This is what makes per-scope ranking mean anything. "user" is rare, and
    /// so high-signal, inside `billing/`, but common across a whole monorepo;
    /// scoring a scope against corpus-wide statistics would erase exactly the
    /// distinction scoping exists to draw.
    pub fn restrict(&self, keep: impl Fn(&str) -> bool) -> AskIndex {
        let docs: Vec<DocEntry> = self
            .docs
            .iter()
            .filter(|doc| keep(&doc.id))
            .cloned()
            .collect();

        let mut df: BTreeMap<String, u32> = BTreeMap::new();
        let mut total_len = 0.0;

        for doc in &docs {
            total_len += doc.len;
            for term in doc.tf.keys() {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
        }

        let avg_len = if docs.is_empty() {
            0.0
        } else {
            total_len / docs.len() as f64
        };

        AskIndex {
            version: self.version,
            extractor: self.extractor.clone(),
            docs,
            df,
            avg_len,
        }
    }
}

/// Build the sidecar from a graph.
///
/// File nodes are indexed alongside symbols: "where is the cache module" should
/// be able to answer with a file, not only with a symbol inside it. Import
/// nodes are excluded — they are resolution scaffolding, and surfacing an
/// `import` line as a search result is never the answer to a question about
/// behaviour.
pub fn build(graph: &GraphV1) -> AskIndex {
    let mut docs = Vec::with_capacity(graph.nodes.len());
    let mut df: BTreeMap<String, u32> = BTreeMap::new();

    for node in &graph.nodes {
        if node.kind == NodeKind::Import {
            continue;
        }

        let mut tf: BTreeMap<String, f64> = BTreeMap::new();

        for token in tokenize(&node.name) {
            *tf.entry(token).or_insert(0.0) += NAME_MATCH_WEIGHT;
        }

        for token in tokenize_path(&node.file) {
            *tf.entry(token).or_insert(0.0) += PATH_MATCH_WEIGHT;
        }

        // Signature and doc text carry real signal ("returns a cached entry")
        // but at unit weight, so a passing mention never outranks a name match.
        if let Some(signature) = &node.signature {
            for token in tokenize(signature) {
                *tf.entry(token).or_insert(0.0) += 1.0;
            }
        }
        if let Some(doc) = &node.doc {
            for token in tokenize(doc) {
                *tf.entry(token).or_insert(0.0) += 1.0;
            }
        }
        if let Some(crux) = &node.crux {
            for token in tokenize(crux) {
                *tf.entry(token).or_insert(0.0) += 1.0;
            }
        }

        if tf.is_empty() {
            continue;
        }

        for term in tf.keys() {
            *df.entry(term.clone()).or_insert(0) += 1;
        }

        let len = tf.values().sum();
        docs.push(DocEntry {
            id: node.id.clone(),
            tf,
            len,
            is_test: Language::is_test_path(&node.file),
        });
    }

    // Sorting keeps the sidecar byte-stable for a given graph, which matters
    // because ties in the ranking are broken by document order.
    docs.sort_by(|a, b| a.id.cmp(&b.id));

    let avg_len = if docs.is_empty() {
        0.0
    } else {
        docs.iter().map(|d| d.len).sum::<f64>() / docs.len() as f64
    };

    AskIndex {
        version: ASK_INDEX_VERSION,
        extractor: graph.extractor.clone(),
        docs,
        df,
        avg_len,
    }
}

/// Persist the sidecar next to the graph. Failure is not fatal to the caller:
/// a missing sidecar costs a rebuild, not a wrong answer.
pub fn write(paths: &GraphPaths, index: &AskIndex) -> anyhow::Result<()> {
    crate::graph::store::write_json(&paths.ask_index(), index)
}

/// Load the sidecar, returning `None` if it is missing, unreadable, or built by
/// a different extractor than the graph it is supposed to describe.
pub fn load(paths: &GraphPaths, expected_extractor: &str) -> Option<AskIndex> {
    let index: AskIndex = crate::graph::store::read_json(&paths.ask_index())?;

    if index.version != ASK_INDEX_VERSION || index.extractor != expected_extractor {
        return None;
    }

    Some(index)
}

/// Load the sidecar if it is current, otherwise build it from the graph and
/// write it back. This is the entry point every query command should use.
pub fn load_or_build(paths: &GraphPaths, graph: &GraphV1) -> AskIndex {
    if let Some(index) = load(paths, &graph.extractor) {
        return index;
    }

    let index = build(graph);
    // A read-only checkout is a legitimate state; serve the query regardless.
    let _ = write(paths, &index);
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{NodeV1, Span};

    fn graph_with(nodes: Vec<NodeV1>) -> GraphV1 {
        let mut graph = GraphV1::new("repo", "test-extractor");
        graph.nodes = nodes;
        graph.normalize();
        graph
    }

    fn node(id: &str, name: &str, file: &str) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            NodeKind::Function,
            name.to_string(),
            file.to_string(),
            Span::new(1, 5),
        )
    }

    #[test]
    fn name_tokens_outweigh_path_tokens() {
        let index = build(&graph_with(vec![node("a", "cache", "src/other.ts")]));
        let doc = &index.docs[0];

        assert_eq!(doc.tf.get("cache"), Some(&NAME_MATCH_WEIGHT));
        assert_eq!(doc.tf.get("other"), Some(&PATH_MATCH_WEIGHT));
    }

    /// A symbol whose name *and* path both say "cache" should beat one where
    /// only the path does; the weights accumulate rather than replace.
    #[test]
    fn name_and_path_boosts_accumulate() {
        let index = build(&graph_with(vec![
            node("a", "cache", "src/cache.ts"),
            node("b", "other", "src/cache.ts"),
        ]));

        let a = index.docs.iter().find(|d| d.id == "a").expect("doc a");
        let b = index.docs.iter().find(|d| d.id == "b").expect("doc b");

        assert_eq!(
            a.tf.get("cache"),
            Some(&(NAME_MATCH_WEIGHT + PATH_MATCH_WEIGHT))
        );
        assert_eq!(b.tf.get("cache"), Some(&PATH_MATCH_WEIGHT));
    }

    #[test]
    fn document_frequency_counts_documents_not_occurrences() {
        let index = build(&graph_with(vec![
            node("a", "cache", "src/cache.ts"),
            node("b", "store", "src/store.ts"),
        ]));

        assert_eq!(index.df.get("cache"), Some(&1));
        assert_eq!(index.df.get("src"), Some(&2));
    }

    #[test]
    fn idf_stays_positive_for_ubiquitous_terms() {
        let index = build(&graph_with(vec![
            node("a", "one", "src/a.ts"),
            node("b", "two", "src/b.ts"),
            node("c", "three", "src/c.ts"),
        ]));

        // "src" appears in every document; without smoothing this goes negative.
        assert!(index.idf("src") > 0.0);
        assert!(index.idf("one") > index.idf("src"));
    }

    #[test]
    fn import_nodes_are_not_searchable() {
        let mut import = node("i", "lodash", "src/a.ts");
        import.kind = NodeKind::Import;

        let index = build(&graph_with(vec![import, node("a", "run", "src/a.ts")]));

        assert_eq!(index.doc_count(), 1);
        assert_eq!(index.docs[0].id, "a");
    }

    #[test]
    fn test_files_are_flagged_at_index_time() {
        let index = build(&graph_with(vec![
            node("a", "run", "src/cache.test.ts"),
            node("b", "run", "src/cache.ts"),
        ]));

        let a = index.docs.iter().find(|d| d.id == "a").expect("doc a");
        let b = index.docs.iter().find(|d| d.id == "b").expect("doc b");

        assert!(a.is_test);
        assert!(!b.is_test);
    }

    #[test]
    fn an_empty_graph_produces_a_usable_index() {
        let index = build(&graph_with(Vec::new()));

        assert_eq!(index.doc_count(), 0);
        assert_eq!(index.avg_len, 0.0);
        // Must not divide by zero.
        assert!(index.idf("anything").is_finite());
    }

    #[test]
    fn building_the_same_graph_twice_gives_identical_indexes() {
        let graph = graph_with(vec![
            node("b", "beta", "src/b.ts"),
            node("a", "alpha", "src/a.ts"),
        ]);

        assert_eq!(build(&graph), build(&graph));
    }

    #[test]
    fn a_sidecar_from_a_different_extractor_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = GraphPaths::new(dir.path());
        paths.ensure().expect("dirs");

        let graph = graph_with(vec![node("a", "cache", "src/cache.ts")]);
        write(&paths, &build(&graph)).expect("write");

        assert!(load(&paths, "test-extractor").is_some());
        assert!(load(&paths, "different-extractor").is_none());
    }

    #[test]
    fn load_or_build_writes_a_sidecar_that_loads_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = GraphPaths::new(dir.path());
        paths.ensure().expect("dirs");

        let graph = graph_with(vec![node("a", "cache", "src/cache.ts")]);
        let built = load_or_build(&paths, &graph);
        let loaded = load(&paths, &graph.extractor).expect("sidecar on disk");

        assert_eq!(built, loaded);
    }
}
