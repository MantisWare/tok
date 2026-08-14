//! `manifest.json`: what was written, from what, and when.
//!
//! The manifest is what makes `tok mem check` possible. Without it, "is the
//! markdown stale?" can only be answered by regenerating everything and
//! diffing, which is exactly the expensive operation the check is supposed to
//! avoid.
//!
//! Each entry records the hash of the *generated* content, not of the whole
//! file. That distinction is the point: a human editing the Notes section
//! changes the file but not the generated block, and reporting that as drift
//! would train people to ignore the check.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graph::types::GraphV1;
use crate::markdown::cards::Card;

pub const MANIFEST_VERSION: u32 = 1;

/// One generated file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Filename within the markdown directory.
    pub file: String,
    /// The source path this was generated from, absent for `INDEX.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Hash of the generated block only.
    pub hash: String,
    /// Hash of the source file at generation time, so content drift is
    /// detectable without reparsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// Extractor that produced the graph these files were generated from.
    pub extractor: String,
    pub repo_id: String,
    pub entries: Vec<Entry>,
}

impl Manifest {
    pub fn new(repo_id: impl Into<String>, extractor: impl Into<String>) -> Self {
        Self {
            version: MANIFEST_VERSION,
            extractor: extractor.into(),
            repo_id: repo_id.into(),
            entries: Vec::new(),
        }
    }

    pub fn entry(&self, file: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.file == file)
    }

    /// Source paths that have a generated card.
    pub fn covered_sources(&self) -> Vec<&str> {
        let mut sources: Vec<&str> = self
            .entries
            .iter()
            .filter_map(|e| e.source.as_deref())
            .collect();
        sources.sort_unstable();
        sources
    }

    /// Sort so the file is byte-stable for a given input.
    pub fn normalize(&mut self) {
        self.entries.sort_by(|a, b| a.file.cmp(&b.file));
        self.entries.dedup_by(|a, b| a.file == b.file);
    }
}

/// Build a manifest describing a generated card set.
pub fn build(graph: &GraphV1, cards: &[Card], index_body: &str) -> Manifest {
    let source_hashes: BTreeMap<&str, &str> = graph
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.hash.as_str()))
        .collect();

    let mut manifest = Manifest::new(graph.repo_id.clone(), graph.extractor.clone());

    manifest.entries.push(Entry {
        file: super::INDEX_FILE.to_string(),
        source: None,
        hash: hash(index_body),
        source_hash: None,
    });

    for card in cards {
        manifest.entries.push(Entry {
            file: card.filename.clone(),
            source: Some(card.path.clone()),
            hash: hash(&card.body),
            source_hash: source_hashes.get(card.path.as_str()).map(|h| h.to_string()),
        });
    }

    manifest.normalize();
    manifest
}

/// Hex SHA-256, truncated to 16 characters.
///
/// Truncation is safe here because the manifest compares a hash against the
/// hash of the same file's regenerated content; this is change detection, not
/// a security boundary.
pub fn hash(content: &str) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(content.as_bytes());
    format!("{digest:x}")[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::FileEntryV1;

    fn card(filename: &str, path: &str, body: &str) -> Card {
        Card {
            filename: filename.to_string(),
            path: path.to_string(),
            frontmatter: String::new(),
            body: body.to_string(),
        }
    }

    fn graph_with(files: Vec<FileEntryV1>) -> GraphV1 {
        let mut g = GraphV1::new("repo", "test-extractor");
        g.files = files;
        g.normalize();
        g
    }

    fn file(path: &str, hash: &str) -> FileEntryV1 {
        FileEntryV1 {
            path: path.to_string(),
            hash: hash.to_string(),
            size: 1,
            language: "typescript".to_string(),
            node_count: 1,
        }
    }

    #[test]
    fn the_index_is_always_recorded() {
        let manifest = build(&graph_with(Vec::new()), &[], "index body");

        assert!(manifest.entry(super::super::INDEX_FILE).is_some());
    }

    #[test]
    fn each_card_records_its_source() {
        let g = graph_with(vec![file("src/a.ts", "srchash")]);
        let cards = vec![card("src-a-ts.md", "src/a.ts", "body")];

        let manifest = build(&g, &cards, "index");
        let entry = manifest.entry("src-a-ts.md").expect("entry");

        assert_eq!(entry.source.as_deref(), Some("src/a.ts"));
        assert_eq!(entry.source_hash.as_deref(), Some("srchash"));
    }

    #[test]
    fn different_content_hashes_differently() {
        assert_ne!(hash("one"), hash("two"));
    }

    #[test]
    fn identical_content_hashes_identically() {
        assert_eq!(hash("same"), hash("same"));
    }

    #[test]
    fn entries_are_sorted_for_stable_output() {
        let g = graph_with(vec![file("src/b.ts", "h"), file("src/a.ts", "h")]);
        let cards = vec![
            card("src-b-ts.md", "src/b.ts", "b"),
            card("src-a-ts.md", "src/a.ts", "a"),
        ];

        let manifest = build(&g, &cards, "index");
        let files: Vec<&str> = manifest.entries.iter().map(|e| e.file.as_str()).collect();

        let mut sorted = files.clone();
        sorted.sort_unstable();
        assert_eq!(files, sorted);
    }

    #[test]
    fn covered_sources_lists_only_cards() {
        let g = graph_with(vec![file("src/a.ts", "h")]);
        let cards = vec![card("src-a-ts.md", "src/a.ts", "body")];

        let manifest = build(&g, &cards, "index");

        assert_eq!(manifest.covered_sources(), vec!["src/a.ts"]);
    }

    #[test]
    fn a_manifest_round_trips_through_json() {
        let g = graph_with(vec![file("src/a.ts", "h")]);
        let manifest = build(&g, &[card("src-a-ts.md", "src/a.ts", "body")], "index");

        let json = serde_json::to_string(&manifest).expect("serialize");
        let parsed: Manifest = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed, manifest);
    }

    #[test]
    fn building_the_same_input_twice_gives_identical_manifests() {
        let g = graph_with(vec![file("src/a.ts", "h")]);
        let cards = vec![card("src-a-ts.md", "src/a.ts", "body")];

        assert_eq!(build(&g, &cards, "index"), build(&g, &cards, "index"));
    }

    #[test]
    fn the_extractor_stamp_is_carried_through() {
        let manifest = build(&graph_with(Vec::new()), &[], "index");

        assert_eq!(manifest.extractor, "test-extractor");
    }
}
