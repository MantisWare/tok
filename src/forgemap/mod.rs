//! ForgeMap — code-indexing and in-file annotation engine.
//!
//! Scans source files to extract exports and import dependencies, builds a
//! reverse dependency graph (`used_by`), and injects machine-readable comment
//! headers (Level 1) at the top of source files. Generates a `.forgemap`
//! project manifest (Level 0) and optional Obsidian wiki documentation.
//!
//! Protocol format is CodeDNA-compatible; see `docs/FORGEMAP.md` for the full spec.

pub mod collect;
pub mod commands;
pub mod constants;
pub mod fmt;
pub mod graph;
pub mod header;
pub mod hook;
pub mod inject;
pub mod manifest_io;
pub mod scan;
pub mod types;
pub mod wiki;
