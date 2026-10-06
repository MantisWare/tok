//! Scope-aware retrieval for monorepos.
//!
//! Ranking a monorepo as one corpus goes wrong in two ways at once. Document
//! frequencies are averaged across unrelated projects, so a term that is
//! distinctive inside one package looks common overall; and a single global
//! ranking lets whichever package happens to be largest crowd out the others,
//! because it simply has more chances to match.
//!
//! The fix is to rank each scope on its own, then merge:
//!
//! 1. **Rank per scope** over a restricted index and subgraph, so both the IDF
//!    statistics and the PageRank walk stay inside the scope.
//! 2. **Gate on participation.** A scope joins the answer only if its best hit
//!    genuinely matches the question — see [`participates`]. Without this every
//!    scope contributes something, and a five-scope repo returns four
//!    irrelevant results out of every five.
//! 3. **Fuse with RRF.** Per-scope scores are each normalized against their own
//!    corpus, so their magnitudes are not comparable. Reciprocal rank fusion
//!    discards the magnitudes and merges on position, which is the only part
//!    that survives normalization intact.
//!
//! Scopes that match weakly are not silently discarded. They come back as
//! [`ScopedResults::also_matched`] so the answer can say where else to look,
//! which is what makes the gate safe to set as tight as it is.

use std::collections::{BTreeMap, HashMap};

use crate::graph::scopes::{self, ScopeV1};
use crate::graph::types::GraphV1;
use crate::query::ask::{self, AskOptions, Hit};
use crate::query::constants::{HIGH_FLOOR, PARTICIPATION_RATIO, RRF_K, STRONG_FLOOR};
use crate::query::index_file::AskIndex;
use crate::query::tokenize::tokenize;

/// A hit together with the scope that produced it.
#[derive(Debug, Clone)]
pub struct ScopedHit<'a> {
    pub hit: Hit<'a>,
    /// Scope prefix. Empty for the root scope.
    pub scope: String,
    /// Child repository, when the query federated across a workspace. Paths in
    /// `hit` stay child-relative, so rendering prepends this to make a pointer
    /// that resolves from where the query was run.
    pub repo: Option<String>,
}

impl ScopedHit<'_> {
    /// How this hit is labelled in output, or `None` for an unlabelled root
    /// hit in a single-project repository.
    pub fn label(&self) -> Option<String> {
        match (&self.repo, self.scope.as_str()) {
            (Some(repo), "") => Some(repo.clone()),
            (Some(repo), scope) => Some(format!("{repo}/{scope}")),
            (None, "") => None,
            (None, scope) => Some(scope.to_string()),
        }
    }

    /// The file path as seen from where the query ran.
    pub fn path(&self) -> String {
        match &self.repo {
            Some(repo) => crate::graph::workspace::federated_path(repo, &self.hit.node.file),
            None => self.hit.node.file.clone(),
        }
    }

    /// `path:line`, ready to paste into an editor.
    pub fn location(&self) -> String {
        format!("{}:{}", self.path(), self.hit.node.span.start)
    }
}

/// One scope's ranked output, waiting to be fused with the others.
pub struct Ranking<'a> {
    pub scope: String,
    pub repo: Option<String>,
    pub hits: Vec<Hit<'a>>,
}

impl Ranking<'_> {
    /// The score of this scope's best hit, used by the participation gate.
    fn top_score(&self) -> f64 {
        self.hits.first().map(|hit| hit.score).unwrap_or(0.0)
    }

    /// How this scope is named when reporting that it matched weakly.
    fn label(&self) -> String {
        match &self.repo {
            Some(repo) => crate::graph::workspace::federated_scope(repo, &self.scope),
            None => self.scope.clone(),
        }
    }
}

/// The outcome of a scope-aware query.
#[derive(Debug, Clone, Default)]
pub struct ScopedResults<'a> {
    pub hits: Vec<ScopedHit<'a>>,
    /// Scopes that matched too weakly to federate, best first. Surfaced so the
    /// caller can suggest narrowing rather than pretending they found nothing.
    pub also_matched: Vec<String>,
    /// How many hits each federated scope contributed.
    pub per_scope: BTreeMap<String, usize>,
}

impl ScopedResults<'_> {
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
}

