//! Directory indexer: walks a codebase, parses files, and populates the memory database.
//!
//! Uses the `ignore` crate to respect `.gitignore` rules and skip binary/vendor dirs.
//!
//! Two extractors run side by side, and which one handles a file depends only
//! on whether a tree-sitter grammar covers its language:
//!
//! - **Tree-sitter** for TypeScript/TSX/JS, Python, Go, and Rust. Produces real
//!   call, import, and inheritance edges plus true multi-line spans, and writes
//!   `.tok/graph/graph.json` alongside the SQLite projection.
//! - **Regex** for everything else — Ruby, C#, Java, and the rest. Symbols
//!   only, no call edges. Retained because dropping it to gain a call graph for
//!   four languages would silently remove indexing for the others.
//!
//! Both paths write into the same `symbols` and `edges` tables, so every
//! existing `tok mem` subcommand keeps working against the queries it already
//! uses.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;

use super::db;
use super::parser_regex;

/// Statistics from an indexing run.
#[derive(Debug, Default)]
pub struct IndexStats {
    pub files_scanned: usize,
    pub files_parsed: usize,
    pub symbols_inserted: usize,
    pub edges_inserted: usize,
    /// Files handled by tree-sitter rather than the regex fallback.
    #[cfg(feature = "graph")]
    pub files_graphed: usize,
    /// Files served from the extract cache instead of being re-parsed.
    #[cfg(feature = "graph")]
    pub files_cached: usize,
    /// Rows dropped because their file no longer exists.
    #[cfg(feature = "graph")]
    pub files_removed: usize,
    pub errors: Vec<String>,
}

/// Index a directory: walk files, extract symbols + edges, insert into DB.
///
/// If `incremental` is false, clears all existing data for the repo first.
pub fn index_directory(
    conn: &Connection,
    dir_path: &str,
    repo_id: &str,
    branch: &str,
    incremental: bool,
) -> Result<IndexStats> {
    let canonical = std::fs::canonicalize(dir_path)
        .with_context(|| format!("Cannot resolve path: {}", dir_path))?;
    let root = canonical.to_str().context("Path contains invalid UTF-8")?;

    db::upsert_repository(conn, repo_id, root, branch)?;

    if !incremental {
        db::clear_repo_symbols(conn, repo_id)?;
    }

    let mut stats = IndexStats::default();

    let walker = ignore::WalkBuilder::new(&canonical)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            // Skip common non-source directories
            !matches!(
                name.as_ref(),
                "node_modules"
                    | ".git"
                    | "target"
                    | "dist"
                    | "build"
                    | ".next"
                    | "__pycache__"
                    | ".venv"
                    | "venv"
                    | "vendor"
                    | ".tox"
                    | ".mypy_cache"
                    | ".pytest_cache"
                    | "coverage"
                    | ".nyc_output"
            )
        })
        .build();

    let mut all_symbols = Vec::new();
    let mut all_edges = Vec::new();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                stats.errors.push(format!("Walk error: {}", e));
                continue;
            }
        };

        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        if !parser_regex::is_supported_extension(ext) {
            continue;
        }

        // Files with a grammar belong to the graph pass, which extracts
        // strictly more from them. Skipping before the read avoids both the
        // wasted I/O and a double count in `files_scanned`.
        if handled_by_graph(ext) {
            continue;
        }

        stats.files_scanned += 1;

        // Skip large files (>1MB likely generated/vendored)
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > 1_048_576 {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                stats
                    .errors
                    .push(format!("Read error {}: {}", path.display(), e));
                continue;
            }
        };

        let relative_path = make_relative(path, root);
        let result = parser_regex::parse_file(&content, &relative_path, repo_id, branch);

        if !result.symbols.is_empty() {
            stats.files_parsed += 1;
            all_symbols.extend(result.symbols);
            all_edges.extend(result.edges);
        }
    }

    // Defer FK checks: edges may reference symbols from external crates/modules.
    conn.execute_batch("PRAGMA defer_foreign_keys = ON;")?;

    let tx = conn
        .unchecked_transaction()
        .context("Failed to begin transaction")?;

    stats.symbols_inserted = db::insert_symbols(&tx, &all_symbols)?;

    // Only insert edges whose source AND target exist in the symbols table
    let valid_edges = filter_valid_edges(&tx, &all_edges);
    stats.edges_inserted = db::insert_edges(&tx, &valid_edges)?;

    index_graph(&tx, &canonical, repo_id, branch, incremental, &mut stats);

    db::rebuild_fts(&tx)?;

    tx.commit().context("Failed to commit index transaction")?;
    conn.execute_batch("PRAGMA defer_foreign_keys = OFF;")?;

    Ok(stats)
}

/// Whether a tree-sitter grammar covers files with this extension.
fn handled_by_graph(extension: &str) -> bool {
    crate::graph::is_available() && crate::graph::Language::detect(extension).is_some()
}

