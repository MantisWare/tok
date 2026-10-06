//! Personalized PageRank over the code graph.
//!
//! Lexical search finds symbols that *mention* the query. This finds symbols
//! that *matter to* those symbols — the caller three hops up that never repeats
//! the word, the interface the match implements. It is what turns a keyword
//! search into a context retriever.
//!
//! Two departures from textbook PageRank, both deliberate:
//!
//! 1. **Damping is [`PAGERANK_DAMPING`] (0.25), not 0.85.** The classic value
//!    is tuned for the web, where the graph is enormous and a long random walk
//!    is the point. A code graph is small and densely connected, so a long walk
//!    converges on the same globally central symbols — the logger, the config
//!    loader — for every query. A low damping factor keeps mass near the seeds,
//!    which is what "relevant to *this* question" means.
//!
//! 2. **The walk is bidirectional with asymmetric weight.** Callers of a match
//!    are usually more informative than callees, but callees still matter, so
//!    edges propagate forwards at full weight and backwards at a discount
//!    rather than being restricted to one direction.
//!
//! The iteration count is capped by [`PAGERANK_ITERATIONS`]. On repo-sized
//! graphs the walk converges long before that; the cap exists so a pathological
//! graph cannot hang a query.

use std::collections::HashMap;

use crate::graph::types::EdgeV1;
use crate::query::constants::{PAGERANK_DAMPING, PAGERANK_ITERATIONS};

/// Weight applied when propagating against an edge's direction.
///
/// Below 1.0 because "who calls this" is a stronger signal of relevance than
/// "what does this call": a function's callees are implementation detail, while
/// its callers are the context the user is usually asking about.
const REVERSE_WEIGHT: f64 = 0.5;

/// Convergence threshold. Once total mass movement falls below this the ranking
/// order is settled and further iterations only shuffle noise.
const CONVERGENCE_EPSILON: f64 = 1e-6;

/// Adjacency built once per query and walked repeatedly.
pub struct RankGraph<'a> {
    /// Outgoing neighbours with their propagation weights.
    out: HashMap<&'a str, Vec<(&'a str, f64)>>,
}

impl<'a> RankGraph<'a> {
    /// Build the walkable adjacency from graph edges.
    ///
    /// Only dependency edges are walked — see [`EdgeKind::is_dependency`].
    /// Containment would connect every symbol in a file to every other and
    /// leave the file outranking all of them.
    pub fn build(edges: &'a [EdgeV1]) -> Self {
        let mut out: HashMap<&str, Vec<(&str, f64)>> = HashMap::new();

        for edge in edges.iter().filter(|e| e.kind.is_dependency()) {
            out.entry(edge.from.as_str())
                .or_default()
                .push((edge.to.as_str(), 1.0));
            out.entry(edge.to.as_str())
                .or_default()
                .push((edge.from.as_str(), REVERSE_WEIGHT));
        }

        Self { out }
    }

    /// Run the walk from a weighted seed set.
    ///
    /// `seeds` maps node id to initial mass; the values are normalized here, so
    /// callers can pass raw lexical scores without pre-scaling. An empty or
    /// all-zero seed set yields an empty result rather than a uniform ranking,
    /// because "no lexical anchor" should return nothing rather than the repo's
    /// most central symbols regardless of the question.
    pub fn walk(&self, seeds: &HashMap<&'a str, f64>) -> HashMap<&'a str, f64> {
        let total: f64 = seeds.values().filter(|v| **v > 0.0).sum();
        if total <= 0.0 {
            return HashMap::new();
        }

        let personalization: HashMap<&str, f64> = seeds
            .iter()
            .filter(|(_, weight)| **weight > 0.0)
            .map(|(id, weight)| (*id, weight / total))
            .collect();

        let mut scores = personalization.clone();

        for _ in 0..PAGERANK_ITERATIONS {
            let mut next: HashMap<&str, f64> = HashMap::new();

            // Restart mass: every iteration returns (1 - damping) of the total
            // to the seeds, which is what anchors the walk to the query.
            for (id, weight) in &personalization {
                *next.entry(id).or_insert(0.0) += (1.0 - PAGERANK_DAMPING) * weight;
            }

            for (id, score) in &scores {
                let Some(neighbours) = self.out.get(id) else {
                    continue;
                };

                let outflow: f64 = neighbours.iter().map(|(_, w)| w).sum();
                if outflow <= 0.0 {
                    continue;
                }

                for (neighbour, weight) in neighbours {
                    *next.entry(neighbour).or_insert(0.0) +=
                        PAGERANK_DAMPING * score * (weight / outflow);
                }
            }

            let drift = total_drift(&scores, &next);
            scores = next;

            if drift < CONVERGENCE_EPSILON {
                break;
            }
        }

        scores
    }
}

/// Sum of absolute per-node change between two iterations.
fn total_drift(previous: &HashMap<&str, f64>, next: &HashMap<&str, f64>) -> f64 {
    let mut drift = 0.0;

    for (id, score) in next {
        drift += (score - previous.get(id).copied().unwrap_or(0.0)).abs();
    }
    for (id, score) in previous {
        if !next.contains_key(id) {
            drift += score.abs();
        }
    }

    drift
}

/// Rescale scores so the highest is 1.0, leaving them comparable with the
/// normalized lexical scores they get blended against.
pub fn normalize(scores: &mut HashMap<&str, f64>) {
    let max = scores.values().copied().fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return;
    }