/// Run a query across every scope in the graph and fuse the results.
///
/// `options.scope_prefix` narrows to a single scope before any of this runs,
/// in which case the fusion machinery is bypassed entirely.
pub fn ask<'a>(
    graph: &'a GraphV1,
    index: &AskIndex,
    query: &str,
    options: &AskOptions,
) -> ScopedResults<'a> {
    let scopes = eligible_scopes(graph, options);

    // One scope means one corpus, which is the plain query path. Taking it
    // here rather than falling through the fusion code keeps single-project
    // repositories byte-identical to their pre-scopes behaviour.
    if scopes.len() <= 1 {
        let prefix = scopes.first().map(|s| s.prefix.clone()).unwrap_or_default();
        let hits = ask::ask(graph, index, query, options);
        let count = hits.len();

        let mut per_scope = BTreeMap::new();
        if count > 0 {
            per_scope.insert(prefix.clone(), count);
        }

        return ScopedResults {
            hits: hits
                .into_iter()
                .map(|hit| ScopedHit {
                    hit,
                    scope: prefix.clone(),
                    repo: None,
                })
                .collect(),
            also_matched: Vec::new(),
            per_scope,
        };
    }

    let (rankings, weak) = rank(graph, index, query, options, &scopes, None);
    finish(rankings, weak, options.limit)
}

/// Rank one repository scope by scope, returning the scopes that qualified and
/// the scopes that matched too weakly to.
///
/// Split out from [`ask`] because workspace federation runs it once per child
/// and then fuses the combined output through the same [`finish`].
pub fn rank<'a>(
    graph: &'a GraphV1,
    index: &AskIndex,
    query: &str,
    options: &AskOptions,
    scopes: &[ScopeV1],
    repo: Option<&str>,
) -> (Vec<Ranking<'a>>, Vec<(String, f64)>) {
    let terms = tokenize(query);
    if terms.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let owners = scope_owners(graph, scopes);
    let mut rankings = Vec::new();
    let mut weak = Vec::new();

    for scope in scopes {
        // Over-fetch: a hit ranked seventh in its scope can still place first
        // overall, and truncating before fusion would lose it.
        let scoped_options = AskOptions {
            limit: options.limit.saturating_mul(4).max(20),
            scope_prefix: Some(scope.prefix.clone()),
            ..options.clone()
        };

        let restricted = index.restrict(|id| {
            owners
                .get(id)
                .is_some_and(|prefix| *prefix == scope.prefix.as_str())
        });
        if restricted.doc_count() == 0 {
            continue;
        }

        let hits = ask::ask(graph, &restricted, query, &scoped_options);
        let Some(best) = hits.first() else {
            continue;
        };
        let qualifies = participates(&restricted, &terms, best);
        let top = best.score;

        let ranking = Ranking {
            scope: scope.prefix.clone(),
            repo: repo.map(str::to_string),
            hits,
        };

        if qualifies {
            rankings.push(ranking);
        } else {
            weak.push((ranking.label(), top));
        }
    }

    (rankings, weak)
}

/// Apply the participation ratio, fuse, and assemble the final answer.
pub fn finish<'a>(
    mut rankings: Vec<Ranking<'a>>,
    weak: Vec<(String, f64)>,
    limit: usize,
) -> ScopedResults<'a> {
    let mut also_matched = weak;
    also_matched.extend(gate(&mut rankings));

    also_matched.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    also_matched.dedup_by(|a, b| a.0 == b.0);

    let mut results = collect(rankings, limit);
    results.also_matched = also_matched.into_iter().map(|(scope, _)| scope).collect();
    results
}

/// Whether a scope's best hit answers the question well enough to federate.
///
/// Two ways to qualify, because one test alone fails a real case:
///
/// - A **name** match above [`STRONG_FLOOR`]. Someone asking about
///   `parseConfig` wants the symbol called that, wherever it lives.
/// - Broad coverage above [`HIGH_FLOOR`] across every indexed field. This is
///   the recall valve for questions phrased as prose, which match doc comments
///   and signatures rather than any single name.
///
/// Both are measured as the IDF-weighted share of query terms matched, so a
/// scope cannot qualify by matching only the common words in the question.
fn participates(index: &AskIndex, terms: &[String], best: &Hit<'_>) -> bool {
    let total: f64 = terms.iter().map(|term| index.idf(term)).sum();
    if total <= 0.0 {
        return false;
    }

    let name_terms = tokenize(&best.hit_name());
    let strong: f64 = terms
        .iter()
        .filter(|term| name_terms.contains(term))
        .map(|term| index.idf(term))
        .sum();

    if strong / total >= STRONG_FLOOR {
        return true;
    }

    let Some(doc) = index.docs.iter().find(|doc| doc.id == best.node.id) else {
        return false;
    };
    let covered: f64 = terms
        .iter()
        .filter(|term| doc.tf.contains_key(term.as_str()))
        .map(|term| index.idf(term))
        .sum();

    covered / total >= HIGH_FLOOR
}