/// Build the code graph and project it into SQLite.
///
/// Failures are recorded and swallowed rather than propagated: the regex pass
/// has already produced a usable index, and a grammar problem should degrade
/// the result rather than fail the whole command.
#[cfg(feature = "graph")]
fn index_graph(
    conn: &Connection,
    repo_root: &Path,
    repo_id: &str,
    branch: &str,
    incremental: bool,
    stats: &mut IndexStats,
) {
    use crate::graph::build::{self, BuildOptions};
    use crate::graph::fingerprint::DriftMode;

    let options = BuildOptions {
        repo_root,
        repo_id,
        incremental,
        drift: DriftMode::from_env(),
    };

    let output = match build::build(&options) {
        Ok(out) => out,
        Err(e) => {
            stats.errors.push(format!("Graph build failed: {e}"));
            return;
        }
    };

    // `files_scanned` counts files this index is responsible for. The graph
    // pass reports only grammar-covered files, and the regex pass above has
    // already counted the rest, so the two are disjoint.
    stats.files_scanned += output.stats.files_scanned;
    stats.files_parsed += output.graph.files.len();
    stats.files_graphed = output.graph.files.len();
    stats.files_cached = output.stats.files_cached;
    stats.errors.extend(output.stats.errors.iter().cloned());

    // Deleted files leave rows behind under the old `--incremental`, which is
    // how `search` and `dead-code` end up reporting code that no longer exists.
    if incremental && !output.removed.is_empty() {
        match crate::graph::project::remove_files(conn, repo_id, &output.removed) {
            Ok(removed) => stats.files_removed = removed,
            Err(e) => stats.errors.push(format!("Stale row cleanup failed: {e}")),
        }
    }

    match crate::graph::project::project(conn, &output.graph, repo_id, branch) {
        Ok(projection) => {
            stats.symbols_inserted += projection.symbols_inserted;
            stats.edges_inserted += projection.edges_inserted;
        }
        Err(e) => stats.errors.push(format!("Graph projection failed: {e}")),
    }

    // The graph file is a convenience for the retrieval layer; SQLite is
    // already consistent by this point, so a write failure is not fatal.
    let paths = crate::graph::store::GraphPaths::new(repo_root);
    if let Err(e) = crate::graph::write::write_graph(&paths, &output.graph) {
        stats.errors.push(format!("Graph write failed: {e}"));
    }
}

/// Without the `graph` feature the regex pass is the whole index.
#[cfg(not(feature = "graph"))]
fn index_graph(
    _conn: &Connection,
    _repo_root: &Path,
    _repo_id: &str,
    _branch: &str,
    _incremental: bool,
    _stats: &mut IndexStats,
) {
}

/// Keep only edges whose source and target both exist in the symbols table.
fn filter_valid_edges(
    conn: &rusqlite::Connection,
    edges: &[super::symbols::Edge],
) -> Vec<super::symbols::Edge> {
    let mut stmt = conn.prepare("SELECT 1 FROM symbols WHERE id = ?1").unwrap();

    edges
        .iter()
        .filter(|e| {
            let src_exists = stmt.exists(rusqlite::params![e.source_id]).unwrap_or(false);
            let tgt_exists = stmt.exists(rusqlite::params![e.target_id]).unwrap_or(false);
            src_exists && tgt_exists
        })
        .cloned()
        .collect()
}

