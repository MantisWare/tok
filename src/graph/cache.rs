//! Memoized per-file extraction.
//!
//! Extraction is the expensive half of indexing — tree-sitter parses the whole
//! file — and it is also pure: a file's [`FileExtraction`] depends on nothing
//! but its own bytes and the extractor version. That makes it exactly the kind
//! of work worth caching.
//!
//! The key is `content hash + extractor stamp`, never the path or mtime:
//!
//! - **Content hash, not mtime**, so a checkout that rewrites timestamps costs
//!   nothing, and a moved file reuses its old entry.
//! - **Extractor stamp**, so changing extraction logic cannot serve results
//!   from the previous implementation. This is the failure mode that makes
//!   caches untrustworthy, and it is silent when it happens.
//!
//! Note that the *path* is still stored in the cached value, so a cache hit
//! for a moved file has its path corrected on the way out.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graph::extract::FileExtraction;

/// On-disk extract cache.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractCache {
    /// The extractor that produced every entry. A mismatch empties the cache.
    #[serde(default)]
    pub stamp: String,
    /// Extraction keyed by content hash.
    #[serde(default)]
    pub entries: BTreeMap<String, FileExtraction>,
}

impl ExtractCache {
    pub fn new(stamp: impl Into<String>) -> Self {
        Self {
            stamp: stamp.into(),
            entries: BTreeMap::new(),
        }
    }

    /// Load a cache, discarding it if it came from a different extractor.
    ///
    /// Returning an empty cache rather than an error keeps a stale or corrupt
    /// file from failing the index; the cost is one slow run.
    pub fn load_or_new(loaded: Option<ExtractCache>, stamp: &str) -> Self {
        match loaded {
            Some(cache) if cache.stamp == stamp => cache,
            _ => Self::new(stamp),
        }
    }

    /// Cached extraction for this content, with `path` applied.
    ///
    /// Two files with identical contents share one entry, so the stored path
    /// is whichever was extracted first and must be corrected per lookup.
    pub fn get(&self, hash: &str, path: &str) -> Option<FileExtraction> {
        let cached = self.entries.get(hash)?;
        if cached.path == path {
            return Some(cached.clone());
        }
        Some(retarget(cached, path))
    }

    pub fn insert(&mut self, hash: String, extraction: FileExtraction) {
        self.entries.insert(hash, extraction);
    }

    /// Drop entries for content that no longer appears in the repository.
    ///
    /// Without this the cache grows without bound, accumulating an entry for
    /// every version of every file ever indexed.
    pub fn retain_hashes(&mut self, live: &std::collections::HashSet<String>) {
        self.entries.retain(|hash, _| live.contains(hash));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Rewrite a cached extraction so its ids and paths refer to `path`.
///
/// Node ids embed the file path (`src/cache.ts::Cache`), so a cache hit from a
/// duplicate file would otherwise attribute symbols to the wrong file entirely.
fn retarget(cached: &FileExtraction, path: &str) -> FileExtraction {
    let old = cached.path.as_str();
    let swap = |id: &str| -> String {
        match id.strip_prefix(old) {
            Some(rest) => format!("{path}{rest}"),
            None => id.to_string(),
        }
    };

    let mut out = cached.clone();
    out.path = path.to_string();

    for node in &mut out.nodes {
        node.id = swap(&node.id);
        node.file = path.to_string();
        node.parent = node.parent.as_deref().map(swap);
    }
    for r in &mut out.refs {
        r.from = swap(&r.from);
    }
    for (method_id, _) in &mut out.method_owners {
        *method_id = swap(method_id);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{NodeKind, NodeV1, Span};

    fn extraction(path: &str) -> FileExtraction {
        let mut out = FileExtraction {
            path: path.to_string(),
            ..Default::default()
        };
        let mut node = NodeV1::new(
            format!("{path}::Cache"),
            NodeKind::Class,
            "Cache".to_string(),
            path.to_string(),
            Span::new(1, 5),
        );
        node.parent = None;

        let mut method = NodeV1::new(
            format!("{path}::get"),
            NodeKind::Method,
            "get".to_string(),
            path.to_string(),
            Span::new(2, 3),
        );
        method.parent = Some(format!("{path}::Cache"));

        out.nodes = vec![node, method];
        out
    }

    #[test]
    fn a_matching_stamp_keeps_the_cache() {
        let mut cache = ExtractCache::new("stamp-a");
        cache.insert("h".into(), extraction("a.ts"));

        let reloaded = ExtractCache::load_or_new(Some(cache), "stamp-a");
        assert_eq!(reloaded.len(), 1);
    }

    /// The silent-staleness guard: changed extraction logic must not serve
    /// results produced by the previous implementation.
    #[test]
    fn a_changed_extractor_empties_the_cache() {
        let mut cache = ExtractCache::new("stamp-a");
        cache.insert("h".into(), extraction("a.ts"));

        let reloaded = ExtractCache::load_or_new(Some(cache), "stamp-b");
        assert_eq!(reloaded.len(), 0);
        assert_eq!(reloaded.stamp, "stamp-b");
    }

    #[test]
    fn a_missing_cache_starts_empty() {
        let cache = ExtractCache::load_or_new(None, "s");
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn identical_content_hits_the_cache() {
        let mut cache = ExtractCache::new("s");
        cache.insert("h".into(), extraction("a.ts"));

        let hit = cache.get("h", "a.ts").expect("hit");
        assert_eq!(hit.nodes.len(), 2);
    }

    #[test]
    fn different_content_misses() {
        let mut cache = ExtractCache::new("s");
        cache.insert("h".into(), extraction("a.ts"));
        assert!(cache.get("other", "a.ts").is_none());
    }

    /// Two files with identical bytes share one entry, so ids must be rewritten
    /// or symbols get attributed to whichever file was indexed first.
    #[test]
    fn a_duplicate_file_gets_its_own_ids() {
        let mut cache = ExtractCache::new("s");
        cache.insert("h".into(), extraction("a.ts"));

        let hit = cache.get("h", "b.ts").expect("hit");

        assert_eq!(hit.path, "b.ts");
        for node in &hit.nodes {
            assert_eq!(node.file, "b.ts");
            assert!(node.id.starts_with("b.ts::"), "got {}", node.id);
        }

        let method = hit
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Method)
            .expect("method");
        assert_eq!(method.parent.as_deref(), Some("b.ts::Cache"));
    }

    #[test]
    fn pruning_drops_content_no_longer_present() {
        let mut cache = ExtractCache::new("s");
        cache.insert("live".into(), extraction("a.ts"));
        cache.insert("dead".into(), extraction("b.ts"));

        let live: std::collections::HashSet<String> = ["live".to_string()].into_iter().collect();
        cache.retain_hashes(&live);

        assert_eq!(cache.len(), 1);
        assert!(cache.get("live", "a.ts").is_some());
        assert!(cache.get("dead", "b.ts").is_none());
    }

    #[test]
    fn cache_survives_a_json_round_trip() {
        let mut cache = ExtractCache::new("s");
        cache.insert("h".into(), extraction("a.ts"));

        let json = serde_json::to_string(&cache).expect("serialize");
        let back: ExtractCache = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.stamp, "s");
        assert_eq!(back.get("h", "a.ts"), cache.get("h", "a.ts"));
    }
}
