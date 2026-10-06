//! Identifier-aware tokenization shared by the index writer and the query path.
//!
//! Both sides must use this exact function. If the index split `getUserById`
//! into `get`/`user`/`by`/`id` but the query path only lowercased it, the term
//! would never match and the symbol would be unreachable — a silent recall
//! failure that no test of either side alone would catch.
//!
//! The splitting rules are deliberately mechanical rather than linguistic:
//!
//! - `camelCase` and `PascalCase` split at the lower-to-upper boundary.
//! - `SCREAMING_SNAKE` and `snake_case` split at underscores.
//! - `kebab-case`, dots, slashes, and every other non-alphanumeric split too.
//! - An acronym run keeps its shape: `HTTPServer` yields `http` and `server`,
//!   not `h`/`t`/`t`/`p`/`server`.
//!
//! Every token is also emitted in its original whole form, so searching for
//! `getUserById` scores higher on the symbol literally called that than on one
//! that merely shares the word `user`.

use std::collections::BTreeSet;

/// Tokens shorter than this are dropped. Single characters match nearly
/// everything and contribute noise rather than signal.
const MIN_TOKEN_LEN: usize = 2;

/// Split an identifier or free-text query into lowercase search tokens.
///
/// The result preserves first-occurrence order and contains no duplicates, so a
/// term repeated within one identifier does not inflate its own term frequency.
pub fn tokenize(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    for word in input.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }

        let lowered = word.to_lowercase();
        if lowered.len() >= MIN_TOKEN_LEN && seen.insert(lowered.clone()) {
            out.push(lowered);
        }

        for part in split_camel(word) {
            let lowered = part.to_lowercase();
            if lowered.len() >= MIN_TOKEN_LEN && seen.insert(lowered.clone()) {
                out.push(lowered);
            }
        }
    }

    out
}

/// Split a single word at case boundaries.
///
/// Returns an empty vector when the word has no internal boundary, so callers
/// do not get a duplicate of the whole-word token they already emitted.
fn split_camel(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0usize;

    for i in 1..chars.len() {
        let previous = chars[i - 1];
        let current = chars[i];

        // `userId` -> split before `I`. Also catches `v2Handler`.
        let lower_to_upper = !previous.is_uppercase() && current.is_uppercase();

        // `HTTPServer` -> split before `S`, because `Server` starts a new word
        // even though the previous character is also uppercase.
        let acronym_end = previous.is_uppercase()
            && current.is_uppercase()
            && chars.get(i + 1).is_some_and(|next| next.is_lowercase());

        if lower_to_upper || acronym_end {
            parts.push(chars[start..i].iter().collect::<String>());
            start = i;
        }
    }

    if parts.is_empty() {
        return Vec::new();
    }

    parts.push(chars[start..].iter().collect::<String>());
    parts
}

/// Tokenize a repo-relative path into searchable segments.
///
/// The extension is dropped: someone searching "cache" means the concept, and
/// `ts` or `py` appearing as a term would match every file in the repo equally,
/// which is the same as matching none of them.
pub fn tokenize_path(path: &str) -> Vec<String> {
    let without_extension = match path.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => path,
    };
    tokenize(without_extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_splits_into_parts_and_keeps_the_whole() {
        let tokens = tokenize("getUserById");
        assert!(tokens.contains(&"getuserbyid".to_string()));
        assert!(tokens.contains(&"get".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"by".to_string()));
        assert!(tokens.contains(&"id".to_string()));
    }

    #[test]
    fn snake_case_splits_on_underscores() {
        let tokens = tokenize("build_cache_entry");
        assert!(tokens.contains(&"build".to_string()));
        assert!(tokens.contains(&"cache".to_string()));
        assert!(tokens.contains(&"entry".to_string()));
    }

    /// The acronym rule is the subtle one: a naive lower-to-upper split would
    /// shred `HTTPServer` into single letters and make it unsearchable.
    #[test]
    fn acronyms_stay_whole() {
        let tokens = tokenize("HTTPServer");
        assert!(tokens.contains(&"http".to_string()));
        assert!(tokens.contains(&"server".to_string()));
        assert!(!tokens.contains(&"h".to_string()));
    }

    #[test]
    fn screaming_snake_lowercases_each_word() {
        let tokens = tokenize("MAX_RETRY_COUNT");
        assert!(tokens.contains(&"max".to_string()));
        assert!(tokens.contains(&"retry".to_string()));
        assert!(tokens.contains(&"count".to_string()));
    }

    #[test]
    fn single_characters_are_dropped() {
        assert!(tokenize("a b c").is_empty());
        assert!(!tokenize("parseX").contains(&"x".to_string()));
    }

    #[test]
    fn duplicates_within_one_identifier_appear_once() {
        let tokens = tokenize("cache_cache");
        assert_eq!(tokens.iter().filter(|t| *t == "cache").count(), 1);
    }

    #[test]
    fn path_tokens_drop_the_extension() {
        let tokens = tokenize_path("src/graph/cache.ts");
        assert!(tokens.contains(&"cache".to_string()));
        assert!(tokens.contains(&"graph".to_string()));
        assert!(!tokens.contains(&"ts".to_string()));
    }

    /// The index and the query path must agree, so the same string has to
    /// produce the same tokens no matter which side calls it.
    #[test]
    fn tokenization_is_symmetric_between_index_and_query() {
        assert_eq!(tokenize("parseConfigFile"), tokenize("parseConfigFile"));
        assert_eq!(tokenize("parse config file"), tokenize("parse_config_file"));
    }

    #[test]
    fn unicode_identifiers_do_not_panic() {
        let tokens = tokenize("café_niño");
        assert!(tokens.contains(&"café".to_string()));
        assert!(tokens.contains(&"niño".to_string()));
    }
}
