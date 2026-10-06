//! Ranking constants, ported verbatim from graft.
//!
//! These are tuned values, not derivations. They are collected here rather than
//! inlined at their use sites so that a retrieval-quality change is a visible,
//! reviewable diff in one file instead of a magic number buried in a scoring
//! loop. Changing any of them changes result ordering, so treat edits as a
//! behaviour change and re-run the retrieval snapshots.

/// Weight applied to a query term matching a symbol's name.
///
/// Names are the strongest signal a developer gives: someone searching
/// "buildCache" almost always wants the symbol literally called `buildCache`.
pub const NAME_MATCH_WEIGHT: f64 = 3.0;

/// Weight applied to a query term matching a path segment. Lower than name
/// because directory names repeat across many unrelated symbols.
pub const PATH_MATCH_WEIGHT: f64 = 2.0;

/// BM25 term-frequency saturation. Standard Robertson/Walker value; above this
/// point repeating a term stops meaningfully increasing the score.
pub const BM25_K1: f64 = 1.2;

/// BM25 length normalization. 0.75 partially discounts long documents without
/// erasing the advantage of genuinely more relevant large files.
pub const BM25_B: f64 = 0.75;

/// PageRank damping factor for the personalized walk over the code graph.
///
/// Deliberately far below the classic 0.85: a code graph is small and densely
/// connected, so a high damping factor washes out the seed set and returns the
/// same globally central symbols for every query. 0.25 keeps the walk close to
/// the seeds.
pub const PAGERANK_DAMPING: f64 = 0.25;

/// Power-iteration cap for PageRank. The walk converges well before this on
/// repo-sized graphs; the bound exists so a pathological graph cannot hang a
/// query.
pub const PAGERANK_ITERATIONS: usize = 25;

/// Mix between lexical and structural scores; 0.5 weights them equally.
pub const STRUCTURAL_BLEND: f64 = 0.5;

/// Score floor below which a structurally-connected symbol is still admitted.
///
/// Rescues neighbours that no query term matched lexically but that the graph
/// says are central to the answer — typically the caller of the function the
/// user asked about.
pub const RESCUE_THRESHOLD: f64 = 0.15;

/// Multiplier applied to symbols in test files.
///
/// Not zero: when a user asks how something is exercised, tests are the answer.
/// But for a general "how does X work" query, the implementation should outrank
/// its test.
pub const TEST_PENALTY: f64 = 0.35;

/// Reciprocal-rank-fusion smoothing constant. The standard value from the
/// original RRF paper; damps the influence of any single ranker's top hit.
pub const RRF_K: f64 = 60.0;

/// Minimum share of a query's IDF weight that a scope's best hit must match in
/// its *name* to join a multi-scope answer.
///
/// Low on purpose. One distinctive term matching a symbol name is strong
/// evidence that the scope holds a real answer, and the alternative — requiring
/// broad coverage — would drop the exact-identifier case this is most often
/// used for.
pub const STRONG_FLOOR: f64 = 0.1;

/// The alternative route past the participation gate: matched share across all
/// indexed fields, for prose questions that match doc comments and signatures
/// rather than any one name.
pub const HIGH_FLOOR: f64 = 0.5;

/// How far behind the leading scope another scope may fall and still
/// contribute. A safety net behind the strength gate, not the primary filter.
pub const PARTICIPATION_RATIO: f64 = 0.25;

#[cfg(test)]
mod tests {
    use super::*;

    /// These values are a contract with the retrieval snapshots. A change here
    /// reorders results, so it should be deliberate and reviewed, never an
    /// incidental edit.
    #[test]
    fn constants_match_graft() {
        assert_eq!(NAME_MATCH_WEIGHT, 3.0);
        assert_eq!(PATH_MATCH_WEIGHT, 2.0);
        assert_eq!(BM25_K1, 1.2);
        assert_eq!(BM25_B, 0.75);
        assert_eq!(PAGERANK_DAMPING, 0.25);
        assert_eq!(PAGERANK_ITERATIONS, 25);
        assert_eq!(STRUCTURAL_BLEND, 0.5);
        assert_eq!(RESCUE_THRESHOLD, 0.15);
        assert_eq!(TEST_PENALTY, 0.35);
        assert_eq!(RRF_K, 60.0);
        assert_eq!(STRONG_FLOOR, 0.1);
        assert_eq!(HIGH_FLOOR, 0.5);
        assert_eq!(PARTICIPATION_RATIO, 0.25);
    }

    // Range invariants are checked at compile time; a runtime test of two
    // constants is folded away before it can ever fail.
    const _: () = assert!(NAME_MATCH_WEIGHT > PATH_MATCH_WEIGHT, "name outranks path");
    const _: () = assert!(BM25_B >= 0.0 && BM25_B <= 1.0);
    const _: () = assert!(PAGERANK_DAMPING > 0.0 && PAGERANK_DAMPING < 1.0);
    const _: () = assert!(STRUCTURAL_BLEND >= 0.0 && STRUCTURAL_BLEND <= 1.0);
    const _: () = assert!(
        TEST_PENALTY > 0.0 && TEST_PENALTY < 1.0,
        "penalty, not a boost"
    );
    const _: () = assert!(PAGERANK_ITERATIONS > 0);
    const _: () = assert!(
        STRONG_FLOOR < HIGH_FLOOR,
        "a name match qualifies on less evidence than broad coverage"
    );
    const _: () = assert!(PARTICIPATION_RATIO > 0.0 && PARTICIPATION_RATIO < 1.0);
}
