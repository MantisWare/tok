//! The indexing pipeline: a directory in, a resolved [`GraphV1`] out.
//!
//! Sequence: walk → drift probe → extract (cached) → resolve → normalize.
//!
//! Resolution is deliberately whole-repo and runs after every file is known,
//! because a call in one file usually targets a declaration in another. That is
//! also why the cache memoizes *extraction* and not resolution: extraction is
//! per-file and pure, resolution is global and cheap by comparison.
//!
//! Files no grammar covers are skipped *here* rather than dropped outright:
//! `src/mem/indexer.rs` runs the regex parser over exactly the extensions this
//! pass declines, so Ruby, C#, and Java still reach `symbols` and stay
//! searchable through `tok mem search|find`. They carry no call edges, so they
//! are absent from the graph itself and from the commands that walk it — a
//! known limit, not an oversight.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};

use crate::graph::cache::ExtractCache;
use crate::graph::extract::FileExtraction;
use crate::graph::fingerprint::{self, DriftMode, FileStatus, Fingerprints};
use crate::graph::store::{self, GraphPaths};
use crate::graph::types::{FileEntryV1, GraphV1};
use crate::graph::Language;

/// Files above this size are almost always generated or vendored, and parsing
/// them costs far more than the symbols are worth. Matches the existing
/// indexer's limit so graph and SQLite coverage stay identical.
const MAX_FILE_BYTES: u64 = 1_048_576;

/// Directories that never contain first-party source. Mirrors the existing
/// indexer so the graph and the SQLite projection cover the same tree.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    "vendor",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    "coverage",
    ".nyc_output",
];

/// What a build did, for reporting and for tests that assert cache behaviour.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BuildStats {
    pub files_scanned: usize,
    /// Files re-parsed with tree-sitter this run.
    pub files_extracted: usize,
    /// Files served from the extract cache.
    pub files_cached: usize,
    /// Files whose language has no grammar in this build.
    pub files_unsupported: usize,
    /// Files present last run but gone now.
    pub files_removed: usize,
    pub nodes: usize,
    pub edges: usize,
    pub unresolved_refs: usize,
    pub ambiguous_dropped: usize,
    pub errors: Vec<String>,
}

/// Inputs to a build.
pub struct BuildOptions<'a> {
    pub repo_root: &'a Path,
    pub repo_id: &'a str,
    /// Reuse fingerprints and the extract cache from the previous run.
    pub incremental: bool,
    pub drift: DriftMode,
}

/// Result of a build, including the caches to persist.
pub struct BuildOutput {
    pub graph: GraphV1,
    pub stats: BuildStats,
    pub fingerprints: Fingerprints,
    /// Paths that disappeared since the last run, so their rows can be removed.
    pub removed: Vec<String>,
}

