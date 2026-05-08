//! Directory indexer: walks a codebase, parses files, and populates the memory database.
//!
//! Uses the `ignore` crate to respect `.gitignore` rules and skip binary/vendor dirs.

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

    db::rebuild_fts(&tx)?;

    tx.commit().context("Failed to commit index transaction")?;
    conn.execute_batch("PRAGMA defer_foreign_keys = OFF;")?;

    Ok(stats)
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
}
