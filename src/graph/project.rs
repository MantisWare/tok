//! Projection of the tree-sitter graph into the existing SQLite tables.
//!
//! `memory.db` is not retired by the graph. Eighteen `tok mem` subcommands
//! query `symbols` and `edges` directly, and `episodes.symbol_id` references
//! `symbols.id`. So indexing dual-writes: the rich graph goes to
//! `.tok/graph/graph.json`, and the same nodes and edges are projected here in
//! the vocabulary those commands already speak.
//!
//! Two constraints make this projection lossy on purpose, and both protect
//! behaviour that exists today:
//!
//! - **`symbols.id` keeps its `sha256(repo_id:file_path:name:kind)[:16]`
//!   formula.** Rewriting it to the graph's readable ids would orphan every
//!   `episodes` row. The readable id is carried alongside in `graph_id`.
//! - **`EXTENDS` projects as `IMPLEMENTS`**, because `tok mem relations
//!   --query-type class_hierarchy` filters on `IMPLEMENTS` and would otherwise
//!   stop reporting inheritance it reports today.
//!
//! The hash formula also means two same-named symbols of the same kind in one
//! file collapse to a single row — the collision the graph's `~N` ordinals fix.
//! The graph keeps them distinct; only this projection merges them, and the
//! merge is reported rather than hidden.

use std::collections::HashMap;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::graph::types::GraphV1;
use crate::mem::symbols::{generate_symbol_id, Edge, EdgeType, Symbol, SymbolKind};

/// What the projection wrote.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProjectionStats {
    pub symbols_inserted: usize,
    pub edges_inserted: usize,
    /// Graph nodes that collapsed onto an existing row because the frozen id
    /// formula cannot tell them apart.
    pub id_collisions: usize,
    /// Edges dropped because an endpoint has no SQLite row — an edge into a
    /// third-party package, or from a `File` node.
    pub edges_unmapped: usize,
}

/// Project a graph into `symbols` and `edges` for one repository.
///
/// Runs inside the caller's transaction so the dual-write is atomic with the
/// rest of the index.
pub fn project(
    conn: &Connection,
    graph: &GraphV1,
    repo_id: &str,
    branch: &str,
) -> Result<ProjectionStats> {
    let mut stats = ProjectionStats::default();
    let indexed_at = chrono::Utc::now().to_rfc3339();

    // graph node id -> SQLite symbol id
    let mut id_map: HashMap<&str, String> = HashMap::new();
    let mut symbols: Vec<Symbol> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();

    for node in &graph.nodes {
        // `File` nodes anchor CONTAINS in the graph but have no row here; the
        // existing schema has no file symbol kind.
        let Some(kind_name) = node.kind.to_symbol_kind() else {
            continue;
        };
        let Some(kind) = symbol_kind(kind_name) else {
            continue;
        };

        let sql_id = generate_symbol_id(repo_id, &node.file, &node.name, kind);
        id_map.insert(node.id.as_str(), sql_id.clone());

        if seen.insert(sql_id.clone(), ()).is_some() {
            stats.id_collisions += 1;
            continue;
        }

        symbols.push(Symbol {
            id: sql_id,
            repo_id: repo_id.to_string(),
            name: node.name.clone(),
            kind,
            file_path: node.file.clone(),
            line_start: node.span.start,
            line_end: node.span.end,
            signature: node.signature.clone().unwrap_or_default(),
            doc_comment: node.doc.clone().unwrap_or_default(),
            branch: branch.to_string(),
            indexed_at: indexed_at.clone(),
        });
    }

    stats.symbols_inserted = crate::mem::db::insert_symbols(conn, &symbols)?;
    write_graph_ids(conn, graph, &id_map)?;

    let mut edges: Vec<Edge> = Vec::new();
    for edge in &graph.edges {
        let (Some(source), Some(target)) =
            (id_map.get(edge.from.as_str()), id_map.get(edge.to.as_str()))
        else {
            stats.edges_unmapped += 1;
            continue;
        };

        // A collision can map two graph nodes onto one row, turning a real edge
        // into a self-loop that would distort centrality.
        if source == target {
            continue;
        }

        let Some(edge_type) = EdgeType::from_str(edge.kind.to_sqlite_edge_type()) else {
            stats.edges_unmapped += 1;
            continue;
        };

        edges.push(Edge {
            source_id: source.clone(),
            target_id: target.clone(),
            edge_type,
            repo_id: repo_id.to_string(),
            branch: branch.to_string(),
        });
    }

    edges.sort_by(|a, b| {
        a.source_id
            .cmp(&b.source_id)
            .then_with(|| a.target_id.cmp(&b.target_id))
            .then_with(|| a.edge_type.as_str().cmp(b.edge_type.as_str()))
    });
    edges.dedup_by(|a, b| {
        a.source_id == b.source_id
            && a.target_id == b.target_id
            && a.edge_type.as_str() == b.edge_type.as_str()
    });

    stats.edges_inserted = crate::mem::db::insert_edges(conn, &edges)?;

    Ok(stats)
}

