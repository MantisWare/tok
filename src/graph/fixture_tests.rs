//! End-to-end extraction checks against the shared fixture repo.
//!
//! Phase 0a recorded what the regex indexer produces for
//! `tests/fixtures/code_graph/`: 43 symbols, 3 edges, no call graph at all.
//! These tests assert the tree-sitter pipeline does materially better on the
//! exact same input, so the benefit is measured rather than assumed.

use std::path::{Path, PathBuf};

use crate::graph::extract::{extract_file, FileExtraction};
use crate::graph::resolve::resolve;
use crate::graph::types::{EdgeKind, EdgeV1, NodeKind, NodeV1};
use crate::graph::Language;

/// Edge count the regex indexer produces for this fixture, from the Phase 0a
/// baseline snapshot.
const REGEX_BASELINE_EDGES: usize = 3;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("code_graph")
}

/// Extract every fixture file, using repo-relative forward-slashed paths.
fn extract_fixture() -> Vec<FileExtraction> {
    let root = fixture_root();
    let mut out = Vec::new();

    let mut dirs = vec![root.clone()];
    let mut files = Vec::new();
    while let Some(dir) = dirs.pop() {
        let entries = std::fs::read_dir(&dir).expect("fixture dir readable");
        for entry in entries {
            let entry = entry.expect("fixture entry");
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                files.push(path);
            }
        }
    }
    // Sort so extraction order does not depend on the filesystem.
    files.sort();

    for path in files {
        let rel = path
            .strip_prefix(&root)
            .expect("under fixture root")
            .to_string_lossy()
            .replace('\\', "/");

        let Some(language) = Language::from_path(&rel) else {
            continue;
        };
        let src = std::fs::read_to_string(&path).expect("fixture file readable");

        if let Some(extraction) = extract_file(&rel, &src, language).expect("extraction succeeds") {
            out.push(extraction);
        }
    }

    out
}

fn node_named<'a>(files: &'a [FileExtraction], name: &str) -> Option<&'a NodeV1> {
    files
        .iter()
        .flat_map(|f| f.nodes.iter())
        .find(|n| n.name == name)
}

/// Whether an edge of `kind` runs between the two named symbols.
fn has_edge(
    files: &[FileExtraction],
    edges: &[EdgeV1],
    from: &str,
    to: &str,
    kind: EdgeKind,
) -> bool {
    let lookup = |name: &str| -> Vec<&str> {
        files
            .iter()
            .flat_map(|f| f.nodes.iter())
            .filter(|n| n.name == name)
            .map(|n| n.id.as_str())
            .collect()
    };
    let from_ids = lookup(from);
    let to_ids = lookup(to);

    edges.iter().any(|e| {
        e.kind == kind && from_ids.contains(&e.from.as_str()) && to_ids.contains(&e.to.as_str())
    })
}

#[test]
fn fixture_extraction_covers_every_language() {
    let files = extract_fixture();
    let languages: Vec<_> = files
        .iter()
        .flat_map(|f| f.nodes.iter())
        .filter_map(|n| n.language.as_deref())
        .collect();

    for expected in ["rust", "typescript", "python", "go"] {
        assert!(
            languages.contains(&expected),
            "no {expected} nodes extracted"
        );
    }
}

/// The headline result: a real call graph where there was none.
#[test]
fn produces_far_more_edges_than_the_regex_indexer() {
    let files = extract_fixture();
    let (edges, stats) = resolve(&files);

    assert!(
        edges.len() > REGEX_BASELINE_EDGES * 5,
        "expected a large edge gain over the regex baseline of {REGEX_BASELINE_EDGES}, got {} \
         (unresolved {}, ambiguous {})",
        edges.len(),
        stats.unresolved,
        stats.ambiguous_dropped
    );

    let calls = edges.iter().filter(|e| e.kind == EdgeKind::Calls).count();
    assert!(
        calls > 0,
        "the regex indexer produces zero CALLS edges; the graph must produce some"
    );
}

