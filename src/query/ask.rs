//! Ranked retrieval: the query path behind `tok mem ask`.
//!
//! The pipeline is four stages, and each exists because the previous one has a
//! specific failure mode:
//!
//! 1. **Lexical (BM25)** over the pre-weighted sidecar. Finds symbols that
//!    mention the query. Fails when the right answer uses different words than
//!    the question — the usual case for "how does auth work".
//! 2. **Structural (personalized PageRank)** seeded by the lexical hits. Finds
//!    symbols that *matter to* the lexical matches. Fails on its own because
//!    without a lexical anchor it just returns the repo's most central symbols
//!    for every query.
//! 3. **Blend** at [`STRUCTURAL_BLEND`], after normalizing both sides to 0..1 so
//!    the mix is meaningful rather than an artifact of BM25's unbounded scale.
//! 4. **Rescue and penalty.** Strongly connected symbols with no lexical match
//!    are admitted above [`RESCUE_THRESHOLD`] — that is how the caller of the
//!    matched function makes it into the answer. Test symbols are scaled by
//!    [`TEST_PENALTY`], demoted but never excluded, because "how is this
//!    exercised" is a real question whose answer is a test.
//!
//! `--lexical` stops after stage 1. It exists for the case where the user knows
//! the identifier and wants exactly it, with no graph expansion.

use std::collections::{HashMap, HashSet};

use crate::graph::types::{GraphV1, NodeV1};
use crate::query::constants::{BM25_B, BM25_K1, RESCUE_THRESHOLD, STRUCTURAL_BLEND, TEST_PENALTY};
use crate::query::graphrank::{self, RankGraph};
use crate::query::index_file::AskIndex;
use crate::query::tokenize::tokenize;

/// How many lexical hits seed the structural walk.
///
/// Seeding with the entire lexical result would flatten the personalization
/// back into global centrality; seeding with only the top hit makes the whole
/// answer hostage to one possibly-wrong match.
const SEED_LIMIT: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// BM25 only. No graph expansion.
    Lexical,
    /// BM25 seeding a personalized PageRank walk, then blended.
    Structural,
}

#[derive(Debug, Clone)]
pub struct AskOptions {
    pub mode: Mode,
    pub limit: usize,
    /// Restrict results to files whose path contains this substring.
    pub path_filter: Option<String>,
    /// Drop test symbols entirely rather than merely penalizing them.
    pub exclude_tests: bool,
    /// Confine both the results and the structural walk to one scope prefix.
    ///
    /// Unlike `path_filter`, which drops non-matching hits at the end, this
    /// also keeps PageRank inside the scope — otherwise a scope's ranking
    /// would be shaped by symbols it cannot return.
    pub scope_prefix: Option<String>,
}

impl Default for AskOptions {
    fn default() -> Self {
        Self {
            mode: Mode::Structural,
            limit: 20,
            path_filter: None,
            exclude_tests: false,
            scope_prefix: None,
        }
    }
}

/// One ranked answer, carrying enough context to render without another lookup.
#[derive(Debug, Clone)]
pub struct Hit<'a> {
    pub node: &'a NodeV1,
    pub score: f64,
    /// Whether this symbol arrived purely through graph connectivity, with no
    /// query term matching it. Worth surfacing: it tells the reader why an
    /// apparently unrelated symbol is in the list.
    pub rescued: bool,
}

/// Run a query against a graph and its sidecar.
pub fn ask<'a>(
    graph: &'a GraphV1,
    index: &AskIndex,
    query: &str,
    options: &AskOptions,
) -> Vec<Hit<'a>> {
    let terms = tokenize(query);
    if terms.is_empty() || index.doc_count() == 0 {
        return Vec::new();
    }

    let lexical = score_lexical(index, &terms);
    if lexical.is_empty() {
        return Vec::new();
    }

    let nodes = graph.node_index();
    let mut combined: HashMap<&str, (f64, bool)> = lexical
        .iter()
        .map(|(id, score)| (*id, (*score * STRUCTURAL_BLEND_LEXICAL_SHARE, false)))
        .collect();

    if options.mode == Mode::Structural {
        let mut structural = structural_scores(graph, &lexical, options.scope_prefix.as_deref());
        graphrank::normalize(&mut structural);

        for (id, score) in structural {
            let contribution = score * STRUCTURAL_BLEND;

            match combined.get_mut(id) {
                Some(entry) => entry.0 += contribution,
                None => {
                    // No query term touched this symbol; it is here only
                    // because the graph says it is close to something that
                    // matched. Admit it only if that closeness is convincing.
                    if score >= RESCUE_THRESHOLD {
                        combined.insert(id, (contribution, true));
                    }
                }
            }
        }
    }

    let test_ids: HashSet<&str> = index
        .docs
        .iter()
        .filter(|doc| doc.is_test)
        .map(|doc| doc.id.as_str())
        .collect();

    let mut hits: Vec<Hit> = combined
        .into_iter()
        .filter_map(|(id, (score, rescued))| {
            let node = nodes.get(id)?;

            if let Some(filter) = &options.path_filter {
                if !node.file.contains(filter.as_str()) {
                    return None;
                }
            }

            if let Some(prefix) = &options.scope_prefix {
                if !crate::graph::scopes::path_under_prefix(&node.file, prefix) {
                    return None;
                }
            }

            let is_test = test_ids.contains(id);
            if is_test && options.exclude_tests {
                return None;
            }

            let score = if is_test { score * TEST_PENALTY } else { score };

            Some(Hit {
                node,
                score,
                rescued,
            })
        })
        .collect();

    // Ties break on id so repeated runs of the same query agree.
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.node.id.cmp(&b.node.id))
    });
    hits.truncate(options.limit);
    hits
}

