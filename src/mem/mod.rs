//! Persistent structural memory for coding agents.
//!
//! Provides AST-derived symbol indexing, full-text search (FTS5/BM25),
//! relationship graph traversal, and blast-radius impact analysis.
//! All data is stored locally in SQLite — no external services required.

pub mod db;
pub mod evolution;
pub mod graph;
pub mod indexer;
pub mod parser_regex;
pub mod quality;
pub mod search;
pub mod symbols;
