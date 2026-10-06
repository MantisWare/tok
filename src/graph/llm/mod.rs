//! The optional `--deep` layer: LLM-written file summaries and per-symbol
//! crux lines.
//!
//! The graph knows what the code *is* — every declaration, every call, every
//! import. What it cannot recover from syntax is what a file is *for*, and that
//! is the sentence an agent actually needs before deciding whether to open it.
//! A summary that saves one unnecessary file read has already paid for itself
//! several times over in tokens.
//!
//! It is off by default and stays off until configured, because unlike the rest
//! of the graph this sends source code to a third party and charges for it.
//!
//! Results are cached by content hash, so a second run costs nothing for files
//! that have not changed.

pub mod cache;
pub mod provider;

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::graph::config::LlmConfig;
use crate::graph::store::GraphPaths;
use crate::graph::types::{GraphV1, NodeKind};

const FILE_SYSTEM_PROMPT: &str =
    "You summarise source files for an AI coding agent deciding what to read. \
     Reply with one or two sentences describing what the file is responsible for. \
     Do not list its functions, do not restate the filename, and do not use \
     preamble like \"This file\".";

const CRUX_SYSTEM_PROMPT: &str =
    "You explain what a single function or type is for, to an AI coding agent. \
     Reply with one sentence covering why it exists, not what its syntax says. \
     No preamble, no restating the name.";

/// What enrichment produced, keyed by file path and symbol id.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Enrichment {
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub symbols: BTreeMap<String, String>,
}

impl Enrichment {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.symbols.is_empty()
    }

    pub fn load(paths: &GraphPaths) -> Enrichment {
        crate::graph::store::read_json(&Self::path(paths)).unwrap_or_default()
    }

    pub fn write(&self, paths: &GraphPaths) -> Result<()> {
        crate::graph::store::write_json(&Self::path(paths), self)
    }

    fn path(paths: &GraphPaths) -> std::path::PathBuf {
        paths.cache_dir().join("enrichment.json")
    }
}

/// What a run did, for reporting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub files_summarised: usize,
    pub symbols_explained: usize,
    /// Served from cache, so free.
    pub cached: usize,
    /// Attempted and failed. Reported rather than fatal.
    pub failed: usize,
}

/// Run enrichment over a graph, writing results and cache as it goes.
pub fn enrich(
    repo_root: &Path,
    graph: &GraphV1,
    config: &LlmConfig,
    verbose: u8,
) -> Result<(Enrichment, Stats)> {
    if !config.enabled {
        bail!(
            "Deep enrichment is off. Enable it in config.toml:\n\n  [graph.llm]\n  enabled = true\n\nand set {}.",
            config.key_env()
        );
    }

    let client = provider::Client::new(config)?;
    let paths = GraphPaths::new(repo_root);
    paths.ensure()?;

    let mut cache = cache::Cache::load(&paths);
    let mut enrichment = Enrichment::load(&paths);
    let mut stats = Stats::default();

    for file in files_to_summarise(graph, config.max_files) {
        let Some(source) = read_capped(repo_root, &file, config.max_chars) else {
            continue;
        };

        let key = cache::key("file", &config.model, &source);
        if let Some(cached) = cache.get(&key) {
            enrichment.files.insert(file, cached.to_string());
            stats.cached += 1;
            continue;
        }

        let prompt = format!("File: {file}\n\n{source}");
        match client.complete(FILE_SYSTEM_PROMPT, &prompt) {
            Ok(summary) => {
                cache.insert(key, summary.clone());
                enrichment.files.insert(file, summary);
                stats.files_summarised += 1;
            }
            // One unreachable file is not a reason to discard the work already
            // paid for, so failures are counted and the run continues.
            Err(error) => {
                stats.failed += 1;
                if verbose > 0 {
                    eprintln!("{file}: {error}");
                }
            }
        }
    }

    for node in symbols_to_explain(graph, config.max_symbols) {
        let Some(source) = symbol_source(repo_root, graph, &node.id, config.max_chars) else {
            continue;
        };

        let key = cache::key("crux", &config.model, &source);
        if let Some(cached) = cache.get(&key) {
            enrichment
                .symbols
                .insert(node.id.clone(), cached.to_string());
            stats.cached += 1;
            continue;
        }

        let prompt = format!(
            "{} `{}` in {}\n\n{source}",
            node.kind.as_str(),
            node.name,
            node.file
        );

        match client.complete(CRUX_SYSTEM_PROMPT, &prompt) {
            Ok(crux) => {
                cache.insert(key, crux.clone());
                enrichment.symbols.insert(node.id.clone(), crux);
                stats.symbols_explained += 1;
            }
            Err(error) => {
                stats.failed += 1;
                if verbose > 0 {
                    eprintln!("{}: {error}", node.name);
                }
            }
        }
    }

    // Written even on a partial run, so an interrupted or rate-limited pass
    // does not have to be paid for twice.
    cache.write(&paths)?;
    enrichment.write(&paths)?;

    Ok((enrichment, stats))
}