/// Drop scopes whose best hit is far behind the strongest scope's.
///
/// A safety net behind [`participates`], for the case where several scopes
/// each contain a legitimate but much weaker match. Returns the scopes it
/// removed so they can be reported rather than vanish.
fn gate(rankings: &mut Vec<Ranking<'_>>) -> Vec<(String, f64)> {
    if rankings.len() <= 1 {
        return Vec::new();
    }

    let best = rankings
        .iter()
        .map(Ranking::top_score)
        .fold(0.0_f64, f64::max);
    let floor = best * PARTICIPATION_RATIO;

    let mut dropped = Vec::new();
    rankings.retain(|ranking| {
        let top = ranking.top_score();
        if top < floor {
            dropped.push((ranking.label(), top));
            return false;
        }
        true
    });

    dropped
}

/// Merge per-scope rankings by reciprocal rank fusion.
fn collect<'a>(rankings: Vec<Ranking<'a>>, limit: usize) -> ScopedResults<'a> {
    // Keyed by repo and id together: two child repositories can legitimately
    // hold different symbols that mint the same id.
    let mut fused: HashMap<(Option<String>, String), (f64, ScopedHit<'a>)> = HashMap::new();

    for ranking in rankings {
        for (position, hit) in ranking.hits.into_iter().enumerate() {
            let contribution = 1.0 / (RRF_K + (position + 1) as f64);
            let key = (ranking.repo.clone(), hit.node.id.clone());

            match fused.get_mut(&key) {
                // The same symbol reachable from two scopes is more relevant,
                // not less, so contributions add; attribution goes to whichever
                // scope ranked it higher.
                Some(entry) => entry.0 += contribution,
                None => {
                    fused.insert(
                        key,
                        (
                            contribution,
                            ScopedHit {
                                hit,
                                scope: ranking.scope.clone(),
                                repo: ranking.repo.clone(),
                            },
                        ),
                    );
                }
            }
        }
    }

    let mut ordered: Vec<(f64, ScopedHit<'a>)> = fused.into_values().collect();
    ordered.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.location().cmp(&b.1.location()))
            .then_with(|| a.1.hit.node.id.cmp(&b.1.hit.node.id))
    });
    ordered.truncate(limit);

    let mut per_scope: BTreeMap<String, usize> = BTreeMap::new();
    let mut hits = Vec::with_capacity(ordered.len());

    for (score, mut scoped) in ordered {
        let key = scoped.label().unwrap_or_default();
        *per_scope.entry(key).or_insert(0) += 1;
        // Fused scores replace the per-scope ones, which were each normalized
        // against a different corpus and so cannot be compared side by side.
        scoped.hit.score = score;
        hits.push(scoped);
    }

    ScopedResults {
        hits,
        also_matched: Vec::new(),
        per_scope,
    }
}

/// The scopes a query may draw on, after any `--in` narrowing.
fn eligible_scopes(graph: &GraphV1, options: &AskOptions) -> Vec<ScopeV1> {
    let scopes = graph.scopes();

    let Some(prefix) = &options.scope_prefix else {
        return scopes;
    };

    // Scopes *inside* the prefix only. Narrowing to `packages` should still
    // fuse `packages/api` against `packages/web`, but must not readmit the root
    // scope, which owns the rest of the repository.
    let narrowed: Vec<ScopeV1> = scopes
        .iter()
        .filter(|scope| scopes::path_under_prefix(&scope.prefix, prefix))
        .cloned()
        .collect();

    // A prefix that names a directory inside a scope rather than a scope
    // itself still has to resolve to something to search.
    if narrowed.is_empty() {
        return vec![ScopeV1 {
            prefix: prefix.clone(),
            label: prefix.clone(),
            markers: Vec::new(),
        }];
    }

    narrowed
}

/// Map every node id to the prefix of the scope owning it, resolved once per
/// query rather than per scope.
fn scope_owners<'a>(graph: &'a GraphV1, scopes: &'a [ScopeV1]) -> HashMap<&'a str, &'a str> {
    graph
        .nodes
        .iter()
        .map(|node| {
            (
                node.id.as_str(),
                scopes::scope_of(&node.file, scopes).prefix.as_str(),
            )
        })
        .collect()
}

