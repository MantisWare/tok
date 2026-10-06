//! SQLite storage layer for the tok mem structural memory.
//!
//! Manages `memory.db` with tables for repositories, symbols, edges,
//! episodes, and an FTS5 virtual table for full-text search.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use super::symbols::{Edge, EdgeType, Repository, Symbol, SymbolKind};
use crate::core::constants::{MEMORY_DB, TOK_DATA_DIR};

/// Open (or create) the memory database, run migrations, return the connection.
pub fn open() -> Result<Connection> {
    let db_path = get_memory_db_path()?;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create directory {:?}", parent))?;
    }

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Cannot open memory database at {:?}", db_path))?;

    let _ = conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA busy_timeout=5000;
         PRAGMA foreign_keys=ON;",
    );

    migrate(&conn)?;
    Ok(conn)
}

/// Resolve the path for `memory.db`.
fn get_memory_db_path() -> Result<std::path::PathBuf> {
    if let Ok(p) = std::env::var("TOK_MEMORY_DB_PATH") {
        return Ok(std::path::PathBuf::from(p));
    }

    let data_dir = dirs::data_local_dir()
        .context("Cannot determine local data directory")?
        .join(TOK_DATA_DIR);

    Ok(data_dir.join(MEMORY_DB))
}

/// Create tables and indexes if they don't exist.
///
/// Visible to the crate so the graph projection can build a schema-complete
/// in-memory database in its tests, rather than duplicating the schema and
/// letting the copy drift from this one.
pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS repositories (
            repo_id   TEXT PRIMARY KEY,
            path      TEXT NOT NULL,
            branch    TEXT NOT NULL DEFAULT 'main',
            last_indexed_at TEXT NOT NULL DEFAULT '',
            last_episode_id TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS symbols (
            id         TEXT PRIMARY KEY,
            repo_id    TEXT NOT NULL REFERENCES repositories(repo_id) ON DELETE CASCADE,
            name       TEXT NOT NULL,
            kind       TEXT NOT NULL,
            file_path  TEXT NOT NULL,
            line_start INTEGER NOT NULL DEFAULT 0,
            line_end   INTEGER NOT NULL DEFAULT 0,
            signature  TEXT NOT NULL DEFAULT '',
            doc_comment TEXT NOT NULL DEFAULT '',
            branch     TEXT NOT NULL DEFAULT 'main',
            indexed_at TEXT NOT NULL DEFAULT '',
            graph_id   TEXT NOT NULL DEFAULT ''
        );

        CREATE INDEX IF NOT EXISTS idx_symbols_repo   ON symbols(repo_id);
        CREATE INDEX IF NOT EXISTS idx_symbols_name    ON symbols(name);
        CREATE INDEX IF NOT EXISTS idx_symbols_file    ON symbols(file_path);
        CREATE INDEX IF NOT EXISTS idx_symbols_kind    ON symbols(kind);

        CREATE TABLE IF NOT EXISTS edges (
            id        INTEGER PRIMARY KEY,
            source_id TEXT NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
            target_id TEXT NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
            edge_type TEXT NOT NULL,
            repo_id   TEXT NOT NULL,
            branch    TEXT NOT NULL DEFAULT 'main'
        );

        CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
        CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
        CREATE INDEX IF NOT EXISTS idx_edges_type   ON edges(edge_type);
        CREATE INDEX IF NOT EXISTS idx_edges_repo   ON edges(repo_id);

        CREATE TABLE IF NOT EXISTS episodes (
            id           TEXT PRIMARY KEY,
            repo_id      TEXT NOT NULL REFERENCES repositories(repo_id) ON DELETE CASCADE,
            symbol_id    TEXT NOT NULL,
            change_type  TEXT NOT NULL,
            commit_hash  TEXT NOT NULL DEFAULT '',
            timestamp    TEXT NOT NULL DEFAULT '',
            diff_summary TEXT NOT NULL DEFAULT '',
            branch       TEXT NOT NULL DEFAULT 'main'
        );

        CREATE INDEX IF NOT EXISTS idx_episodes_repo   ON episodes(repo_id);
        CREATE INDEX IF NOT EXISTS idx_episodes_symbol ON episodes(symbol_id);
        CREATE INDEX IF NOT EXISTS idx_episodes_time   ON episodes(timestamp);",
    )
    .context("Failed to create core tables")?;

    // FTS5 virtual table — standalone (no content= sync issues with INSERT OR REPLACE)
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
            name,
            signature,
            doc_comment,
            file_path,
            symbol_id
        );",
    )
    .context("Failed to create FTS5 table")?;

    add_missing_columns(conn)?;

    Ok(())
}