/// The lexical half's share of the blend. Named rather than written as
/// `1.0 - STRUCTURAL_BLEND` at the use site so the two halves are visibly
/// complementary.
const STRUCTURAL_BLEND_LEXICAL_SHARE: f64 = 1.0 - STRUCTURAL_BLEND;

/// BM25 over the field-weighted sidecar, normalized to 0..1.
///
/// Returns ids paired with scores, ordered best first.
fn score_lexical<'a>(index: &'a AskIndex, terms: &[String]) -> Vec<(&'a str, f64)> {
    let mut scored: Vec<(&str, f64)> = Vec::new();

    for doc in &index.docs {
        let mut score = 0.0;

        for term in terms {
            let Some(tf) = doc.tf.get(term.as_str()) else {
                continue;
            };

            // Standard BM25: saturating term frequency, length-normalized.
            let length_ratio = if index.avg_len > 0.0 {
                doc.len / index.avg_len
            } else {
                1.0
            };
            let denominator = tf + BM25_K1 * (1.0 - BM25_B + BM25_B * length_ratio);
            if denominator <= 0.0 {
                continue;
            }

            score += index.idf(term) * (tf * (BM25_K1 + 1.0)) / denominator;
        }

        if score > 0.0 {
            scored.push((doc.id.as_str(), score));
        }
    }

    let max = scored.iter().map(|(_, s)| *s).fold(0.0_f64, f64::max);
    if max > 0.0 {
        for entry in &mut scored {
            entry.1 /= max;
        }
    }

    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });
    scored
}