    for score in scores.values_mut() {
        *score /= max;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::EdgeKind;

    fn edge(from: &str, to: &str, kind: EdgeKind) -> EdgeV1 {
        EdgeV1::new(from, to, kind)
    }

    fn seeds<'a>(pairs: &[(&'a str, f64)]) -> HashMap<&'a str, f64> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn mass_spreads_from_the_seed_to_its_neighbours() {
        let edges = vec![edge("a", "b", EdgeKind::Calls)];
        let graph = RankGraph::build(&edges);

        let scores = graph.walk(&seeds(&[("a", 1.0)]));

        assert!(scores.get("b").copied().unwrap_or(0.0) > 0.0);
        assert!(scores["a"] > scores["b"]);
    }

    /// The whole point of *personalized* PageRank: two different questions must
    /// get two different answers from the same graph.
    #[test]
    fn different_seeds_produce_different_rankings() {
        let edges = vec![
            edge("a", "shared", EdgeKind::Calls),
            edge("b", "shared", EdgeKind::Calls),
            edge("a", "only_a", EdgeKind::Calls),
            edge("b", "only_b", EdgeKind::Calls),
        ];
        let graph = RankGraph::build(&edges);

        let from_a = graph.walk(&seeds(&[("a", 1.0)]));
        let from_b = graph.walk(&seeds(&[("b", 1.0)]));

        assert!(from_a.get("only_a") > from_a.get("only_b"));
        assert!(from_b.get("only_b") > from_b.get("only_a"));
    }

    /// Callers are what a developer usually wants when asking about a function,
    /// so reverse traversal has to work even though the edge points forward.
    #[test]
    fn callers_are_reachable_from_the_callee() {
        let edges = vec![edge("caller", "target", EdgeKind::Calls)];
        let graph = RankGraph::build(&edges);

        let scores = graph.walk(&seeds(&[("target", 1.0)]));

        assert!(scores.get("caller").copied().unwrap_or(0.0) > 0.0);
    }

    #[test]
    fn forward_edges_carry_more_mass_than_reverse_edges() {
        let edges = vec![
            edge("seed", "callee", EdgeKind::Calls),
            edge("caller", "seed", EdgeKind::Calls),
        ];
        let graph = RankGraph::build(&edges);

        let scores = graph.walk(&seeds(&[("seed", 1.0)]));

        assert!(scores["callee"] > scores["caller"]);
    }

    #[test]
    fn an_empty_seed_set_ranks_nothing() {
        let edges = vec![edge("a", "b", EdgeKind::Calls)];
        let graph = RankGraph::build(&edges);

        assert!(graph.walk(&HashMap::new()).is_empty());
        assert!(graph.walk(&seeds(&[("a", 0.0)])).is_empty());
    }

    /// A cycle is the classic non-termination trap for an iterative walk.
    #[test]
    fn cycles_terminate_and_stay_finite() {
        let edges = vec![
            edge("a", "b", EdgeKind::Calls),
            edge("b", "c", EdgeKind::Calls),
            edge("c", "a", EdgeKind::Calls),
        ];
        let graph = RankGraph::build(&edges);

        let scores = graph.walk(&seeds(&[("a", 1.0)]));

        assert!(scores.values().all(|s| s.is_finite()));
        assert!(scores["a"] > 0.0);
    }

    #[test]
    fn a_self_loop_does_not_diverge() {
        let edges = vec![edge("a", "a", EdgeKind::Calls)];
        let graph = RankGraph::build(&edges);

        let scores = graph.walk(&seeds(&[("a", 1.0)]));

        assert!(scores["a"].is_finite());
        assert!(scores["a"] <= 1.0);
    }

    #[test]
    fn a_seed_with_no_edges_keeps_its_own_mass() {
        let graph = RankGraph::build(&[]);

        let scores = graph.walk(&seeds(&[("lonely", 1.0)]));

        assert!(scores["lonely"] > 0.0);
    }

    /// Containment is not a dependency, so a file's other symbols must not
    /// pick up rank merely by living next to the seed.
    #[test]
    fn containment_carries_no_rank() {
        let edges = vec![
            edge("seed", "called", EdgeKind::Calls),
            edge("seed", "sibling", EdgeKind::Contains),
        ];
        let graph = RankGraph::build(&edges);

        let scores = graph.walk(&seeds(&[("seed", 1.0)]));

        assert!(scores["called"] > 0.0);
        assert_eq!(scores.get("sibling"), None);
    }

    #[test]
    fn the_walk_is_deterministic_across_runs() {
        let edges = vec![
            edge("a", "b", EdgeKind::Calls),
            edge("b", "c", EdgeKind::Calls),
            edge("c", "d", EdgeKind::Imports),
        ];
        let graph = RankGraph::build(&edges);
        let seed = seeds(&[("a", 1.0), ("c", 0.5)]);

        let first = graph.walk(&seed);
        for _ in 0..5 {
            assert_eq!(graph.walk(&seed), first);
        }
    }

    #[test]
    fn normalizing_puts_the_top_score_at_one() {
        let mut scores: HashMap<&str, f64> = [("a", 4.0), ("b", 2.0)].into_iter().collect();

        normalize(&mut scores);

        assert_eq!(scores["a"], 1.0);
        assert_eq!(scores["b"], 0.5);
    }

    #[test]
    fn normalizing_an_empty_map_is_a_no_op() {
        let mut scores: HashMap<&str, f64> = HashMap::new();
        normalize(&mut scores);
        assert!(scores.is_empty());
    }
}
