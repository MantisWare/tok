//! Hash-keyed cache for enrichment results.
//!
//! Every entry costs a network round trip and real money, so the cache is what
//! makes `--deep` usable more than once. Keys cover the content, the model, and
//! the prompt version, because a result is only reusable when all three are
//! unchanged — a new model or a reworded prompt produces different output for
//! the same file, and silently serving the old answer would make the feature
//! feel broken.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::graph::store::GraphPaths;

/// Bumped whenever a prompt changes, which invalidates everything it produced.
pub const PROMPT_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cache {
    #[serde(default)]
    pub entries: BTreeMap<String, String>,
}

impl Cache {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    pub fn insert(&mut self, key: String, value: String) {
        self.entries.insert(key, value);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn load(paths: &GraphPaths) -> Cache {
        // A damaged cache is a recoverable problem: the worst case is paying
        // for the calls again, which beats refusing to run.
        crate::graph::store::read_json(&path(paths)).unwrap_or_default()
    }

    pub fn write(&self, paths: &GraphPaths) -> anyhow::Result<()> {
        crate::graph::store::write_json(&path(paths), self)
    }
}

fn path(paths: &GraphPaths) -> std::path::PathBuf {
    paths.cache_dir().join("llm-cache.json")
}

/// Key for one enrichment result.
///
/// `kind` separates a file summary from a symbol crux, which are different
/// answers derived from overlapping text and would otherwise collide.
pub fn key(kind: &str, model: &str, content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(model.as_bytes());
    hasher.update([0]);
    hasher.update(PROMPT_VERSION.to_le_bytes());
    hasher.update([0]);
    hasher.update(content.as_bytes());

    format!("{:x}", hasher.finalize())[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_inputs_produce_the_same_key() {
        assert_eq!(key("file", "m", "content"), key("file", "m", "content"));
    }

    #[test]
    fn changed_content_produces_a_different_key() {
        assert_ne!(key("file", "m", "one"), key("file", "m", "two"));
    }

    /// A different model gives a different answer for the same file, so
    /// serving the cached one would look like the setting had no effect.
    #[test]
    fn a_different_model_produces_a_different_key() {
        assert_ne!(key("file", "a", "content"), key("file", "b", "content"));
    }

    /// A file summary and a symbol crux are different answers about
    /// overlapping text.
    #[test]
    fn a_different_kind_produces_a_different_key() {
        assert_ne!(key("file", "m", "content"), key("crux", "m", "content"));
    }

    /// Concatenating the fields without a separator would make ("ab", "c") and
    /// ("a", "bc") hash alike.
    #[test]
    fn field_boundaries_are_unambiguous() {
        assert_ne!(key("ab", "c", "x"), key("a", "bc", "x"));
    }

    #[test]
    fn keys_are_a_fixed_length() {
        assert_eq!(key("file", "m", "").len(), 32);
        assert_eq!(key("file", "m", &"x".repeat(100_000)).len(), 32);
    }

    #[test]
    fn entries_round_trip_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = GraphPaths::new(dir.path());
        paths.ensure().expect("ensure");
        let mut cache = Cache::default();
        cache.insert("k".to_string(), "a summary".to_string());

        cache.write(&paths).expect("write");
        let loaded = Cache::load(&paths);

        assert_eq!(loaded.get("k"), Some("a summary"));
    }

    #[test]
    fn a_missing_cache_reads_as_empty() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert!(Cache::load(&GraphPaths::new(dir.path())).is_empty());
    }

    /// Paying for the calls again beats refusing to run.
    #[test]
    fn a_damaged_cache_reads_as_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = GraphPaths::new(dir.path());
        paths.ensure().expect("ensure");
        std::fs::write(path(&paths), "{ not json").expect("write");

        assert!(Cache::load(&paths).is_empty());
    }
}
