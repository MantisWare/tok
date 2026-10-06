//! Reciprocal rank fusion for combining rankers with incomparable scales.
//!
//! BM25 produces unbounded scores whose magnitude depends on corpus statistics.
//! PageRank produces a probability distribution summing to one. Averaging them
//! directly means whichever ranker happens to have the larger numbers wins, and
//! that ratio shifts with repo size — a blend tuned on a small repo silently
//! becomes lexical-only on a large one.
//!
//! RRF sidesteps the problem by discarding magnitudes and keeping only order:
//! each ranker contributes `1 / (k + rank)`. A symbol ranked first by either
//! ranker gets a strong contribution, and one ranked highly by *both* gets the
//! top result. The constant [`RRF_K`] damps the influence of any single
//! ranker's top hit, so one confident-but-wrong ranker cannot dominate.
//!
//! This is used for multi-scope fusion (Phase 8) and wherever two independently
//! scaled rankings must merge. The single-scope lexical/structural blend uses
//! the weighted form in [`crate::query::ask`] instead, because there both
//! scores are already normalized to 0..1 and the relative *margin* carries real
//! information worth preserving.

use std::collections::HashMap;

use crate::query::constants::RRF_K;

/// One ranker's output: ids in descending relevance order.
pub type Ranking<'a> = Vec<&'a str>;

/// Fuse several rankings into one, returning ids with their fused scores in
/// descending order.
///
/// Ties break on id so the output is stable across runs regardless of the
/// iteration order of the intermediate map.
pub fn fuse<'a>(rankings: &[Ranking<'a>]) -> Vec<(&'a str, f64)> {
    fuse_weighted(
        &rankings
            .iter()
            .map(|r| (r.clone(), 1.0))
            .collect::<Vec<_>>(),
    )
}

/// Fuse rankings where some rankers are trusted more than others.
///
/// A weight scales that ranker's whole contribution, so a weight of 2.0 makes
/// its ordering count twice as much without changing the shape of the decay.
pub fn fuse_weighted<'a>(rankings: &[(Ranking<'a>, f64)]) -> Vec<(&'a str, f64)> {
    let mut fused: HashMap<&str, f64> = HashMap::new();

    for (ranking, weight) in rankings {
        for (position, id) in ranking.iter().enumerate() {
            // Rank is 1-based: the top hit should score 1/(k+1), not 1/k.
            let rank = (position + 1) as f64;
            *fused.entry(id).or_insert(0.0) += weight / (RRF_K + rank);
        }
    }

    let mut out: Vec<(&str, f64)> = fused.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_symbol_ranked_well_by_both_wins() {
        let lexical = vec!["shared", "lex_only"];
        let structural = vec!["shared", "struct_only"];

        let fused = fuse(&[lexical, structural]);

        assert_eq!(fused[0].0, "shared");
    }

    #[test]
    fn results_come_back_in_descending_score_order() {
        let fused = fuse(&[vec!["a", "b", "c"], vec!["a", "b", "c"]]);

        assert_eq!(
            fused.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    /// The property that motivates RRF: a ranker with enormous raw scores must
    /// not dominate, because only positions are fused.
    #[test]
    fn score_magnitude_is_irrelevant_only_position_counts() {
        // Both rankers rank "b" first; their internal score scales differ
        // wildly, but the fused order cannot see that.
        let fused = fuse(&[vec!["b", "a"], vec!["b", "a"]]);

        assert_eq!(fused[0].0, "b");
        assert_eq!(fused[1].0, "a");
    }

    #[test]
    fn a_single_ranking_passes_through_in_order() {
        let fused = fuse(&[vec!["x", "y", "z"]]);

        assert_eq!(
            fused.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec!["x", "y", "z"]
        );
    }

    #[test]
    fn no_rankings_fuse_to_nothing() {
        assert!(fuse(&[]).is_empty());
        assert!(fuse(&[vec![]]).is_empty());
    }

    #[test]
    fn ties_break_on_id_so_output_is_stable() {
        // "a" and "b" are symmetric, so only the id tiebreak decides.
        let first = fuse(&[vec!["a"], vec!["b"]]);
        for _ in 0..5 {
            assert_eq!(fuse(&[vec!["a"], vec!["b"]]), first);
        }
        assert_eq!(first[0].0, "a");
    }

    #[test]
    fn weighting_lets_one_ranker_count_for_more() {
        let unweighted = fuse(&[vec!["a", "b"], vec!["b", "a"]]);
        // Perfectly symmetric, so the id tiebreak decides.
        assert_eq!(unweighted[0].0, "a");

        let weighted = fuse_weighted(&[(vec!["a", "b"], 1.0), (vec!["b", "a"], 5.0)]);
        assert_eq!(weighted[0].0, "b");
    }

    #[test]
    fn a_zero_weighted_ranker_contributes_nothing() {
        let fused = fuse_weighted(&[(vec!["a", "b"], 1.0), (vec!["b"], 0.0)]);

        assert_eq!(fused[0].0, "a");
    }

    #[test]
    fn the_top_hit_scores_the_documented_reciprocal() {
        let fused = fuse(&[vec!["a"]]);

        assert!((fused[0].1 - 1.0 / (RRF_K + 1.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn every_input_id_survives_fusion() {
        let fused = fuse(&[vec!["a", "b"], vec!["c"]]);

        assert_eq!(fused.len(), 3);
    }
}