impl Hit<'_> {
    /// The symbol's name, for the strength gate.
    fn hit_name(&self) -> String {
        self.node.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EdgeKind, EdgeV1, NodeKind, NodeV1, Span};
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

    fn scope(prefix: &str) -> ScopeV1 {
        ScopeV1 {
            prefix: prefix.to_string(),
            label: prefix.to_string(),
            markers: Vec::new(),
        }
    }

    fn build(nodes: Vec<NodeV1>, edges: Vec<EdgeV1>, scopes: Vec<ScopeV1>) -> (GraphV1, AskIndex) {
        let mut graph = GraphV1::new("repo", "test");
        graph.nodes = nodes;
        graph.edges = edges;
        graph.scopes = scopes;
        graph.normalize();
        let index = index_file::build(&graph);
        (graph, index)
    }

    fn ids(results: &ScopedResults<'_>) -> Vec<String> {
        results.hits.iter().map(|h| h.hit.node.id.clone()).collect()
    }

    /// A repo with no sub-projects must behave exactly as it did before scopes
    /// existed, fusion included — which means no fusion at all.
    #[test]
    fn a_single_scope_repo_takes_the_plain_query_path() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "src/config.ts"),
                node("b", "helper", "src/util.ts"),
            ],
            Vec::new(),
            Vec::new(),
        );

        let scoped = ask(&graph, &index, "parseConfig", &AskOptions::default());
        let plain = ask::ask(&graph, &index, "parseConfig", &AskOptions::default());

        assert_eq!(
            ids(&scoped),
            plain.iter().map(|h| h.node.id.clone()).collect::<Vec<_>>()
        );
        assert!(scoped.also_matched.is_empty());
    }

    #[test]
    fn every_hit_carries_the_scope_that_produced_it() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "api/config.ts"),
                node("b", "parseConfig", "web/config.ts"),
            ],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let results = ask(&graph, &index, "parseConfig", &AskOptions::default());

        for hit in &results.hits {
            let expected = if hit.hit.node.id == "a" { "api" } else { "web" };
            assert_eq!(hit.scope, expected);
        }
    }

    #[test]
    fn a_matching_symbol_in_each_scope_is_returned_from_both() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "api/config.ts"),
                node("b", "parseConfig", "web/config.ts"),
            ],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let results = ask(&graph, &index, "parseConfig", &AskOptions::default());

        assert_eq!(results.per_scope.len(), 2);
        assert_eq!(results.hits.len(), 2);
    }

    /// The gate is the point of the whole module: a scope with nothing to say
    /// should not fill half the answer.
    #[test]
    fn a_scope_with_no_real_match_does_not_federate() {
        let (graph, index) = build(
            vec![
                node("hit", "parseConfig", "api/config.ts"),
                // Shares the path token "web" only, never the query term.
                node("miss", "unrelatedThing", "web/other.ts"),
            ],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let results = ask(&graph, &index, "parseConfig", &AskOptions::default());

        assert_eq!(ids(&results), vec!["hit"]);
        assert!(!results.per_scope.contains_key("web"));
    }

    #[test]
    fn a_gated_out_scope_is_reported_rather_than_hidden() {
        let (graph, index) = build(
            vec![
                node("hit", "parseConfig", "api/config.ts"),
                node("weak", "config", "web/thing.ts"),
            ],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let results = ask(&graph, &index, "parseConfig zzz", &AskOptions::default());

        assert!(ids(&results).contains(&"hit".to_string()));
        assert!(!results.hits.iter().any(|h| h.hit.node.id == "weak"));
        assert!(results.also_matched.contains(&"web".to_string()));
    }

    #[test]
    fn narrowing_to_one_scope_searches_only_that_scope() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "api/config.ts"),
                node("b", "parseConfig", "web/config.ts"),
            ],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let options = AskOptions {
            scope_prefix: Some("web".to_string()),
            ..AskOptions::default()
        };
        let results = ask(&graph, &index, "parseConfig", &options);

        assert_eq!(ids(&results), vec!["b"]);
    }

    #[test]
    fn narrowing_suppresses_the_also_matched_hint() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "api/config.ts"),
                node("b", "parseConfig", "web/config.ts"),
            ],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let options = AskOptions {
            scope_prefix: Some("web".to_string()),
            ..AskOptions::default()
        };

        assert!(ask(&graph, &index, "parseConfig", &options)
            .also_matched
            .is_empty());
    }

    #[test]
    fn narrowing_to_a_directory_inside_a_scope_still_searches() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "api/inner/config.ts"),
                node("b", "parseConfig", "api/outer/config.ts"),
            ],
            Vec::new(),
            vec![scope("api"), ScopeV1::root()],
        );

        let options = AskOptions {
            scope_prefix: Some("api/inner".to_string()),
            ..AskOptions::default()
        };

        assert_eq!(
            ids(&ask(&graph, &index, "parseConfig", &options)),
            vec!["a"]
        );
    }

    /// PageRank confined to a scope: `caller` must be rescued in its own scope
    /// and must not drag in the identically-named symbol next door.
    #[test]
    fn the_structural_walk_does_not_cross_scope_boundaries() {
        let (graph, index) = build(
            vec![
                node("target", "parseConfig", "api/config.ts"),
                node("caller", "renderPage", "api/page.ts"),
                node("outsider", "renderPage", "web/page.ts"),
            ],
            vec![
                EdgeV1::new("caller", "target", EdgeKind::Calls),
                EdgeV1::new("outsider", "target", EdgeKind::Calls),
            ],
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let results = ask(&graph, &index, "parseConfig", &AskOptions::default());
        let returned = ids(&results);

        assert!(returned.contains(&"caller".to_string()));
        assert!(!returned.contains(&"outsider".to_string()));
    }

    #[test]
    fn the_limit_applies_to_the_fused_result_not_to_each_scope() {
        let mut nodes = Vec::new();
        for i in 0..6 {
            nodes.push(node(
                &format!("a{i}"),
                "parseConfig",
                &format!("api/f{i}.ts"),
            ));
            nodes.push(node(
                &format!("w{i}"),
                "parseConfig",
                &format!("web/f{i}.ts"),
            ));
        }
        let (graph, index) = build(
            nodes,
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let options = AskOptions {
            limit: 5,
            ..AskOptions::default()
        };

        assert_eq!(ask(&graph, &index, "parseConfig", &options).hits.len(), 5);
    }

    #[test]
    fn results_are_ordered_by_fused_score() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "api/config.ts"),
                node("b", "parseConfig", "web/config.ts"),
            ],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let results = ask(&graph, &index, "parseConfig", &AskOptions::default());

        for pair in results.hits.windows(2) {
            assert!(pair[0].hit.score >= pair[1].hit.score);
        }
    }

    #[test]
    fn the_same_query_fuses_identically_every_run() {
        let (graph, index) = build(
            vec![
                node("a", "cacheGet", "api/cache.ts"),
                node("b", "cacheGet", "web/cache.ts"),
                node("c", "cachePut", "api/cache.ts"),
            ],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let first = ids(&ask(&graph, &index, "cache", &AskOptions::default()));
        for _ in 0..5 {
            assert_eq!(
                ids(&ask(&graph, &index, "cache", &AskOptions::default())),
                first
            );
        }
    }

    #[test]
    fn a_query_matching_nothing_returns_nothing() {
        let (graph, index) = build(
            vec![node("a", "parseConfig", "api/config.ts")],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        assert!(ask(&graph, &index, "zzzznomatch", &AskOptions::default()).is_empty());
    }

    #[test]
    fn an_empty_query_returns_nothing() {
        let (graph, index) = build(
            vec![node("a", "parseConfig", "api/config.ts")],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        assert!(ask(&graph, &index, "", &AskOptions::default()).is_empty());
    }

    #[test]
    fn a_scope_holding_no_indexed_symbols_is_skipped() {
        let (graph, index) = build(
            vec![node("a", "parseConfig", "api/config.ts")],
            Vec::new(),
            vec![scope("api"), scope("empty"), ScopeV1::root()],
        );

        let results = ask(&graph, &index, "parseConfig", &AskOptions::default());

        assert_eq!(ids(&results), vec!["a"]);
        assert!(!results.per_scope.contains_key("empty"));
    }

    /// Per-scope scores are each normalized against a different corpus, so the
    /// merged ordering must come from fusion rather than from those numbers.
    #[test]
    fn fused_scores_replace_the_incomparable_per_scope_ones() {
        let (graph, index) = build(
            vec![
                node("a", "parseConfig", "api/config.ts"),
                node("b", "parseConfig", "web/config.ts"),
            ],
            Vec::new(),
            vec![scope("api"), scope("web"), ScopeV1::root()],
        );

        let results = ask(&graph, &index, "parseConfig", &AskOptions::default());

        for hit in &results.hits {
            assert!(hit.hit.score <= 1.0 / (RRF_K + 1.0) + f64::EPSILON);
            assert!(hit.hit.score > 0.0);
        }
    }
}