/// Make a path relative to the repo root, using forward slashes.
fn make_relative(path: &Path, root: &str) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_relative_strips_prefix() {
        let p = Path::new("/home/user/project/src/main.rs");
        assert_eq!(make_relative(p, "/home/user/project"), "src/main.rs");
    }

    #[test]
    fn make_relative_handles_no_match() {
        let p = Path::new("/other/path/file.rs");
        assert_eq!(
            make_relative(p, "/home/user/project"),
            "/other/path/file.rs"
        );
    }

    #[cfg(feature = "graph")]
    mod dual_write {
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

        fn db() -> Connection {
            let conn = Connection::open_in_memory().expect("db");
            db::migrate(&conn).expect("migrate");
            conn
        }

        fn index(conn: &Connection, dir: &TempDir, incremental: bool) -> IndexStats {
            index_directory(
                conn,
                dir.path().to_str().expect("utf-8 path"),
                "r",
                "main",
                incremental,
            )
            .expect("index succeeds")
        }

        fn count(conn: &Connection, sql: &str) -> i64 {
            conn.query_row(sql, [], |r| r.get(0)).expect("count")
        }

        /// The headline gain: the regex indexer produces no call edges at all.
        #[test]
        fn indexing_produces_real_call_edges() {
            let dir = repo(&[(
                "src/a.rs",
                "pub fn helper() {}\npub fn main_entry() { helper(); }",
            )]);
            let conn = db();

            index(&conn, &dir, false);

            let calls = count(
                &conn,
                "SELECT COUNT(*) FROM edges WHERE edge_type = 'CALLS'",
            );
            assert!(calls > 0, "graph indexing should yield CALLS edges");
        }

        #[test]
        fn indexing_records_true_multi_line_spans() {
            let dir = repo(&[(
                "src/a.rs",
                "pub fn wide() {\n    let x = 1;\n    let y = 2;\n}\n",
            )]);
            let conn = db();
            index(&conn, &dir, false);

            let (start, end): (u32, u32) = conn
                .query_row(
                    "SELECT line_start, line_end FROM symbols WHERE name = 'wide'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .expect("row");

            assert!(end > start, "regex indexing collapsed spans to one line");
        }

        #[test]
        fn the_graph_file_is_written_beside_the_database() {
            let dir = repo(&[("src/a.rs", "pub fn f() {}")]);
            let conn = db();
            index(&conn, &dir, false);

            let paths = crate::graph::store::GraphPaths::new(dir.path());
            assert!(paths.graph().exists(), "graph.json should exist");
        }

        #[test]
        fn symbols_carry_their_readable_graph_id() {
            let dir = repo(&[("src/a.rs", "pub fn f() {}")]);
            let conn = db();
            index(&conn, &dir, false);

            let graph_id: String = conn
                .query_row("SELECT graph_id FROM symbols WHERE name = 'f'", [], |r| {
                    r.get(0)
                })
                .expect("row");

            assert_eq!(graph_id, "src/a.rs::f");
        }

        /// Languages without a grammar must keep working, or the port would
        /// have removed support TOK already shipped.
        #[test]
        fn languages_without_a_grammar_still_index_via_regex() {
            let dir = repo(&[("app/models.rb", "class Widget\n  def save\n  end\nend\n")]);
            let conn = db();

            index(&conn, &dir, false);

            let symbols = count(&conn, "SELECT COUNT(*) FROM symbols");
            assert!(symbols > 0, "Ruby indexing must not regress");
        }

        #[test]
        fn grammar_and_regex_languages_coexist_in_one_index() {
            let dir = repo(&[
                ("src/a.rs", "pub fn rust_fn() {}"),
                ("app/models.rb", "class Widget\nend\n"),
            ]);
            let conn = db();
            index(&conn, &dir, false);

            let rust = count(&conn, "SELECT COUNT(*) FROM symbols WHERE name = 'rust_fn'");
            let ruby = count(&conn, "SELECT COUNT(*) FROM symbols WHERE name = 'Widget'");

            assert_eq!(rust, 1);
            assert_eq!(ruby, 1);
        }

        #[test]
        fn a_grammar_file_is_not_parsed_twice() {
            let dir = repo(&[("src/a.rs", "pub fn f() {}")]);
            let conn = db();

            let stats = index(&conn, &dir, false);
            assert_eq!(stats.files_scanned, 1, "counted once, not once per pass");
            assert_eq!(stats.files_graphed, 1);
        }

        /// The `--incremental` defect: rows for deleted files used to survive
        /// forever, so `search` kept returning code that had been removed.
        #[test]
        fn incremental_reindex_removes_rows_for_deleted_files() {
            let dir = repo(&[
                ("src/a.rs", "pub fn kept() {}"),
                ("src/gone.rs", "pub fn vanishing() {}"),
            ]);
            let conn = db();
            index(&conn, &dir, false);

            assert_eq!(
                count(
                    &conn,
                    "SELECT COUNT(*) FROM symbols WHERE name = 'vanishing'"
                ),
                1
            );

            std::fs::remove_file(dir.path().join("src/gone.rs")).expect("delete");
            let stats = index(&conn, &dir, true);

            assert_eq!(stats.files_removed, 1);
            assert_eq!(
                count(
                    &conn,
                    "SELECT COUNT(*) FROM symbols WHERE name = 'vanishing'"
                ),
                0,
                "stale rows must not survive an incremental reindex"
            );
            assert_eq!(
                count(&conn, "SELECT COUNT(*) FROM symbols WHERE name = 'kept'"),
                1
            );
        }

        #[test]
        fn incremental_reindex_reuses_the_extract_cache() {
            let dir = repo(&[("src/a.rs", "pub fn f() {}"), ("src/b.rs", "pub fn g() {}")]);
            let conn = db();
            index(&conn, &dir, false);

            let stats = index(&conn, &dir, true);
            assert_eq!(stats.files_cached, 2, "unchanged files should not reparse");
        }

        #[test]
        fn reindexing_is_idempotent() {
            let dir = repo(&[(
                "src/a.rs",
                "pub fn helper() {}\npub fn main_entry() { helper(); }",
            )]);
            let conn = db();

            index(&conn, &dir, false);
            let symbols = count(&conn, "SELECT COUNT(*) FROM symbols");
            let edges = count(&conn, "SELECT COUNT(*) FROM edges");

            index(&conn, &dir, false);
            assert_eq!(count(&conn, "SELECT COUNT(*) FROM symbols"), symbols);
            assert_eq!(count(&conn, "SELECT COUNT(*) FROM edges"), edges);
        }

        #[test]
        fn the_full_text_index_covers_graph_symbols() {
            let dir = repo(&[("src/a.rs", "pub fn searchable_thing() {}")]);
            let conn = db();
            index(&conn, &dir, false);

            let hits = count(
                &conn,
                "SELECT COUNT(*) FROM symbols_fts WHERE symbols_fts MATCH 'searchable_thing'",
            );
            assert_eq!(hits, 1, "graph symbols must be searchable");
        }
    }
}