/// Record the graph's readable id on each projected row.
///
/// This is the bridge between the two id schemes: SQLite keeps its hash so
/// `episodes` stays valid, and `graph_id` lets a row be traced back to the
/// node it came from.
fn write_graph_ids(
    conn: &Connection,
    graph: &GraphV1,
    id_map: &HashMap<&str, String>,
) -> Result<()> {
    let mut stmt = conn
        .prepare("UPDATE symbols SET graph_id = ?1 WHERE id = ?2")
        .context("Failed to prepare graph_id update")?;

    for node in &graph.nodes {
        if let Some(sql_id) = id_map.get(node.id.as_str()) {
            stmt.execute(params![node.id, sql_id])?;
        }
    }

    Ok(())
}

/// Remove every row belonging to files that no longer exist.
///
/// The defect this fixes: `--incremental` previously left symbols and edges for
/// deleted files in place forever, so `search` and `dead-code` kept reporting
/// code that had been removed.
pub fn remove_files(conn: &Connection, repo_id: &str, paths: &[String]) -> Result<usize> {
    if paths.is_empty() {
        return Ok(0);
    }

    let mut removed = 0usize;
    let mut find = conn.prepare("SELECT id FROM symbols WHERE repo_id = ?1 AND file_path = ?2")?;
    let mut drop_edges =
        conn.prepare("DELETE FROM edges WHERE source_id = ?1 OR target_id = ?1")?;
    let mut drop_symbol = conn.prepare("DELETE FROM symbols WHERE id = ?1")?;

    for path in paths {
        let ids: Vec<String> = find
            .query_map(params![repo_id, path], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        for id in ids {
            // Edges first: a dangling edge is worse than a missing one, and
            // `filter_valid_edges` only guards inserts, not deletes.
            drop_edges.execute(params![id])?;
            drop_symbol.execute(params![id])?;
            removed += 1;
        }
    }

    Ok(removed)
}

/// Map a `SymbolKind` name back to the enum. The two vocabularies are kept in
/// sync by `NodeKind::to_symbol_kind`, so an unknown name means a bug there.
fn symbol_kind(name: &str) -> Option<SymbolKind> {
    Some(match name {
        "Function" => SymbolKind::Function,
        "Method" => SymbolKind::Method,
        "Class" => SymbolKind::Class,
        "Struct" => SymbolKind::Struct,
        "Enum" => SymbolKind::Enum,
        "Trait" => SymbolKind::Trait,
        "Interface" => SymbolKind::Interface,
        "Type" => SymbolKind::Type,
        "Const" => SymbolKind::Const,
        "Static" => SymbolKind::Static,
        "Module" => SymbolKind::Module,
        "Import" => SymbolKind::Import,
        "Export" => SymbolKind::Export,
        _ => return None,
    })
}

/// Every `NodeKind` that projects must map to a real `SymbolKind`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{EdgeKind, EdgeV1, NodeKind, NodeV1, Span};

    fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        crate::mem::db::migrate(&conn).expect("migrate");
        crate::mem::db::upsert_repository(&conn, "r", "/tmp", "main").expect("repo");
        conn
    }

    fn node(id: &str, name: &str, kind: NodeKind, file: &str) -> NodeV1 {
        NodeV1::new(
            id.to_string(),
            kind,
            name.to_string(),
            file.to_string(),
            Span::new(1, 4),
        )
    }

    fn graph_with(nodes: Vec<NodeV1>, edges: Vec<EdgeV1>) -> GraphV1 {
        let mut g = GraphV1::new("r", "test");
        g.nodes = nodes;
        g.edges = edges;
        g.normalize();
        g
    }

    #[test]
    fn every_projectable_node_kind_maps_to_a_symbol_kind() {
        for kind in [
            NodeKind::Function,
            NodeKind::Method,
            NodeKind::Class,
            NodeKind::Interface,
            NodeKind::Struct,
            NodeKind::Enum,
            NodeKind::Trait,
            NodeKind::Type,
            NodeKind::Constant,
            NodeKind::Variable,
            NodeKind::Module,
            NodeKind::Import,
        ] {
            let name = kind.to_symbol_kind().expect("projectable");
            assert!(
                symbol_kind(name).is_some(),
                "{name} has no SymbolKind variant"
            );
        }
    }

    #[test]
    fn projects_symbols_into_sqlite() {
        let conn = db();
        let graph = graph_with(
            vec![node("a.rs::f", "f", NodeKind::Function, "a.rs")],
            vec![],
        );

        let stats = project(&conn, &graph, "r", "main").expect("project");
        assert_eq!(stats.symbols_inserted, 1);

        let name: String = conn
            .query_row("SELECT name FROM symbols WHERE repo_id = 'r'", [], |r| {
                r.get(0)
            })
            .expect("row");
        assert_eq!(name, "f");
    }

    #[test]
    fn symbol_ids_keep_the_frozen_formula() {
        let conn = db();
        let graph = graph_with(
            vec![node("a.rs::f", "f", NodeKind::Function, "a.rs")],
            vec![],
        );
        project(&conn, &graph, "r", "main").expect("project");

        let expected = generate_symbol_id("r", "a.rs", "f", SymbolKind::Function);
        let id: String = conn
            .query_row("SELECT id FROM symbols WHERE name = 'f'", [], |r| r.get(0))
            .expect("row");

        assert_eq!(id, expected, "episodes.symbol_id depends on this");
    }

    #[test]
    fn the_readable_graph_id_is_recorded_alongside() {
        let conn = db();
        let graph = graph_with(
            vec![node("a.rs::f", "f", NodeKind::Function, "a.rs")],
            vec![],
        );
        project(&conn, &graph, "r", "main").expect("project");

        let graph_id: String = conn
            .query_row("SELECT graph_id FROM symbols WHERE name = 'f'", [], |r| {
                r.get(0)
            })
            .expect("row");
        assert_eq!(graph_id, "a.rs::f");
    }

    #[test]
    fn projects_call_edges() {
        let conn = db();
        let graph = graph_with(
            vec![
                node("a.rs::f", "f", NodeKind::Function, "a.rs"),
                node("a.rs::g", "g", NodeKind::Function, "a.rs"),
            ],
            vec![EdgeV1::new("a.rs::f", "a.rs::g", EdgeKind::Calls)],
        );

        let stats = project(&conn, &graph, "r", "main").expect("project");
        assert_eq!(stats.edges_inserted, 1);

        let kind: String = conn
            .query_row("SELECT edge_type FROM edges", [], |r| r.get(0))
            .expect("row");
        assert_eq!(kind, "CALLS");
    }

    #[test]
    fn extends_projects_as_implements() {
        let conn = db();
        let graph = graph_with(
            vec![
                node("a.ts::Cache", "Cache", NodeKind::Class, "a.ts"),
                node("a.ts::Base", "Base", NodeKind::Class, "a.ts"),
            ],
            vec![EdgeV1::new("a.ts::Cache", "a.ts::Base", EdgeKind::Extends)],
        );
        project(&conn, &graph, "r", "main").expect("project");

        let kind: String = conn
            .query_row("SELECT edge_type FROM edges", [], |r| r.get(0))
            .expect("row");
        assert_eq!(kind, "IMPLEMENTS", "class_hierarchy queries depend on this");
    }

    #[test]
    fn file_nodes_are_not_projected() {
        let conn = db();
        let graph = graph_with(vec![node("a.rs", "a.rs", NodeKind::File, "a.rs")], vec![]);

        let stats = project(&conn, &graph, "r", "main").expect("project");
        assert_eq!(stats.symbols_inserted, 0);
    }

    #[test]
    fn edges_touching_unprojectable_nodes_are_counted_not_inserted() {
        let conn = db();
        let graph = graph_with(
            vec![
                node("a.rs", "a.rs", NodeKind::File, "a.rs"),
                node("a.rs::f", "f", NodeKind::Function, "a.rs"),
            ],
            vec![EdgeV1::new("a.rs", "a.rs::f", EdgeKind::Contains)],
        );

        let stats = project(&conn, &graph, "r", "main").expect("project");
        assert_eq!(stats.edges_inserted, 0);
        assert_eq!(stats.edges_unmapped, 1);
    }

    /// The regex indexer's collision, now visible instead of silent: two `get`
    /// declarations in one file share a hash id.
    #[test]
    fn id_collisions_are_counted_rather_than_hidden() {
        let conn = db();
        let graph = graph_with(
            vec![
                node("a.rs::get", "get", NodeKind::Function, "a.rs"),
                node("a.rs::get~2", "get", NodeKind::Function, "a.rs"),
            ],
            vec![],
        );

        let stats = project(&conn, &graph, "r", "main").expect("project");
        assert_eq!(stats.symbols_inserted, 1);
        assert_eq!(stats.id_collisions, 1);
    }

    #[test]
    fn a_collision_does_not_create_a_self_edge() {
        let conn = db();
        let graph = graph_with(
            vec![
                node("a.rs::get", "get", NodeKind::Function, "a.rs"),
                node("a.rs::get~2", "get", NodeKind::Function, "a.rs"),
            ],
            vec![EdgeV1::new("a.rs::get", "a.rs::get~2", EdgeKind::Calls)],
        );

        project(&conn, &graph, "r", "main").expect("project");
        let edges: i64 = conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .expect("count");
        assert_eq!(edges, 0, "a self-loop would distort centrality");
    }

    #[test]
    fn duplicate_edges_are_written_once() {
        let conn = db();
        let mut graph = graph_with(
            vec![
                node("a.rs::f", "f", NodeKind::Function, "a.rs"),
                node("a.rs::g", "g", NodeKind::Function, "a.rs"),
            ],
            vec![],
        );
        // Extends and Implements both project to IMPLEMENTS.
        graph.edges = vec![
            EdgeV1::new("a.rs::f", "a.rs::g", EdgeKind::Extends),
            EdgeV1::new("a.rs::f", "a.rs::g", EdgeKind::Implements),
        ];

        let stats = project(&conn, &graph, "r", "main").expect("project");
        assert_eq!(stats.edges_inserted, 1, "projection collapsed them");
    }

    #[test]
    fn removing_a_file_drops_its_symbols_and_edges() {
        let conn = db();
        let graph = graph_with(
            vec![
                node("a.rs::f", "f", NodeKind::Function, "a.rs"),
                node("b.rs::g", "g", NodeKind::Function, "b.rs"),
            ],
            vec![EdgeV1::new("a.rs::f", "b.rs::g", EdgeKind::Calls)],
        );
        project(&conn, &graph, "r", "main").expect("project");

        let removed = remove_files(&conn, "r", &["b.rs".to_string()]).expect("remove");
        assert_eq!(removed, 1);

        let symbols: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
            .expect("count");
        assert_eq!(symbols, 1);

        let edges: i64 = conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .expect("count");
        assert_eq!(edges, 0, "edges into a deleted file must go too");
    }

    #[test]
    fn removing_nothing_is_a_no_op() {
        let conn = db();
        assert_eq!(remove_files(&conn, "r", &[]).expect("remove"), 0);
    }

    #[test]
    fn reprojecting_the_same_graph_is_idempotent() {
        let conn = db();
        let graph = graph_with(
            vec![
                node("a.rs::f", "f", NodeKind::Function, "a.rs"),
                node("a.rs::g", "g", NodeKind::Function, "a.rs"),
            ],
            vec![EdgeV1::new("a.rs::f", "a.rs::g", EdgeKind::Calls)],
        );

        project(&conn, &graph, "r", "main").expect("first");
        let symbols_after_first: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
            .expect("count");

        project(&conn, &graph, "r", "main").expect("second");
        let symbols_after_second: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
            .expect("count");

        assert_eq!(symbols_after_first, symbols_after_second);
    }
}
