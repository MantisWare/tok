//! Graph traversal for relationships and blast-radius impact analysis.
//!
//! Operates on the `edges` table using breadth-first search.
//! No external graph library needed -- BFS on indexed SQLite foreign keys
//! handles codebases up to ~100K symbols comfortably.

use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use super::symbols::{ImpactNode, Symbol, SymbolKind};

/// Direction for impact traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Follow edges where symbol is target (who depends on me?)
    Upstream,
    /// Follow edges where symbol is source (what do I depend on?)
    Downstream,
    /// Both directions
    Both,
}

impl Direction {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "upstream" => Some(Self::Upstream),
            "downstream" => Some(Self::Downstream),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

/// Callers of a given symbol (reverse CALLS edges).
pub fn find_callers(conn: &Connection, symbol_id: &str, limit: usize) -> Result<Vec<Symbol>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.repo_id, s.name, s.kind, s.file_path, s.line_start, s.line_end,
                s.signature, s.doc_comment, s.branch, s.indexed_at
         FROM edges e
         JOIN symbols s ON e.source_id = s.id
         WHERE e.target_id = ?1 AND e.edge_type = 'CALLS'
         LIMIT ?2",
    )?;

    let results = stmt
        .query_map(params![symbol_id, limit], symbol_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to find callers")?;

    Ok(results)
}

/// Callees of a given symbol (forward CALLS edges).
pub fn find_callees(conn: &Connection, symbol_id: &str, limit: usize) -> Result<Vec<Symbol>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.repo_id, s.name, s.kind, s.file_path, s.line_start, s.line_end,
                s.signature, s.doc_comment, s.branch, s.indexed_at
         FROM edges e
         JOIN symbols s ON e.target_id = s.id
         WHERE e.source_id = ?1 AND e.edge_type = 'CALLS'
         LIMIT ?2",
    )?;

    let results = stmt
        .query_map(params![symbol_id, limit], symbol_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to find callees")?;

    Ok(results)
}

/// Symbols that import the given symbol.
pub fn find_importers(conn: &Connection, symbol_id: &str, limit: usize) -> Result<Vec<Symbol>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.repo_id, s.name, s.kind, s.file_path, s.line_start, s.line_end,
                s.signature, s.doc_comment, s.branch, s.indexed_at
         FROM edges e
         JOIN symbols s ON e.source_id = s.id
         WHERE e.target_id = ?1 AND e.edge_type = 'IMPORTS'
         LIMIT ?2",
    )?;

    let results = stmt
        .query_map(params![symbol_id, limit], symbol_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to find importers")?;

    Ok(results)
}

/// Relationships by query type, similar to analyze_relationships.
pub fn analyze_relationships(
    conn: &Connection,
    symbol_id: &str,
    query_type: &str,
    depth: u32,
    limit: usize,
) -> Result<Vec<ImpactNode>> {
    let (edge_type_filter, direction) = match query_type {
        "find_callers" => (Some("CALLS"), Direction::Upstream),
        "find_callees" => (Some("CALLS"), Direction::Downstream),
        "class_hierarchy" | "overrides" => (Some("IMPLEMENTS"), Direction::Both),
        "imports" => (Some("IMPORTS"), Direction::Downstream),
        "exporters" => (Some("EXPORTS"), Direction::Upstream),
        "type_usages" => (Some("TYPE_REF"), Direction::Upstream),
        _ => (None, Direction::Both),
    };

    bfs_impact(conn, symbol_id, direction, depth, limit, edge_type_filter)
}

/// BFS-based blast radius / impact analysis.
///
/// Starting from `origin_id`, traverses edges in the specified direction
/// up to `max_depth` hops, collecting affected symbols.
pub fn bfs_impact(
    conn: &Connection,
    origin_id: &str,
    direction: Direction,
    max_depth: u32,
    limit: usize,
    edge_type_filter: Option<&str>,
) -> Result<Vec<ImpactNode>> {
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, u32)> = VecDeque::new();
    let mut results: Vec<ImpactNode> = Vec::new();

    visited.insert(origin_id.to_string());
    queue.push_back((origin_id.to_string(), 0));

    // Pre-build adjacency from DB for traversal
    let adjacency = build_adjacency(conn, direction, edge_type_filter)?;

    while let Some((current_id, current_depth)) = queue.pop_front() {
        if current_depth >= max_depth {
            continue;
        }

        if let Some(neighbors) = adjacency.get(&current_id) {
            for (neighbor_id, et) in neighbors {
                if visited.contains(neighbor_id) {
                    continue;
                }
                visited.insert(neighbor_id.clone());

                if let Some(sym) = get_symbol_by_id_internal(conn, neighbor_id)? {
                    results.push(ImpactNode {
                        symbol: sym,
                        depth: current_depth + 1,
                        edge_type: et.clone(),
                    });

                    if results.len() >= limit {
                        return Ok(results);
                    }

                    queue.push_back((neighbor_id.clone(), current_depth + 1));
                }
            }
        }
    }

    Ok(results)
}