/// The files worth spending a call on: the ones with the most declarations,
/// which are the ones an agent is most likely to have to choose about.
fn files_to_summarise(graph: &GraphV1, limit: usize) -> Vec<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for node in &graph.nodes {
        if node.kind != NodeKind::File {
            *counts.entry(node.file.as_str()).or_default() += 1;
        }
    }

    let mut ranked: Vec<(&str, usize)> = counts.into_iter().collect();
    // Count first, then path, so a tie does not reorder between runs.
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    ranked
        .into_iter()
        .take(limit)
        .map(|(file, _)| file.to_string())
        .collect()
}

/// The symbols worth explaining: the most depended-upon, since an agent is most
/// likely to meet those first and least able to skip them.
fn symbols_to_explain(graph: &GraphV1, limit: usize) -> Vec<&crate::graph::types::NodeV1> {
    let mut dependents: BTreeMap<&str, usize> = BTreeMap::new();
    for edge in &graph.edges {
        *dependents.entry(edge.to.as_str()).or_default() += 1;
    }

    let mut ranked: Vec<(&crate::graph::types::NodeV1, usize)> = graph
        .nodes
        .iter()
        .filter(|node| node.kind != NodeKind::File)
        .map(|node| {
            let count = dependents.get(node.id.as_str()).copied().unwrap_or(0);
            (node, count)
        })
        .filter(|(_, count)| *count > 0)
        .collect();

    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));

    ranked
        .into_iter()
        .take(limit)
        .map(|(node, _)| node)
        .collect()
}

/// Read a file, truncated to the configured budget.
///
/// Whole files would exceed both the context window and the bill on anything
/// real, and the top of a file is where the imports and the main declaration
/// live — the part that says what it is for.
fn read_capped(repo_root: &Path, file: &str, max_chars: usize) -> Option<String> {
    let contents = std::fs::read_to_string(repo_root.join(file)).ok()?;
    if contents.trim().is_empty() {
        return None;
    }

    Some(truncate(&contents, max_chars))
}

/// The source of one symbol, sliced from its span.
fn symbol_source(repo_root: &Path, graph: &GraphV1, id: &str, max_chars: usize) -> Option<String> {
    let node = graph.node(id)?;
    let contents = std::fs::read_to_string(repo_root.join(&node.file)).ok()?;

    let start = node.span.start as usize;
    let end = (node.span.end as usize).min(contents.len());
    if start >= end {
        return None;
    }

    // Spans are byte offsets and may land inside a multi-byte character, which
    // would panic on a slice.
    let start = floor_boundary(&contents, start);
    let end = floor_boundary(&contents, end);

    let slice = contents.get(start..end)?;
    if slice.trim().is_empty() {
        return None;
    }

    Some(truncate(slice, max_chars))
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }

    let cut = floor_boundary(text, max_chars);
    text[..cut].to_string()
}