/// Walk the graph from the strongest lexical hits.
///
/// A scope prefix drops edges touching anything outside it, so a scope's
/// structural ranking is computed over its own subgraph.
fn structural_scores<'a>(
    graph: &'a GraphV1,
    lexical: &[(&'a str, f64)],
    scope_prefix: Option<&str>,
) -> HashMap<&'a str, f64> {
    let seeds: HashMap<&str, f64> = lexical.iter().take(SEED_LIMIT).copied().collect();

    let Some(prefix) = scope_prefix else {
        return RankGraph::build(&graph.edges).walk(&seeds);
    };

    let inside: HashSet<&str> = graph
        .nodes
        .iter()
        .filter(|node| crate::graph::scopes::path_under_prefix(&node.file, prefix))
        .map(|node| node.id.as_str())
        .collect();

    let edges: Vec<crate::graph::types::EdgeV1> = graph
        .edges
        .iter()
        .filter(|edge| inside.contains(edge.from.as_str()) && inside.contains(edge.to.as_str()))
        .cloned()
        .collect();

    // Ids are owned by the graph, not by the temporary edge list, so the
    // returned borrows outlive it.
    RankGraph::build(&edges)
        .walk(&seeds)
        .into_iter()
        .filter_map(|(id, score)| inside.get(id).map(|owned| (*owned, score)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EdgeKind, EdgeV1, NodeKind, Span};
    use crate::query::index_file;

    fn node(id: &str, name: &str, file: &str) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            NodeKind::Function,
            name.to_string(),
            file.to_string(),
            Span::new(1, 10),
        )
    }

    fn build(nodes: Vec<NodeV1>, edges: Vec<EdgeV1>) -> (GraphV1, AskIndex) {
        let mut graph = GraphV1::new("repo", "test");
        graph.nodes = nodes;
        graph.edges = edges;
        graph.normalize();
        let index = index_file::build(&graph);
        (graph, index)
    }

    fn ids<'a>(hits: &[Hit<'a>]) -> Vec<&'a str> {
        hits.iter().map(|h| h.node.id.as_str()).collect()
    }

    #[test]
    fn an_exact_name_match_ranks_first() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "src/config.ts"),
                node("b", "helper", "src/config.ts"),
            ],
            Vec::new(),
        );

        let hits = ask(&graph, &index, "parseConfig", &AskOptions::default());

        assert_eq!(hits[0].node.id, "a");
    }

    #[test]
    fn a_query_with_no_matches_returns_nothing() {
        let (graph, index) = build(vec![node("a", "parseConfig", "src/config.ts")], Vec::new());

        assert!(ask(&graph, &index, "zzzznomatch", &AskOptions::default()).is_empty());
    }

    #[test]
    fn an_empty_query_returns_nothing() {
        let (graph, index) = build(vec![node("a", "parseConfig", "src/config.ts")], Vec::new());

        assert!(ask(&graph, &index, "", &AskOptions::default()).is_empty());
        assert!(ask(&graph, &index, "a", &AskOptions::default()).is_empty());
    }

    /// The rescue rule is the reason structural mode exists: `renderPage` shares
    /// no term with the query but calls the function that does.
    #[test]
    fn a_caller_with_no_lexical_match_is_rescued() {
        let (graph, index) = build(
            vec![
                node("target", "parseConfig", "src/config.ts"),
                node("caller", "renderPage", "src/page.ts"),
            ],
            vec![EdgeV1::new("caller", "target", EdgeKind::Calls)],
        );

        let hits = ask(&graph, &index, "parseConfig", &AskOptions::default());

        assert!(ids(&hits).contains(&"caller"));
        let rescued = hits.iter().find(|h| h.node.id == "caller").expect("caller");
        assert!(rescued.rescued);
    }

    #[test]
    fn lexical_mode_does_not_rescue() {
        let (graph, index) = build(
            vec![
                node("target", "parseConfig", "src/config.ts"),
                node("caller", "renderPage", "src/page.ts"),
            ],
            vec![EdgeV1::new("caller", "target", EdgeKind::Calls)],
        );

        let options = AskOptions {
            mode: Mode::Lexical,
            ..AskOptions::default()
        };
        let hits = ask(&graph, &index, "parseConfig", &options);

        assert_eq!(ids(&hits), vec!["target"]);
    }

    /// Demoted, not excluded: a test is a legitimate answer, just a worse one
    /// than the implementation it exercises.
    #[test]
    fn tests_rank_below_equivalent_implementations() {
        let (graph, index) = build(
            vec![
                node("impl", "parseConfig", "src/config.ts"),
                node("spec", "parseConfig", "src/config.test.ts"),
            ],
            Vec::new(),
        );

        let hits = ask(&graph, &index, "parseConfig", &AskOptions::default());

        assert_eq!(hits[0].node.id, "impl");
        assert!(ids(&hits).contains(&"spec"));
    }

    #[test]
    fn excluding_tests_removes_them_entirely() {
        let (graph, index) = build(
            vec![
                node("impl", "parseConfig", "src/config.ts"),
                node("spec", "parseConfig", "src/config.test.ts"),
            ],
            Vec::new(),
        );

        let options = AskOptions {
            exclude_tests: true,
            ..AskOptions::default()
        };
        let hits = ask(&graph, &index, "parseConfig", &options);

        assert_eq!(ids(&hits), vec!["impl"]);
    }

    #[test]
    fn a_path_filter_narrows_results_to_matching_files() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "src/server/config.ts"),
                node("b", "parseConfig", "src/client/config.ts"),
            ],
            Vec::new(),
        );

        let options = AskOptions {
            path_filter: Some("server".to_string()),
            ..AskOptions::default()
        };
        let hits = ask(&graph, &index, "parseConfig", &options);

        assert_eq!(ids(&hits), vec!["a"]);
    }

    #[test]
    fn the_limit_is_respected() {
        let nodes = (0..10)
            .map(|i| node(&format!("n{i}"), "parseConfig", &format!("src/f{i}.ts")))
            .collect();
        let (graph, index) = build(nodes, Vec::new());

        let options = AskOptions {
            limit: 3,
            ..AskOptions::default()
        };

        assert_eq!(ask(&graph, &index, "parseConfig", &options).len(), 3);
    }

    #[test]
    fn the_same_query_ranks_identically_every_run() {
        let (graph, index) = build(
            vec![
                node("a", "cacheGet", "src/cache.ts"),
                node("b", "cachePut", "src/cache.ts"),
                node("c", "cacheClear", "src/cache.ts"),
            ],
            vec![EdgeV1::new("a", "b", EdgeKind::Calls)],
        );

        let first = ids(&ask(&graph, &index, "cache", &AskOptions::default()));
        for _ in 0..5 {
            assert_eq!(
                ids(&ask(&graph, &index, "cache", &AskOptions::default())),
                first
            );
        }
    }

    /// Multi-word queries must reward covering more of the question.
    #[test]
    fn matching_more_query_terms_scores_higher() {
        let (graph, index) = build(
            vec![
                node("both", "parseConfigFile", "src/a.ts"),
                node("one", "parseInput", "src/b.ts"),
            ],
            Vec::new(),
        );

        let hits = ask(&graph, &index, "parse config", &AskOptions::default());

        assert_eq!(hits[0].node.id, "both");
    }

    #[test]
    fn querying_an_empty_graph_is_safe() {
        let (graph, index) = build(Vec::new(), Vec::new());

        assert!(ask(&graph, &index, "anything", &AskOptions::default()).is_empty());
    }

    #[test]
    fn scores_stay_finite_and_ordered() {
        let (graph, index) = build(
            vec![
                node("a", "cache", "src/cache.ts"),
                node("b", "cacheEntry", "src/cache.ts"),
            ],
            vec![EdgeV1::new("a", "b", EdgeKind::Calls)],
        );

        let hits = ask(&graph, &index, "cache", &AskOptions::default());

        assert!(hits.iter().all(|h| h.score.is_finite() && h.score > 0.0));
        for pair in hits.windows(2) {
            assert!(pair[0].score >= pair[1].score);
        }
    }
}