/// Bring an existing database up to the current column set.
///
/// The schema above is `CREATE TABLE IF NOT EXISTS`, which is a no-op against a
/// database created by an earlier version — so new columns have to be added
/// here as well. Every addition must be nullable or carry a default, since
/// SQLite cannot add a `NOT NULL` column without one.
fn add_missing_columns(conn: &Connection) -> Result<()> {
    // `graph_id` links a row to its node in the tree-sitter code graph. It is
    // *not* the primary key: `symbols.id` keeps its
    // `sha256(repo_id:file_path:name:kind)[:16]` formula because
    // `episodes.symbol_id` already references those values, and rewriting them
    // would orphan every recorded change.
    //
    // Empty string means "not linked", matching how the rest of this table
    // represents absent text rather than introducing NULL handling.
    add_column_if_missing(conn, "symbols", "graph_id", "TEXT NOT NULL DEFAULT ''")?;

    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_symbols_graph ON symbols(graph_id);")
        .context("Failed to create graph_id index")?;

    Ok(())
}

/// Add a column when absent. Safe to call on every open.
fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    if column_exists(conn, table, column)? {
        return Ok(());
    }

    // Table and column names are compile-time constants from this module, never
    // user input, so the format! here cannot be injected into.
    conn.execute_batch(&format!(
        "ALTER TABLE {table} ADD COLUMN {column} {definition};"
    ))
    .with_context(|| format!("Failed to add {table}.{column}"))?;

    Ok(())
}

/// Whether `table` already has `column`, via `PRAGMA table_info`.
fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table});"))
        .with_context(|| format!("Failed to inspect {table}"))?;

    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        // PRAGMA table_info columns: cid, name, type, notnull, dflt_value, pk
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

// ── Repository CRUD ──

pub fn upsert_repository(conn: &Connection, repo_id: &str, path: &str, branch: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO repositories (repo_id, path, branch, last_indexed_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(repo_id) DO UPDATE SET
            path = excluded.path,
            branch = excluded.branch,
            last_indexed_at = excluded.last_indexed_at",
        params![repo_id, path, branch, now],
    )
    .context("Failed to upsert repository")?;
    Ok(())
}

pub fn get_repository(conn: &Connection, repo_id: &str) -> Result<Option<Repository>> {
    let mut stmt = conn.prepare(
        "SELECT r.repo_id, r.path, r.branch, r.last_indexed_at, r.last_episode_id,
                (SELECT COUNT(*) FROM symbols s WHERE s.repo_id = r.repo_id) as sym_count,
                (SELECT COUNT(*) FROM edges e WHERE e.repo_id = r.repo_id) as edge_count,
                (SELECT COUNT(DISTINCT s2.file_path) FROM symbols s2 WHERE s2.repo_id = r.repo_id) as file_count
         FROM repositories r
         WHERE r.repo_id = ?1",
    )?;

    let repo = stmt
        .query_row(params![repo_id], |row| {
            Ok(Repository {
                repo_id: row.get(0)?,
                path: row.get(1)?,
                branch: row.get(2)?,
                last_indexed_at: row.get(3)?,
                last_episode_id: row.get(4)?,
                symbol_count: row.get::<_, usize>(5)?,
                edge_count: row.get::<_, usize>(6)?,
                file_count: row.get::<_, usize>(7)?,
            })
        })
        .optional()
        .context("Failed to query repository")?;

    Ok(repo)
}