/// Build the graph for a repository.
#[cfg(feature = "graph")]
pub fn build(options: &BuildOptions) -> Result<BuildOutput> {
    let paths = GraphPaths::new(options.repo_root);
    let stamp = crate::graph::extractor_stamp();

    let previous: Fingerprints = if options.incremental {
        store::read_json(&paths.fingerprints(&stamp)).unwrap_or_default()
    } else {
        Fingerprints::new()
    };

    let mut cache = ExtractCache::load_or_new(
        options
            .incremental
            .then(|| store::read_json(&paths.extract_cache(&stamp)))
            .flatten(),
        &stamp,
    );

    let mut stats = BuildStats::default();
    let mut fingerprints = Fingerprints::new();
    let mut extractions: Vec<FileExtraction> = Vec::new();
    let mut file_entries: Vec<FileEntryV1> = Vec::new();
    let mut live_hashes: HashSet<String> = HashSet::new();

    for path in walk(options.repo_root, &mut stats) {
        let relative = relative_path(&path, options.repo_root);

        // Files no grammar covers are the regex extractor's job. Skipping
        // before the stat keeps them out of the fingerprint cache and out of
        // the scanned count, which reports files this pass is responsible for.
        let Some(language) = Language::from_path(&relative) else {
            stats.files_unsupported += 1;
            continue;
        };

        let Some((size, mtime_ms)) = fingerprint::stat(&path) else {
            continue;
        };
        if size > MAX_FILE_BYTES {
            continue;
        }

        stats.files_scanned += 1;

        let (status, hash) = fingerprint::probe(
            previous.get(&relative),
            size,
            mtime_ms,
            options.drift,
            || std::fs::read(&path).ok(),
        );

        let Some(hash) = hash else {
            stats
                .errors
                .push(format!("Could not read {}", path.display()));
            continue;
        };
        live_hashes.insert(hash.clone());

        // An unchanged file still needs its extraction, and the cache is keyed
        // by content, so a hit serves both the unchanged and the moved case.
        let cached = cache.get(&hash, &relative);
        let extraction = match (status, cached) {
            (_, Some(hit)) => {
                stats.files_cached += 1;
                Some(hit)
            }
            (FileStatus::Unchanged | FileStatus::Changed, None) => {
                match extract_one(&path, &relative, language) {
                    Ok(Some(fresh)) => {
                        stats.files_extracted += 1;
                        cache.insert(hash.clone(), fresh.clone());
                        Some(fresh)
                    }
                    // A grammar-covered file that extracts nothing means the
                    // grammar was compiled out of this build.
                    Ok(None) => None,
                    Err(e) => {
                        stats.errors.push(format!("{}: {e}", path.display()));
                        None
                    }
                }
            }
        };

        fingerprints.insert(
            relative.clone(),
            fingerprint::FileFingerprint {
                size,
                mtime_ms,
                hash,
            },
        );

        if let Some(extraction) = extraction {
            file_entries.push(FileEntryV1 {
                path: relative.clone(),
                hash: fingerprints[&relative].hash.clone(),
                size,
                language: language.as_str().to_string(),
                node_count: extraction.nodes.len() as u32,
            });
            extractions.push(extraction);
        }
    }

    let (edges, resolve_stats) = crate::graph::resolve::resolve(&extractions);
    stats.unresolved_refs = resolve_stats.unresolved;
    stats.ambiguous_dropped = resolve_stats.ambiguous_dropped;

    let mut graph = GraphV1::new(options.repo_id, &stamp);
    graph.nodes = extractions.into_iter().flat_map(|f| f.nodes).collect();
    graph.edges = edges;
    graph.files = file_entries;

    // Discovery reads the layout, so it only sees directories extraction saw;
    // the substance guard then needs node counts, which exist only now.
    let indexed: Vec<String> = graph.files.iter().map(|f| f.path.clone()).collect();
    let discovered = crate::graph::scopes::discover(options.repo_root, &indexed);
    graph.scopes = crate::graph::scopes::apply_min_substance(discovered, &graph.nodes);
    // A repo with no sub-projects stores nothing, which keeps single-scope
    // graphs byte-identical to those written before scopes existed.
    if graph.scopes.iter().all(|scope| scope.is_root()) {
        graph.scopes.clear();
    }

    graph.normalize();

    stats.nodes = graph.nodes.len();
    stats.edges = graph.edges.len();

    let current: Vec<String> = fingerprints.keys().cloned().collect();
    let removed = fingerprint::removed_paths(&previous, &current);
    stats.files_removed = removed.len();

    // Persist caches for the next run. A failure here costs a slow rebuild,
    // not a wrong answer, so it must not fail the index.
    cache.retain_hashes(&live_hashes);
    if paths.ensure().is_ok() {
        let _ = store::write_json(&paths.fingerprints(&stamp), &fingerprints);
        let _ = store::write_json(&paths.extract_cache(&stamp), &cache);
    }

    Ok(BuildOutput {
        graph,
        stats,
        fingerprints,
        removed,
    })
}

/// Extract one file, returning `None` when its grammar is not in this build.
#[cfg(feature = "graph")]
fn extract_one(path: &Path, relative: &str, language: Language) -> Result<Option<FileExtraction>> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;

    crate::graph::extract::extract_file(relative, &source, language)
}

/// Walk the repository, yielding candidate source files in a stable order.
///
/// Sorted because directory order varies by filesystem, and an unstable walk
/// would make `~N` id ordinals — which depend on encounter order — differ
/// between machines.
fn walk(root: &Path, stats: &mut BuildStats) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !SKIP_DIRS.contains(&name.as_ref())
        })
        .build();

    for entry in walker {
        match entry {
            Ok(e) if e.file_type().is_some_and(|ft| ft.is_file()) => files.push(e.into_path()),
            Ok(_) => {}
            Err(e) => stats.errors.push(format!("Walk error: {e}")),
        }
    }

    files.sort();
    files
}

/// Repo-relative, forward-slashed path.
fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Index nodes by the file that declares them, for the SQLite projection.
pub fn nodes_by_file(graph: &GraphV1) -> HashMap<&str, Vec<&crate::graph::NodeV1>> {
    let mut out: HashMap<&str, Vec<&crate::graph::NodeV1>> = HashMap::new();
    for node in &graph.nodes {
        out.entry(node.file.as_str()).or_default().push(node);
    }
    out
}