/// Build an in-memory adjacency list from the edges table.
fn build_adjacency(
    conn: &Connection,
    direction: Direction,
    edge_type_filter: Option<&str>,
) -> Result<HashMap<String, Vec<(String, String)>>> {
    let sql = match edge_type_filter {
        Some(_) => "SELECT source_id, target_id, edge_type FROM edges WHERE edge_type = ?1",
        None => "SELECT source_id, target_id, edge_type FROM edges",
    };

    let mut stmt = conn.prepare(sql)?;

    let rows: Vec<(String, String, String)> = if let Some(et) = edge_type_filter {
        stmt.query_map(params![et], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };

    let mut adj: HashMap<String, Vec<(String, String)>> = HashMap::new();

    for (source, target, et) in rows {
        match direction {
            Direction::Downstream => {
                adj.entry(source).or_default().push((target, et));
            }
            Direction::Upstream => {
                adj.entry(target).or_default().push((source, et));
            }
            Direction::Both => {
                adj.entry(source.clone())
                    .or_default()
                    .push((target.clone(), et.clone()));
                adj.entry(target).or_default().push((source, et));
            }
        }
    }

    Ok(adj)
}

fn get_symbol_by_id_internal(conn: &Connection, id: &str) -> Result<Option<Symbol>> {
    let mut stmt = conn.prepare(
        "SELECT id, repo_id, name, kind, file_path, line_start, line_end,
                signature, doc_comment, branch, indexed_at
         FROM symbols WHERE id = ?1",
    )?;

    let sym = stmt
        .query_row(params![id], symbol_from_row)
        .optional()
        .context("Failed to get symbol by ID")?;

    Ok(sym)
}

use rusqlite::OptionalExtension;

fn symbol_from_row(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        let _ = conn.execute_batch("PRAGMA foreign_keys=ON;");
        // run migrations manually
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS repositories (
                repo_id TEXT PRIMARY KEY, path TEXT, branch TEXT DEFAULT 'main',
                last_indexed_at TEXT DEFAULT '', last_episode_id TEXT DEFAULT '');
             CREATE TABLE IF NOT EXISTS symbols (
                id TEXT PRIMARY KEY, repo_id TEXT, name TEXT, kind TEXT,
                file_path TEXT, line_start INTEGER DEFAULT 0, line_end INTEGER DEFAULT 0,
                signature TEXT DEFAULT '', doc_comment TEXT DEFAULT '',
                branch TEXT DEFAULT 'main', indexed_at TEXT DEFAULT '');
             CREATE TABLE IF NOT EXISTS edges (
                id INTEGER PRIMARY KEY, source_id TEXT, target_id TEXT,
                edge_type TEXT, repo_id TEXT, branch TEXT DEFAULT 'main');
             CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id);
             CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO repositories VALUES ('r', '/tmp', 'main', '', '')",
            [],
        )
        .unwrap();

        let syms = vec![
            ("a", "funcA"),
            ("b", "funcB"),
            ("c", "funcC"),
            ("d", "funcD"),
        ];
        for (id, name) in &syms {
            conn.execute(
                "INSERT INTO symbols (id, repo_id, name, kind, file_path) VALUES (?1, 'r', ?2, 'Function', 'f.rs')",
                params![id, name],
            ).unwrap();
        }

        // a -> b -> c -> d (call chain)
        for (src, tgt) in &[("a", "b"), ("b", "c"), ("c", "d")] {
            conn.execute(
                "INSERT INTO edges (source_id, target_id, edge_type, repo_id) VALUES (?1, ?2, 'CALLS', 'r')",
                params![src, tgt],
            ).unwrap();
        }

        conn
    }

    #[test]
    fn callers_and_callees() {
        let conn = setup();
        let callers = find_callers(&conn, "b", 10).unwrap();
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].name, "funcA");

        let callees = find_callees(&conn, "b", 10).unwrap();
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].name, "funcC");
    }

    #[test]
    fn bfs_downstream_impact() {
        let conn = setup();
        let impact = bfs_impact(&conn, "a", Direction::Downstream, 3, 100, Some("CALLS")).unwrap();
        assert_eq!(impact.len(), 3); // b, c, d
        assert_eq!(impact[0].depth, 1);
        assert_eq!(impact[1].depth, 2);
        assert_eq!(impact[2].depth, 3);
    }

    #[test]
    fn bfs_upstream_impact() {
        let conn = setup();
        let impact = bfs_impact(&conn, "d", Direction::Upstream, 5, 100, Some("CALLS")).unwrap();
        assert_eq!(impact.len(), 3); // c, b, a
    }

    #[test]
    fn bfs_respects_depth_limit() {
        let conn = setup();
        let impact = bfs_impact(&conn, "a", Direction::Downstream, 1, 100, Some("CALLS")).unwrap();
        assert_eq!(impact.len(), 1); // only b
    }

    #[test]
    fn bfs_respects_result_limit() {
        let conn = setup();
        let impact = bfs_impact(&conn, "a", Direction::Downstream, 5, 2, Some("CALLS")).unwrap();
        assert_eq!(impact.len(), 2);
    }
}