/// The nearest character boundary at or before `index`.
fn floor_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EdgeKind, EdgeV1, NodeV1, Span};

    fn node(id: &str, name: &str, file: &str, kind: NodeKind) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            kind,
            name.to_string(),
            file.to_string(),
            Span::new(0, 10),
        )
    }

    fn graph(nodes: Vec<NodeV1>, edges: Vec<EdgeV1>) -> GraphV1 {
        let mut graph = GraphV1::new("repo", "test");
        graph.nodes = nodes;
        graph.edges = edges;
        graph
    }

    // ------------------------------------------------------------ selection

    /// Calls cost money, so the budget goes to the files an agent is most
    /// likely to have to choose about.
    #[test]
    fn the_densest_files_are_summarised_first() {
        let g = graph(
            vec![
                node("a1", "a1", "small.rs", NodeKind::Function),
                node("b1", "b1", "big.rs", NodeKind::Function),
                node("b2", "b2", "big.rs", NodeKind::Function),
                node("b3", "b3", "big.rs", NodeKind::Function),
            ],
            Vec::new(),
        );

        assert_eq!(files_to_summarise(&g, 1), vec!["big.rs".to_string()]);
    }

    #[test]
    fn file_selection_respects_the_limit() {
        let g = graph(
            vec![
                node("a", "a", "a.rs", NodeKind::Function),
                node("b", "b", "b.rs", NodeKind::Function),
                node("c", "c", "c.rs", NodeKind::Function),
            ],
            Vec::new(),
        );

        assert_eq!(files_to_summarise(&g, 2).len(), 2);
    }

    /// A run that reordered between passes would invalidate the cache for
    /// files it had already paid for.
    #[test]
    fn ties_are_broken_by_path_so_runs_are_repeatable() {
        let g = graph(
            vec![
                node("b", "b", "b.rs", NodeKind::Function),
                node("a", "a", "a.rs", NodeKind::Function),
            ],
            Vec::new(),
        );

        assert_eq!(files_to_summarise(&g, 2), files_to_summarise(&g, 2));
        assert_eq!(files_to_summarise(&g, 1), vec!["a.rs".to_string()]);
    }

    /// The file node exists to carry the residual; summarising it as a symbol
    /// would spend a call to describe a filename.
    #[test]
    fn file_nodes_are_not_counted_as_declarations() {
        let g = graph(vec![node("f", "a.rs", "a.rs", NodeKind::File)], Vec::new());

        assert!(files_to_summarise(&g, 10).is_empty());
    }

    #[test]
    fn the_most_depended_upon_symbols_are_explained_first() {
        let g = graph(
            vec![
                node("hub", "hub", "a.rs", NodeKind::Function),
                node("leaf", "leaf", "a.rs", NodeKind::Function),
                node("caller", "caller", "a.rs", NodeKind::Function),
            ],
            vec![
                EdgeV1::new("caller", "hub", EdgeKind::Calls),
                EdgeV1::new("leaf", "hub", EdgeKind::Calls),
                EdgeV1::new("caller", "leaf", EdgeKind::Calls),
            ],
        );

        let chosen = symbols_to_explain(&g, 1);

        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].name, "hub");
    }

    /// A symbol nothing depends on is one an agent will not meet by accident.
    #[test]
    fn symbols_with_no_dependents_are_skipped() {
        let g = graph(
            vec![node("lonely", "lonely", "a.rs", NodeKind::Function)],
            Vec::new(),
        );

        assert!(symbols_to_explain(&g, 10).is_empty());
    }

    // ---------------------------------------------------------- truncation

    #[test]
    fn short_text_is_left_alone() {
        assert_eq!(truncate("hello", 100), "hello");
    }

    #[test]
    fn long_text_is_cut_to_the_budget() {
        assert_eq!(truncate(&"x".repeat(100), 10).len(), 10);
    }

    /// Spans are byte offsets and cutting mid-character would panic.
    #[test]
    fn truncation_never_splits_a_character() {
        let text = "héllo wörld";

        for limit in 0..text.len() + 2 {
            let cut = truncate(text, limit);
            assert!(text.starts_with(&cut), "limit {limit} produced {cut:?}");
        }
    }

    #[test]
    fn boundaries_are_found_at_or_before_the_index() {
        let text = "aé";

        assert_eq!(floor_boundary(text, 0), 0);
        assert_eq!(floor_boundary(text, 1), 1);
        // Index 2 is inside the two-byte 'é'.
        assert_eq!(floor_boundary(text, 2), 1);
        assert_eq!(floor_boundary(text, 99), text.len());
    }

    // ------------------------------------------------------------- gating

    /// The failure has to say how to enable it; a bare "disabled" leaves the
    /// user hunting through config documentation.
    #[test]
    fn a_disabled_run_explains_how_to_enable_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let g = graph(Vec::new(), Vec::new());

        let error = enrich(dir.path(), &g, &LlmConfig::default(), 0).expect_err("should fail");

        let message = format!("{error}");
        assert!(message.contains("[graph.llm]"), "{message}");
        assert!(message.contains("OPENAI_API_KEY"), "{message}");
    }

    // ---------------------------------------------------------- persistence

    #[test]
    fn enrichment_round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = GraphPaths::new(dir.path());
        paths.ensure().expect("ensure");
        let mut enrichment = Enrichment::default();
        enrichment
            .files
            .insert("a.rs".to_string(), "does a thing".to_string());

        enrichment.write(&paths).expect("write");

        let loaded = Enrichment::load(&paths);
        assert_eq!(
            loaded.files.get("a.rs").map(String::as_str),
            Some("does a thing")
        );
    }

    #[test]
    fn absent_enrichment_reads_as_empty() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert!(Enrichment::load(&GraphPaths::new(dir.path())).is_empty());
    }
}