#[cfg(all(test, feature = "graph"))]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn repo(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        for (path, contents) in files {
            let full = dir.path().join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).expect("mkdir");
            }
            std::fs::write(&full, contents).expect("write");
        }
        dir
    }

    fn build_in(dir: &TempDir, incremental: bool) -> BuildOutput {
        build(&BuildOptions {
            repo_root: dir.path(),
            repo_id: "r",
            incremental,
            drift: DriftMode::Fast,
        })
        .expect("build succeeds")
    }

    #[test]
    fn builds_a_graph_from_a_directory() {
        let dir = repo(&[
            ("src/util.ts", "export function normalize(s) { return s; }"),
            (
                "src/cache.ts",
                "import { normalize } from './util';\n\
                 export function get(k) { return normalize(k); }",
            ),
        ]);

        let out = build_in(&dir, false);

        assert_eq!(out.stats.files_scanned, 2);
        assert_eq!(out.stats.files_extracted, 2);
        assert!(out.stats.nodes >= 2);
        assert!(
            out.graph
                .edges
                .iter()
                .any(|e| e.kind == crate::graph::EdgeKind::Calls),
            "cross-file call should resolve"
        );
    }

    #[test]
    fn a_cold_build_reads_every_file() {
        let dir = repo(&[("a.rs", "pub fn f() {}")]);
        let out = build_in(&dir, false);

        assert_eq!(out.stats.files_extracted, 1);
        assert_eq!(out.stats.files_cached, 0);
    }

    /// The point of the cache: a second run over an untouched tree should parse
    /// nothing at all.
    #[test]
    fn an_incremental_build_reparses_nothing_when_nothing_changed() {
        let dir = repo(&[("a.rs", "pub fn f() {}"), ("b.rs", "pub fn g() {}")]);
        build_in(&dir, false);

        let second = build_in(&dir, true);

        assert_eq!(second.stats.files_extracted, 0, "should be fully cached");
        assert_eq!(second.stats.files_cached, 2);
    }

    #[test]
    fn an_incremental_build_produces_the_same_graph_as_a_cold_one() {
        let dir = repo(&[
            ("src/util.ts", "export function normalize(s) { return s; }"),
            (
                "src/cache.ts",
                "import { normalize } from './util';\n\
                 export function get(k) { return normalize(k); }",
            ),
        ]);

        let cold = build_in(&dir, false);
        let warm = build_in(&dir, true);

        assert_eq!(cold.graph, warm.graph, "cache must not change the result");
    }

    #[test]
    fn an_edited_file_is_reextracted() {
        let dir = repo(&[("a.rs", "pub fn f() {}")]);
        build_in(&dir, false);

        std::fs::write(dir.path().join("a.rs"), "pub fn f() {}\npub fn second() {}").expect("edit");

        let out = build_in(&dir, true);
        assert_eq!(out.stats.files_extracted, 1);
        assert!(out.graph.nodes.iter().any(|n| n.name == "second"));
    }

    /// Today's `--incremental` leaves rows for deleted files behind forever.
    /// The build has to surface the deletion for the caller to act on.
    #[test]
    fn a_deleted_file_is_reported_as_removed() {
        let dir = repo(&[("a.rs", "pub fn f() {}"), ("b.rs", "pub fn g() {}")]);
        build_in(&dir, false);

        std::fs::remove_file(dir.path().join("b.rs")).expect("delete");
        let out = build_in(&dir, true);

        assert_eq!(out.removed, vec!["b.rs".to_string()]);
        assert_eq!(out.stats.files_removed, 1);
        assert!(!out.graph.nodes.iter().any(|n| n.file == "b.rs"));
    }

    #[test]
    fn the_graph_is_byte_identical_across_runs() {
        let dir = repo(&[
            ("a.rs", "pub fn one() {}\npub fn two() { one(); }"),
            ("b.rs", "pub fn three() {}"),
        ]);

        let first = serde_json::to_string(&build_in(&dir, false).graph).expect("serialize");
        let second = serde_json::to_string(&build_in(&dir, false).graph).expect("serialize");

        assert_eq!(first, second);
    }

    #[test]
    fn files_without_a_grammar_are_counted_not_dropped_silently() {
        let dir = repo(&[("notes.txt", "not code"), ("a.rs", "pub fn f() {}")]);
        let out = build_in(&dir, false);

        assert_eq!(out.stats.files_unsupported, 1);
        assert_eq!(out.stats.files_extracted, 1);
    }

    #[test]
    fn vendored_directories_are_skipped() {
        let dir = repo(&[
            ("node_modules/dep/index.ts", "export function vendored() {}"),
            ("src/a.ts", "export function mine() {}"),
        ]);

        let out = build_in(&dir, false);
        assert!(!out.graph.nodes.iter().any(|n| n.name == "vendored"));
        assert!(out.graph.nodes.iter().any(|n| n.name == "mine"));
    }

    #[test]
    fn oversized_files_are_skipped() {
        let big = "// pad\n".repeat(200_000);
        let dir = repo(&[("huge.rs", big.as_str()), ("a.rs", "pub fn f() {}")]);

        let out = build_in(&dir, false);
        assert_eq!(out.stats.files_scanned, 1, "only the small file counts");
    }

    #[test]
    fn a_non_incremental_build_ignores_the_cache() {
        let dir = repo(&[("a.rs", "pub fn f() {}")]);
        build_in(&dir, false);

        let out = build_in(&dir, false);
        assert_eq!(out.stats.files_cached, 0, "cold build must not reuse");
        assert_eq!(out.stats.files_extracted, 1);
    }

    #[test]
    fn file_entries_record_hashes_for_every_indexed_file() {
        let dir = repo(&[("a.rs", "pub fn f() {}")]);
        let out = build_in(&dir, false);

        let entry = out.graph.files.first().expect("one file entry");
        assert_eq!(entry.path, "a.rs");
        assert_eq!(entry.hash.len(), 64, "hex sha-256");
        assert_eq!(entry.language, "rust");
    }
}
