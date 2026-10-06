//! Committed markdown describing the code graph.
//!
//! The graph in `.tok/graph/` is a derived cache: gitignored, machine-readable,
//! rebuilt on demand. This module produces the other half — a set of markdown
//! files under `.tok/map/` that are meant to be committed, reviewed in pull
//! requests, and annotated by hand.
//!
//! Why write markdown at all when the graph can be queried directly:
//!
//! - **It survives outside the tool.** A card is readable in a browser, on
//!   GitHub, and by any agent with filesystem access but no TOK binary.
//! - **It gives humans somewhere to write.** Each file has a Notes section that
//!   regeneration preserves, so institutional knowledge lands next to the code
//!   it describes instead of in a wiki nobody opens.
//! - **It makes staleness reviewable.** A change that alters the wiring shows
//!   up as a diff on the card, which is a prompt to think about coupling.
//!
//! The layout:
//!
//! ```text
//! .tok/map/
//!   INDEX.md            navigation, repo shape, hubs
//!   src-graph-cache-ts.md   one card per source file
//!   manifest.json       what was generated, from what, for drift checks
//! ```

pub mod blocks;
pub mod cards;
pub mod check;
pub mod frontmatter;
pub mod index;
pub mod manifest;
pub mod slug;
pub mod write;

/// Navigation entry point within the markdown directory.
pub const INDEX_FILE: &str = "INDEX.md";

/// Generation bookkeeping, read by `tok mem check`.
pub const MANIFEST_FILE: &str = "manifest.json";
