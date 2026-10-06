//! Ranked retrieval over the code graph.
//!
//! Answers "which symbols should an agent read to work on X" by combining a
//! lexical pass (IDF + BM25 over names, paths, and signatures) with a
//! structural pass (personalized PageRank seeded from the lexical hits), fused
//! with reciprocal rank fusion.
//!
//! The point is token economy: returning twenty relevant symbol spans costs far
//! fewer tokens than the handful of whole files an agent would otherwise read
//! to find them.
//!
//! This module reads the graph produced by [`crate::graph`]; it never parses
//! source itself.

pub mod ask;
pub mod constants;
pub mod federate;
pub mod fuse;
pub mod graphrank;
pub mod grep;
pub mod index_file;
pub mod map;
pub mod scoped;
pub mod skeleton;
pub mod tokenize;
pub mod traverse;
