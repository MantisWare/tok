//! Node id minting.
//!
//! Ids are readable rather than hashed — `src/cache.ts::Cache` — because they
//! appear in agent-facing output, in the markdown layer, and in MCP responses,
//! where an opaque hash would force a second lookup just to know what was
//! referenced.
//!
//! Readability costs uniqueness: one file can declare the same name twice (an
//! overload, a trait method and its impl, a re-export). Collisions are resolved
//! with a `~N` ordinal in declaration order, which is stable as long as the file
//! is walked in source order.

use std::collections::HashMap;

/// Mints unique, readable ids within a single file.
///
/// Scoped per file so ordinals never depend on how many other files were
/// processed first, which keeps extraction parallelizable and deterministic.
#[derive(Debug, Default)]
pub struct IdMinter {
    counts: HashMap<String, u32>,
}

impl IdMinter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint an id for `name` declared in `file`.
    ///
    /// The first occurrence is bare; later ones get `~2`, `~3`, and so on. This
    /// is what fixes the collision the regex indexer has today, where a trait
    /// method and its implementation in one file overwrite each other.
    pub fn mint(&mut self, file: &str, name: &str) -> String {
        let base = format!("{file}::{name}");
        let count = self.counts.entry(base.clone()).or_insert(0);
        *count += 1;

        if *count == 1 {
            base
        } else {
            format!("{base}~{count}")
        }
    }

    /// The id of the node representing the file itself.
    pub fn file_id(file: &str) -> String {
        file.to_string()
    }
}

/// Split an id back into its file and symbol halves.
///
/// Returns `None` for a file node, which has no `::` separator.
pub fn split_id(id: &str) -> Option<(&str, &str)> {
    // rsplit_once, not split_once: a Windows-style path cannot contain "::",
    // but a C++-ish symbol name could, and the file half is unambiguous.
    id.rsplit_once("::")
}

/// The symbol name an id refers to, with any `~N` ordinal removed.
pub fn name_from_id(id: &str) -> &str {
    let tail = split_id(id).map(|(_, name)| name).unwrap_or(id);
    match tail.rsplit_once('~') {
        // Only strip when the suffix is genuinely an ordinal; a name may
        // legitimately contain '~'.
        Some((name, ordinal))
            if !ordinal.is_empty() && ordinal.bytes().all(|b| b.is_ascii_digit()) =>
        {
            name
        }
        _ => tail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_occurrence_has_no_ordinal() {
        let mut m = IdMinter::new();
        assert_eq!(m.mint("a.ts", "Cache"), "a.ts::Cache");
    }

    #[test]
    fn repeats_get_sequential_ordinals() {
        let mut m = IdMinter::new();
        assert_eq!(m.mint("a.ts", "get"), "a.ts::get");
        assert_eq!(m.mint("a.ts", "get"), "a.ts::get~2");
        assert_eq!(m.mint("a.ts", "get"), "a.ts::get~3");
    }

    /// The exact case the regex indexer gets wrong today: `trait Store::get`
    /// and `impl Store for MemoryStore::get` collapse into one row.
    #[test]
    fn trait_and_impl_methods_stay_distinct() {
        let mut m = IdMinter::new();
        let decl = m.mint("lib.rs", "get");
        let imp = m.mint("lib.rs", "get");
        assert_ne!(decl, imp);
    }

    #[test]
    fn counters_are_independent_per_name_and_file() {
        let mut m = IdMinter::new();
        assert_eq!(m.mint("a.ts", "f"), "a.ts::f");
        assert_eq!(m.mint("b.ts", "f"), "b.ts::f");
        assert_eq!(m.mint("a.ts", "g"), "a.ts::g");
        assert_eq!(m.mint("a.ts", "f"), "a.ts::f~2");
    }

    #[test]
    fn splits_ids_back_apart() {
        assert_eq!(split_id("src/a.ts::Cache"), Some(("src/a.ts", "Cache")));
        assert_eq!(split_id("src/a.ts"), None);
    }

    #[test]
    fn strips_ordinals_from_names() {
        assert_eq!(name_from_id("a.rs::get"), "get");
        assert_eq!(name_from_id("a.rs::get~2"), "get");
        assert_eq!(name_from_id("a.rs"), "a.rs");
    }

    #[test]
    fn keeps_tildes_that_are_not_ordinals() {
        // A name may legitimately contain '~'; only a trailing all-digit
        // suffix is an ordinal.
        assert_eq!(name_from_id("a.rs::weird~name"), "weird~name");
        assert_eq!(name_from_id("a.rs::trailing~"), "trailing~");
    }
}
