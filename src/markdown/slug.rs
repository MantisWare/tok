//! Turning symbol names and file paths into stable, filesystem-safe filenames.
//!
//! Two constraints make this less trivial than it looks:
//!
//! 1. **Case-insensitive filesystems.** macOS and Windows treat `Cache.md` and
//!    `cache.md` as the same file, so a repo containing both `Cache` and
//!    `cache` would have one silently overwrite the other. Slugs are therefore
//!    lowercased, and collisions are resolved explicitly by the caller rather
//!    than left to the filesystem.
//! 2. **Stability across runs.** A slug is a filename that gets committed and
//!    linked to. If the same symbol produced a different slug on a different
//!    machine, every checkout would churn.
//!
//! Reserved Windows device names (`con`, `aux`, `nul`, `com1`...) are also
//! handled: creating `con.md` fails outright on Windows, which would make the
//! markdown layer unusable on that platform for a repo containing a symbol
//! named `Con`.

use std::collections::BTreeMap;

/// Names that cannot be used as files on Windows, regardless of extension.
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Cap on slug length. Long enough to stay readable, short enough to survive
/// deep directory nesting on filesystems with a 255-byte name limit.
const MAX_LEN: usize = 80;

/// Convert arbitrary text into a lowercase, hyphen-separated slug.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_was_separator = true;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if ch.is_alphanumeric() {
            // Keep non-ASCII letters: a repo written in another language should
            // still get readable filenames rather than a row of hyphens.
            out.extend(ch.to_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            out.push('-');
            last_was_separator = true;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }

    if out.chars().count() > MAX_LEN {
        out = out.chars().take(MAX_LEN).collect();
        while out.ends_with('-') {
            out.pop();
        }
    }

    if out.is_empty() {
        return "unnamed".to_string();
    }

    if RESERVED.contains(&out.as_str()) {
        return format!("{out}-");
    }

    out
}

/// Slug for a source file, preserving directory structure as hyphens.
///
/// `src/graph/cache.ts` becomes `src-graph-cache-ts`. Flattening rather than
/// mirroring the tree keeps every card in one directory, which makes them
/// greppable and keeps relative links between cards trivial.
pub fn slugify_path(path: &str) -> String {
    slugify(path)
}

/// Assign unique slugs to a set of names, in a way that does not depend on
/// input order.
///
/// Collisions get a numeric suffix, and which one keeps the bare slug is
/// decided by sorting rather than by iteration order — otherwise two machines
/// could disagree about which `Cache` owns `cache.md`.
pub fn unique_slugs<'a>(names: impl IntoIterator<Item = &'a str>) -> BTreeMap<&'a str, String> {
    let mut sorted: Vec<&str> = names.into_iter().collect();
    sorted.sort_unstable();
    sorted.dedup();

    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = BTreeMap::new();

    for name in sorted {
        let base = slugify(name);
        let count = used.entry(base.clone()).or_insert(0);
        *count += 1;

        let slug = if *count == 1 {
            base
        } else {
            format!("{base}-{count}")
        };

        out.insert(name, slug);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_becomes_lowercase() {
        assert_eq!(slugify("BuildCache"), "buildcache");
    }

    #[test]
    fn separators_collapse_to_single_hyphens() {
        assert_eq!(slugify("build   cache__entry"), "build-cache-entry");
    }

    #[test]
    fn leading_and_trailing_separators_are_dropped() {
        assert_eq!(slugify("__cache__"), "cache");
        assert_eq!(slugify("///a///"), "a");
    }

    #[test]
    fn a_path_flattens_into_one_name() {
        assert_eq!(slugify_path("src/graph/cache.ts"), "src-graph-cache-ts");
    }

    /// Without lowercasing, these two collide on macOS and Windows and one
    /// silently overwrites the other.
    #[test]
    fn case_variants_produce_the_same_base_slug() {
        assert_eq!(slugify("Cache"), slugify("cache"));
    }

    #[test]
    fn collisions_get_distinct_slugs() {
        let slugs = unique_slugs(["Cache", "cache"]);

        let values: Vec<&String> = slugs.values().collect();
        assert_ne!(values[0], values[1]);
    }

    /// Two machines walking the repo in different order must agree.
    #[test]
    fn collision_resolution_does_not_depend_on_input_order() {
        let forward = unique_slugs(["Cache", "cache", "CACHE"]);
        let backward = unique_slugs(["CACHE", "cache", "Cache"]);

        assert_eq!(forward, backward);
    }

    #[test]
    fn distinct_names_keep_their_bare_slugs() {
        let slugs = unique_slugs(["alpha", "beta"]);

        assert_eq!(slugs["alpha"], "alpha");
        assert_eq!(slugs["beta"], "beta");
    }

    /// `con.md` cannot be created on Windows at all.
    #[test]
    fn reserved_device_names_are_escaped() {
        assert_ne!(slugify("con"), "con");
        assert_ne!(slugify("AUX"), "aux");
        assert!(slugify("con").starts_with("con"));
    }

    #[test]
    fn a_name_with_no_usable_characters_gets_a_placeholder() {
        assert_eq!(slugify("!!!"), "unnamed");
        assert_eq!(slugify(""), "unnamed");
    }

    #[test]
    fn long_names_are_truncated_without_a_trailing_hyphen() {
        let slug = slugify(&"averylongname ".repeat(40));

        assert!(slug.chars().count() <= MAX_LEN);
        assert!(!slug.ends_with('-'));
    }

    #[test]
    fn non_ascii_letters_are_kept_and_lowercased() {
        assert_eq!(slugify("Café"), "café");
        assert_eq!(slugify("Ünïcödé"), "ünïcödé");
    }

    #[test]
    fn slugs_are_stable_across_calls() {
        let first = slugify("SomeSymbol::method");
        for _ in 0..5 {
            assert_eq!(slugify("SomeSymbol::method"), first);
        }
    }
}