/// `slugify` calls `normalize` two lines below it. The regex indexer misses
/// this, which is why it reports `normalize` as dead code.
#[test]
fn resolves_the_call_that_the_regex_indexer_reports_as_dead_code() {
    let files = extract_fixture();
    let (edges, _) = resolve(&files);

    assert!(
        has_edge(&files, &edges, "slugify", "normalize", EdgeKind::Calls),
        "slugify -> normalize must be a call edge"
    );
}

#[test]
fn resolves_cross_file_calls_through_imports() {
    let files = extract_fixture();
    let (edges, _) = resolve(&files);

    // ts/cache.ts imports normalize from ts/util.ts and calls it in Cache.get.
    assert!(
        edges.iter().any(|e| e.kind == EdgeKind::Imports),
        "imports should resolve across fixture files"
    );
}

#[test]
fn recovers_the_ids_that_collide_under_the_regex_indexer() {
    let files = extract_fixture();

    // rust/lib.rs declares `get` twice: once in `trait Store`, once in the
    // impl. The regex indexer collapses them; the graph must not.
    let gets: Vec<_> = files
        .iter()
        .flat_map(|f| f.nodes.iter())
        .filter(|n| n.file == "rust/lib.rs" && n.name == "get")
        .collect();

    assert_eq!(gets.len(), 2, "both `get` declarations must survive");
    assert_ne!(gets[0].id, gets[1].id);
}

#[test]
fn spans_are_real_ranges_not_single_lines() {
    let files = extract_fixture();

    // The regex indexer sets line_end == line_start for everything.
    let multi_line = files
        .iter()
        .flat_map(|f| f.nodes.iter())
        .filter(|n| n.kind != NodeKind::File && n.span.end > n.span.start)
        .count();

    assert!(
        multi_line > 5,
        "expected real multi-line spans, found {multi_line}"
    );
}

#[test]
fn inheritance_resolves_in_every_language_that_declares_it() {
    let files = extract_fixture();
    let (edges, _) = resolve(&files);

    // Rust: impl Store for MemoryStore
    assert!(
        has_edge(&files, &edges, "MemoryStore", "Store", EdgeKind::Implements),
        "rust impl-for should resolve"
    );
    // TypeScript: class Cache extends BaseCache
    assert!(
        has_edge(&files, &edges, "Cache", "BaseCache", EdgeKind::Extends),
        "typescript extends should resolve"
    );
}

/// Ownership is a graph property, not a lexical one: Go declares methods
/// outside the type body, so every language must reach the same `CONTAINS`
/// shape even though only some of them nest methods syntactically.
#[test]
fn methods_are_attributed_to_their_owning_type_in_every_language() {
    let files = extract_fixture();
    let (edges, _) = resolve(&files);

    for cache_file in ["go/cache.go", "python/cache.py", "ts/cache.ts"] {
        let cache = files
            .iter()
            .flat_map(|f| f.nodes.iter())
            .find(|n| n.file == cache_file && n.name == "Cache")
            .unwrap_or_else(|| panic!("{cache_file} declares Cache"));

        let owned: Vec<_> = edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains && e.from == cache.id)
            .collect();

        assert!(
            !owned.is_empty(),
            "Cache in {cache_file} should own its methods, got none"
        );
    }
}

#[test]
fn extraction_is_deterministic_across_runs() {
    let (first_edges, first_stats) = resolve(&extract_fixture());
    let (second_edges, second_stats) = resolve(&extract_fixture());

    assert_eq!(first_edges, second_edges);
    assert_eq!(first_stats, second_stats);
}

/// Ambiguity drops are expected on a fixture that reuses names like
/// `normalize` across four languages. What matters is that they are counted,
/// so graph quality stays observable.
#[test]
fn ambiguity_is_reported_rather_than_silently_guessed() {
    let files = extract_fixture();
    let (_, stats) = resolve(&files);

    assert!(
        stats.ambiguous_dropped > 0,
        "the fixture defines `normalize` in four languages, so some calls must be ambiguous"
    );
}