pub fn list_repositories(conn: &Connection) -> Result<Vec<Repository>> {
    let mut stmt = conn.prepare(
        "SELECT r.repo_id, r.path, r.branch, r.last_indexed_at, r.last_episode_id,
                (SELECT COUNT(*) FROM symbols s WHERE s.repo_id = r.repo_id),
                (SELECT COUNT(*) FROM edges e WHERE e.repo_id = r.repo_id),
                (SELECT COUNT(DISTINCT s2.file_path) FROM symbols s2 WHERE s2.repo_id = r.repo_id)
         FROM repositories r
         ORDER BY r.last_indexed_at DESC",
    )?;

    let repos = stmt
        .query_map([], |row| {
            Ok(Repository {
                repo_id: row.get(0)?,
                path: row.get(1)?,
                branch: row.get(2)?,
                last_indexed_at: row.get(3)?,
                last_episode_id: row.get(4)?,
                symbol_count: row.get::<_, usize>(5)?,
                edge_count: row.get::<_, usize>(6)?,
                file_count: row.get::<_, usize>(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to list repositories")?;

    Ok(repos)
}

pub fn delete_repository(conn: &Connection, repo_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM repositories WHERE repo_id = ?1",
        params![repo_id],
    )?;
    Ok(())
}

// ── Symbol CRUD ──

pub fn insert_symbols(conn: &Connection, symbols: &[Symbol]) -> Result<usize> {
    let mut count = 0usize;
    let mut stmt = conn.prepare(
        "INSERT OR REPLACE INTO symbols
            (id, repo_id, name, kind, file_path, line_start, line_end, signature, doc_comment, branch, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;

    for sym in symbols {
        stmt.execute(params![
            sym.id,
            sym.repo_id,
            sym.name,
            sym.kind.as_str(),
            sym.file_path,
            sym.line_start,
            sym.line_end,
            sym.signature,
            sym.doc_comment,
            sym.branch,
            sym.indexed_at,
        ])?;
        count += 1;
    }

    Ok(count)
}

/// Rebuild the FTS5 index from the symbols table.
pub fn rebuild_fts(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM symbols_fts;
         INSERT INTO symbols_fts(name, signature, doc_comment, file_path, symbol_id)
            SELECT name, signature, doc_comment, file_path, id FROM symbols;",
    )
    .context("Failed to rebuild FTS index")?;
    Ok(())
}

/// Remove all symbols (and cascade edges) for a given repo, then rebuild FTS.
pub fn clear_repo_symbols(conn: &Connection, repo_id: &str) -> Result<()> {
    conn.execute("DELETE FROM edges WHERE repo_id = ?1", params![repo_id])?;
    conn.execute("DELETE FROM symbols WHERE repo_id = ?1", params![repo_id])?;
    conn.execute("DELETE FROM episodes WHERE repo_id = ?1", params![repo_id])?;
    Ok(())
}

// ── Edge CRUD ──

pub fn insert_edges(conn: &Connection, edges: &[Edge]) -> Result<usize> {
    let mut count = 0usize;
    let mut stmt = conn.prepare(
        "INSERT INTO edges (source_id, target_id, edge_type, repo_id, branch)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;

    for edge in edges {
        stmt.execute(params![
            edge.source_id,
            edge.target_id,
            edge.edge_type.as_str(),
            edge.repo_id,
            edge.branch,
        ])?;
        count += 1;
    }

    Ok(count)
}

// ── Queries ──

pub fn find_symbol_by_name(
    conn: &Connection,
    name: &str,
    repo_id: Option<&str>,
    kind: Option<SymbolKind>,
    limit: usize,
) -> Result<Vec<Symbol>> {
    let query = format!(
        "SELECT id, repo_id, name, kind, file_path, line_start, line_end,
                signature, doc_comment, branch, indexed_at
         FROM symbols
         WHERE name = ?1
           {}
           {}
         LIMIT ?2",
        if repo_id.is_some() {
            "AND repo_id = ?3"
        } else {
            ""
        },
        if kind.is_some() { "AND kind = ?4" } else { "" },
    );

    let mut stmt = conn.prepare(&query)?;

    let row_mapper = |row: &rusqlite::Row| -> rusqlite::Result<Symbol> {
        Ok(Symbol {
            id: row.get(0)?,
            repo_id: row.get(1)?,
            name: row.get(2)?,
            kind: SymbolKind::from_str(&row.get::<_, String>(3)?).unwrap_or(SymbolKind::Function),
            file_path: row.get(4)?,
            line_start: row.get(5)?,
            line_end: row.get(6)?,
            signature: row.get(7)?,
            doc_comment: row.get(8)?,
            branch: row.get(9)?,
            indexed_at: row.get(10)?,
        })
    };

    let results: Vec<Symbol> = match (repo_id, kind) {
        (Some(rid), Some(k)) => stmt
            .query_map(params![name, limit, rid, k.as_str()], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        (Some(rid), None) => stmt
            .query_map(params![name, limit, rid], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        (None, Some(k)) => {
            let q2 = "SELECT id, repo_id, name, kind, file_path, line_start, line_end,
                              signature, doc_comment, branch, indexed_at
                       FROM symbols WHERE name = ?1 AND kind = ?3 LIMIT ?2";
            let mut s2 = conn.prepare(q2)?;
            let collected = s2
                .query_map(params![name, limit, k.as_str()], row_mapper)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            collected
        }
        (None, None) => stmt
            .query_map(params![name, limit], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };

    Ok(results)
}

/// Fuzzy search: match symbols whose name contains the query substring.
pub fn find_symbol_fuzzy(
    conn: &Connection,
    query: &str,
    repo_id: Option<&str>,
    limit: usize,
) -> Result<Vec<Symbol>> {
    let pattern = format!("%{}%", query);

    let sql = if repo_id.is_some() {
        "SELECT id, repo_id, name, kind, file_path, line_start, line_end,
                signature, doc_comment, branch, indexed_at
         FROM symbols
         WHERE name LIKE ?1 AND repo_id = ?3
         ORDER BY length(name) ASC
         LIMIT ?2"
    } else {
        "SELECT id, repo_id, name, kind, file_path, line_start, line_end,
                signature, doc_comment, branch, indexed_at
         FROM symbols
         WHERE name LIKE ?1
         ORDER BY length(name) ASC
         LIMIT ?2"
    };

    let mut stmt = conn.prepare(sql)?;

    let row_mapper = |row: &rusqlite::Row| -> rusqlite::Result<Symbol> {
        Ok(Symbol {
            id: row.get(0)?,
            repo_id: row.get(1)?,
            name: row.get(2)?,
            kind: SymbolKind::from_str(&row.get::<_, String>(3)?).unwrap_or(SymbolKind::Function),
            file_path: row.get(4)?,
            line_start: row.get(5)?,
            line_end: row.get(6)?,
            signature: row.get(7)?,
            doc_comment: row.get(8)?,
            branch: row.get(9)?,
            indexed_at: row.get(10)?,
        })
    };

    let results = if let Some(rid) = repo_id {
        stmt.query_map(params![pattern, limit, rid], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![pattern, limit], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };

    Ok(results)
}

#[allow(dead_code)]
pub fn get_symbol_by_id(conn: &Connection, id: &str) -> Result<Option<Symbol>> {
    let mut stmt = conn.prepare(
        "SELECT id, repo_id, name, kind, file_path, line_start, line_end,
                signature, doc_comment, branch, indexed_at
         FROM symbols WHERE id = ?1",
    )?;

    let sym = stmt
        .query_row(params![id], |row| {
            Ok(Symbol {
                id: row.get(0)?,
                repo_id: row.get(1)?,
                name: row.get(2)?,
                kind: SymbolKind::from_str(&row.get::<_, String>(3)?)
                    .unwrap_or(SymbolKind::Function),
                file_path: row.get(4)?,
                line_start: row.get(5)?,
                line_end: row.get(6)?,
                signature: row.get(7)?,
                doc_comment: row.get(8)?,
                branch: row.get(9)?,
                indexed_at: row.get(10)?,
            })
        })
        .optional()?;

    Ok(sym)
}

/// Get edges where the given symbol is either source or target.
pub fn get_edges_for_symbol(
    conn: &Connection,
    symbol_id: &str,
    edge_type: Option<EdgeType>,
) -> Result<Vec<(Edge, Option<Symbol>)>> {
    let base = if edge_type.is_some() {
        "SELECT e.source_id, e.target_id, e.edge_type, e.repo_id, e.branch,
                s.id, s.repo_id, s.name, s.kind, s.file_path, s.line_start, s.line_end,
                s.signature, s.doc_comment, s.branch, s.indexed_at
         FROM edges e
         LEFT JOIN symbols s ON (CASE WHEN e.source_id = ?1 THEN e.target_id ELSE e.source_id END) = s.id
         WHERE (e.source_id = ?1 OR e.target_id = ?1) AND e.edge_type = ?2"
    } else {
        "SELECT e.source_id, e.target_id, e.edge_type, e.repo_id, e.branch,
                s.id, s.repo_id, s.name, s.kind, s.file_path, s.line_start, s.line_end,
                s.signature, s.doc_comment, s.branch, s.indexed_at
         FROM edges e
         LEFT JOIN symbols s ON (CASE WHEN e.source_id = ?1 THEN e.target_id ELSE e.source_id END) = s.id
         WHERE (e.source_id = ?1 OR e.target_id = ?1)"
    };

    let mut stmt = conn.prepare(base)?;

    let row_mapper = |row: &rusqlite::Row| -> rusqlite::Result<(Edge, Option<Symbol>)> {
        let edge = Edge {
            source_id: row.get(0)?,
            target_id: row.get(1)?,
            edge_type: EdgeType::from_str(&row.get::<_, String>(2)?).unwrap_or(EdgeType::Calls),
            repo_id: row.get(3)?,
            branch: row.get(4)?,
        };

        let sym_id: Option<String> = row.get(5)?;
        let sym = if let Some(sid) = sym_id {
            Some(Symbol {
                id: sid,
                repo_id: row.get(6)?,
                name: row.get(7)?,
                kind: SymbolKind::from_str(&row.get::<_, String>(8)?)
                    .unwrap_or(SymbolKind::Function),
                file_path: row.get(9)?,
                line_start: row.get(10)?,
                line_end: row.get(11)?,
                signature: row.get(12)?,
                doc_comment: row.get(13)?,
                branch: row.get(14)?,
                indexed_at: row.get(15)?,
            })
        } else {
            None
        };

        Ok((edge, sym))
    };

    let results = if let Some(et) = edge_type {
        stmt.query_map(params![symbol_id, et.as_str()], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![symbol_id], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };

    Ok(results)
}

/// Count symbols, edges, and files for a repo.
pub fn repo_stats(conn: &Connection, repo_id: &str) -> Result<(usize, usize, usize)> {
    let sym_count: usize = conn.query_row(
        "SELECT COUNT(*) FROM symbols WHERE repo_id = ?1",
        params![repo_id],
        |r| r.get(0),
    )?;
    let edge_count: usize = conn.query_row(
        "SELECT COUNT(*) FROM edges WHERE repo_id = ?1",
        params![repo_id],
        |r| r.get(0),
    )?;
    let file_count: usize = conn.query_row(
        "SELECT COUNT(DISTINCT file_path) FROM symbols WHERE repo_id = ?1",
        params![repo_id],
        |r| r.get(0),
    )?;
    Ok((sym_count, edge_count, file_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::symbols::generate_symbol_id;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        let _ = conn.execute_batch("PRAGMA foreign_keys=ON;");
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = in_memory_db();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
    }

    /// The upgrade path that matters: a database created before the code graph
    /// must gain `graph_id` without losing rows.
    #[test]
    fn migrate_adds_graph_id_to_a_pre_existing_database() {
        let conn = Connection::open_in_memory().unwrap();

        // The `symbols` table exactly as an older TOK created it.
        conn.execute_batch(
            "CREATE TABLE repositories (
                repo_id TEXT PRIMARY KEY, path TEXT NOT NULL,
                branch TEXT NOT NULL DEFAULT 'main',
                last_indexed_at TEXT NOT NULL DEFAULT '',
                last_episode_id TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE symbols (
                id TEXT PRIMARY KEY, repo_id TEXT NOT NULL, name TEXT NOT NULL,
                kind TEXT NOT NULL, file_path TEXT NOT NULL,
                line_start INTEGER NOT NULL DEFAULT 0,
                line_end INTEGER NOT NULL DEFAULT 0,
                signature TEXT NOT NULL DEFAULT '',
                doc_comment TEXT NOT NULL DEFAULT '',
                branch TEXT NOT NULL DEFAULT 'main',
                indexed_at TEXT NOT NULL DEFAULT ''
             );
             INSERT INTO repositories (repo_id, path) VALUES ('r', '/tmp/r');
             INSERT INTO symbols (id, repo_id, name, kind, file_path)
                VALUES ('legacy-id', 'r', 'oldFn', 'Function', 'a.ts');",
        )
        .unwrap();

        assert!(!column_exists(&conn, "symbols", "graph_id").unwrap());

        migrate(&conn).unwrap();

        assert!(column_exists(&conn, "symbols", "graph_id").unwrap());

        // The pre-existing row survives, keeps its id, and defaults to unlinked.
        let (id, graph_id): (String, String) = conn
            .query_row(
                "SELECT id, graph_id FROM symbols WHERE name = 'oldFn'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(id, "legacy-id", "symbol ids must not be rewritten");
        assert_eq!(graph_id, "", "unlinked rows use the empty-string sentinel");
    }

    /// `episodes.symbol_id` holds a bare string, so nothing at the database
    /// level would complain if the migration rewrote symbol ids — the join
    /// would just start returning nothing. This walks that join.
    #[test]
    fn episodes_still_resolve_to_their_symbols_after_the_upgrade() {
        let conn = Connection::open_in_memory().unwrap();

        conn.execute_batch(
            "CREATE TABLE repositories (
                repo_id TEXT PRIMARY KEY, path TEXT NOT NULL,
                branch TEXT NOT NULL DEFAULT 'main',
                last_indexed_at TEXT NOT NULL DEFAULT '',
                last_episode_id TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE symbols (
                id TEXT PRIMARY KEY, repo_id TEXT NOT NULL, name TEXT NOT NULL,
                kind TEXT NOT NULL, file_path TEXT NOT NULL,
                line_start INTEGER NOT NULL DEFAULT 0,
                line_end INTEGER NOT NULL DEFAULT 0,
                signature TEXT NOT NULL DEFAULT '',
                doc_comment TEXT NOT NULL DEFAULT '',
                branch TEXT NOT NULL DEFAULT 'main',
                indexed_at TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE episodes (
                id TEXT PRIMARY KEY, repo_id TEXT NOT NULL,
                symbol_id TEXT NOT NULL, change_type TEXT NOT NULL,
                commit_hash TEXT NOT NULL DEFAULT '',
                timestamp TEXT NOT NULL DEFAULT '',
                diff_summary TEXT NOT NULL DEFAULT '',
                branch TEXT NOT NULL DEFAULT 'main'
             );
             INSERT INTO repositories (repo_id, path) VALUES ('r', '/tmp/r');
             INSERT INTO symbols (id, repo_id, name, kind, file_path)
                VALUES ('legacy-id', 'r', 'oldFn', 'Function', 'a.ts');
             INSERT INTO episodes (id, repo_id, symbol_id, change_type, diff_summary)
                VALUES ('e1', 'r', 'legacy-id', 'modified', 'renamed for clarity');",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let name: String = conn
            .query_row(
                "SELECT s.name FROM episodes e
                 JOIN symbols s ON s.id = e.symbol_id
                 WHERE e.id = 'e1'",
                [],
                |r| r.get(0),
            )
            .expect("the episode lost its symbol");
        assert_eq!(name, "oldFn");
    }

    #[test]
    fn adding_a_column_twice_is_a_no_op() {
        let conn = in_memory_db();
        add_missing_columns(&conn).unwrap();
        add_missing_columns(&conn).unwrap();
        assert!(column_exists(&conn, "symbols", "graph_id").unwrap());
    }

    #[test]
    fn column_exists_reports_absent_columns() {
        let conn = in_memory_db();
        assert!(column_exists(&conn, "symbols", "name").unwrap());
        assert!(!column_exists(&conn, "symbols", "no_such_column").unwrap());
    }

    /// Pins the on-disk schema. `episodes.symbol_id` references the symbol id
    /// formula, and agents read `symbols_fts` through `tok mem search`, so any
    /// change here has to be additive and deliberate.
    #[test]
    fn schema_baseline() {
        let conn = in_memory_db();
        let mut stmt = conn
            .prepare(
                "SELECT type, name, COALESCE(sql, '') FROM sqlite_master \
                 WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
            )
            .unwrap();
        let rows: Vec<String> = stmt
            .query_map([], |row| {
                let kind: String = row.get(0)?;
                let name: String = row.get(1)?;
                let sql: String = row.get(2)?;
                Ok(format!("[{kind}] {name}\n{}\n", sql.trim()))
            })
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        insta::assert_snapshot!("mem_db_schema", rows.join("\n"));
    }

    #[test]
    fn upsert_and_get_repository() {
        let conn = in_memory_db();
        upsert_repository(&conn, "test-repo", "/tmp/test", "main").unwrap();

        let repo = get_repository(&conn, "test-repo").unwrap().unwrap();
        assert_eq!(repo.repo_id, "test-repo");
        assert_eq!(repo.path, "/tmp/test");
    }

    #[test]
    fn insert_and_find_symbols() {
        let conn = in_memory_db();
        upsert_repository(&conn, "r", "/tmp/r", "main").unwrap();

        let sym = Symbol {
            id: generate_symbol_id("r", "src/lib.rs", "do_thing", SymbolKind::Function),
            repo_id: "r".to_string(),
            name: "do_thing".to_string(),
            kind: SymbolKind::Function,
            file_path: "src/lib.rs".to_string(),
            line_start: 10,
            line_end: 20,
            signature: "fn do_thing() -> bool".to_string(),
            doc_comment: "Does the thing".to_string(),
            branch: "main".to_string(),
            indexed_at: "2026-01-01T00:00:00Z".to_string(),
        };

        insert_symbols(&conn, &[sym]).unwrap();
        let found = find_symbol_by_name(&conn, "do_thing", None, None, 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "do_thing");
    }

    #[test]
    fn insert_edges_and_query() {
        let conn = in_memory_db();
        upsert_repository(&conn, "r", "/tmp/r", "main").unwrap();

        let s1 = Symbol {
            id: "sym1".to_string(),
            repo_id: "r".to_string(),
            name: "caller".to_string(),
            kind: SymbolKind::Function,
            file_path: "a.rs".to_string(),
            line_start: 1,
            line_end: 5,
            signature: "fn caller()".to_string(),
            doc_comment: String::new(),
            branch: "main".to_string(),
            indexed_at: String::new(),
        };
        let s2 = Symbol {
            id: "sym2".to_string(),
            repo_id: "r".to_string(),
            name: "callee".to_string(),
            kind: SymbolKind::Function,
            file_path: "b.rs".to_string(),
            line_start: 1,
            line_end: 5,
            signature: "fn callee()".to_string(),
            doc_comment: String::new(),
            branch: "main".to_string(),
            indexed_at: String::new(),
        };

        insert_symbols(&conn, &[s1, s2]).unwrap();

        let edge = Edge {
            source_id: "sym1".to_string(),
            target_id: "sym2".to_string(),
            edge_type: EdgeType::Calls,
            repo_id: "r".to_string(),
            branch: "main".to_string(),
        };

        insert_edges(&conn, &[edge]).unwrap();

        let edges = get_edges_for_symbol(&conn, "sym1", None).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].0.edge_type, EdgeType::Calls);
    }

    #[test]
    fn fuzzy_search_works() {
        let conn = in_memory_db();
        upsert_repository(&conn, "r", "/tmp/r", "main").unwrap();

        let sym = Symbol {
            id: "s1".to_string(),
            repo_id: "r".to_string(),
            name: "handleUserLogin".to_string(),
            kind: SymbolKind::Function,
            file_path: "auth.rs".to_string(),
            line_start: 1,
            line_end: 10,
            signature: "fn handleUserLogin()".to_string(),
            doc_comment: String::new(),
            branch: "main".to_string(),
            indexed_at: String::new(),
        };
        insert_symbols(&conn, &[sym]).unwrap();

        let found = find_symbol_fuzzy(&conn, "Login", None, 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "handleUserLogin");
    }

    #[test]
    fn delete_repository_cascades() {
        let conn = in_memory_db();
        upsert_repository(&conn, "r", "/tmp/r", "main").unwrap();

        let sym = Symbol {
            id: "s1".to_string(),
            repo_id: "r".to_string(),
            name: "foo".to_string(),
            kind: SymbolKind::Function,
            file_path: "f.rs".to_string(),
            line_start: 1,
            line_end: 1,
            signature: String::new(),
            doc_comment: String::new(),
            branch: "main".to_string(),
            indexed_at: String::new(),
        };
        insert_symbols(&conn, &[sym]).unwrap();

        delete_repository(&conn, "r").unwrap();

        let found = find_symbol_by_name(&conn, "foo", None, None, 10).unwrap();
        assert!(found.is_empty());
    }
}
