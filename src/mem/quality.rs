//! Code quality analysis: dead code detection, centrality, bridges,
//! community detection, and cyclomatic complexity estimation.

use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

// Symbol and SymbolKind types used indirectly via SQL queries

/// A symbol with its centrality score.
#[derive(Debug, Clone, Serialize)]
pub struct CentralSymbol {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub score: f64,
    pub in_degree: usize,
    pub out_degree: usize,
}

/// A bridge/bottleneck symbol connecting otherwise separate subgraphs.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeSymbol {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub communities_connected: usize,
}

/// A community of tightly connected symbols (Louvain-like clustering).
#[derive(Debug, Clone, Serialize)]
pub struct Community {
    pub id: usize,
    pub members: Vec<String>,
    pub member_count: usize,
    pub label: String,
}

/// A symbol identified as likely dead code (zero inbound references).
#[derive(Debug, Clone, Serialize)]
pub struct DeadCodeEntry {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub line_start: u32,
}

/// Rough cyclomatic complexity estimate for a function.
#[derive(Debug, Clone, Serialize)]
pub struct ComplexityEntry {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub complexity: u32,
    pub line_start: u32,
}

/// Find symbols with highest in+out degree (centrality proxy).
pub fn find_central_symbols(
    conn: &Connection,
    repo_id: &str,
    limit: usize,
) -> Result<Vec<CentralSymbol>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.kind, s.file_path,
                (SELECT COUNT(*) FROM edges WHERE target_id = s.id) as in_deg,
                (SELECT COUNT(*) FROM edges WHERE source_id = s.id) as out_deg
         FROM symbols s
         WHERE s.repo_id = ?1
           AND ((SELECT COUNT(*) FROM edges WHERE target_id = s.id) +
                (SELECT COUNT(*) FROM edges WHERE source_id = s.id)) > 0
         ORDER BY (in_deg + out_deg) DESC
         LIMIT ?2",
    )?;

    let rows = stmt
        .query_map(params![repo_id, limit], |row| {
            let in_d: usize = row.get(3)?;
            let out_d: usize = row.get(4)?;
            Ok(CentralSymbol {
                name: row.get(0)?,
                kind: row.get(1)?,
                file_path: row.get(2)?,
                score: (in_d + out_d) as f64,
                in_degree: in_d,
                out_degree: out_d,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to find central symbols")?;

    Ok(rows)
}

/// Find symbols with zero inbound edges (not called/imported by anything).
pub fn find_dead_code(
    conn: &Connection,
    repo_id: &str,
    include_tests: bool,
    limit: usize,
    kinds: Option<&[&str]>,
) -> Result<Vec<DeadCodeEntry>> {
    let base = "SELECT s.name, s.kind, s.file_path, s.line_start
                FROM symbols s
                LEFT JOIN edges e ON e.target_id = s.id
                WHERE s.repo_id = ?1
                  AND e.id IS NULL
                  AND s.kind NOT IN ('Import', 'Module', 'Export')";

    let mut conditions = String::new();
    if !include_tests {
        conditions.push_str(" AND s.file_path NOT LIKE '%test%' AND s.name NOT LIKE 'test_%'");
    }
    if let Some(ks) = kinds {
        let k_list: Vec<String> = ks.iter().map(|k| format!("'{}'", k)).collect();
        conditions.push_str(&format!(" AND s.kind IN ({})", k_list.join(",")));
    }

    let sql = format!(
        "{}{} ORDER BY s.file_path, s.line_start LIMIT ?2",
        base, conditions
    );

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt
        .query_map(params![repo_id, limit], |row| {
            Ok(DeadCodeEntry {
                name: row.get(0)?,
                kind: row.get(1)?,
                file_path: row.get(2)?,
                line_start: row.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to find dead code")?;

    Ok(rows)
}

/// Estimate cyclomatic complexity for functions using regex heuristics.
/// Counts branching keywords (if, else, match, for, while, &&, ||) in the source.
pub fn estimate_complexity(
    conn: &Connection,
    repo_id: &str,
    limit: usize,
    min_complexity: u32,
) -> Result<Vec<ComplexityEntry>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.kind, s.file_path, s.line_start, s.line_end
         FROM symbols s
         WHERE s.repo_id = ?1 AND s.kind IN ('Function', 'Method')
         ORDER BY s.file_path, s.line_start",
    )?;

    let functions: Vec<(String, String, String, u32, u32)> = stmt
        .query_map(params![repo_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Get repo path for reading source files
    let repo_path: Option<String> = conn
        .query_row(
            "SELECT path FROM repositories WHERE repo_id = ?1",
            params![repo_id],
            |row| row.get(0),
        )
        .ok();

    let repo_root = match repo_path {
        Some(p) => p,
        None => return Ok(Vec::new()),
    };

    let mut results = Vec::new();
    let mut file_cache: HashMap<String, Vec<String>> = HashMap::new();

    for (name, kind, file_path, line_start, line_end) in &functions {
        let lines = file_cache.entry(file_path.clone()).or_insert_with(|| {
            let full_path = format!("{}/{}", repo_root, file_path);
            std::fs::read_to_string(&full_path)
                .unwrap_or_default()
                .lines()
                .map(|l| l.to_string())
                .collect()
        });

        let start = (*line_start as usize).saturating_sub(1);
        // Use a heuristic: scan from line_start to line_start + 100 (or end of file)
        let end = if *line_end > *line_start {
            (*line_end as usize).min(lines.len())
        } else {
            (start + 100).min(lines.len())
        };

        let mut complexity: u32 = 1; // base path
        for line in lines.get(start..end).unwrap_or(&[]) {
            let trimmed = line.trim();
            // Count branching keywords
            if trimmed.starts_with("if ")
                || trimmed.starts_with("} else if")
                || trimmed.starts_with("else if")
                || trimmed.starts_with("elif ")
            {
                complexity += 1;
            }
            if trimmed.starts_with("for ") || trimmed.starts_with("while ") {
                complexity += 1;
            }
            if trimmed.contains("match ") || trimmed.contains("switch ") {
                complexity += 1;
            }
            if trimmed.contains("&&") {
                complexity += trimmed.matches("&&").count() as u32;
            }
            if trimmed.contains("||") {
                complexity += trimmed.matches("||").count() as u32;
            }
            // Match arms in Rust/similar
            if trimmed.contains("=>") && !trimmed.starts_with("//") {
                complexity += 1;
            }
        }

        if complexity >= min_complexity {
            results.push(ComplexityEntry {
                name: name.clone(),
                kind: kind.clone(),
                file_path: file_path.clone(),
                complexity,
                line_start: *line_start,
            });
        }
    }

    results.sort_by(|a, b| b.complexity.cmp(&a.complexity));
    results.truncate(limit);

    Ok(results)
}

/// Simple community detection using connected components on the edges graph.
/// (A lightweight alternative to Louvain for small to medium codebases.)
pub fn detect_communities(
    conn: &Connection,
    repo_id: &str,
    min_size: usize,
    limit: usize,
) -> Result<Vec<Community>> {
    // Build adjacency from edges
    let mut stmt = conn.prepare("SELECT source_id, target_id FROM edges WHERE repo_id = ?1")?;

    let edges: Vec<(String, String)> = stmt
        .query_map(params![repo_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
    for (src, tgt) in &edges {
        adj.entry(src.clone()).or_default().insert(tgt.clone());
        adj.entry(tgt.clone()).or_default().insert(src.clone());
    }

    // BFS-based connected components
    let mut visited: HashSet<String> = HashSet::new();
    let mut communities = Vec::new();
    let mut community_id = 0usize;

    for node in adj.keys() {
        if visited.contains(node) {
            continue;
        }

        let mut component = Vec::new();
        let mut queue = VecDeque::new();
        queue.push_back(node.clone());
        visited.insert(node.clone());

        while let Some(current) = queue.pop_front() {
            component.push(current.clone());
            if let Some(neighbors) = adj.get(&current) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        visited.insert(neighbor.clone());
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }

        if component.len() >= min_size {
            // Resolve symbol names for the label
            let member_names: Vec<String> = component
                .iter()
                .take(5)
                .filter_map(|id| {
                    conn.query_row(
                        "SELECT name FROM symbols WHERE id = ?1",
                        params![id],
                        |row| row.get(0),
                    )
                    .ok()
                })
                .collect();

            let label = if member_names.is_empty() {
                format!("Community {}", community_id)
            } else {
                member_names.join(", ")
            };

            communities.push(Community {
                id: community_id,
                member_count: component.len(),
                members: member_names,
                label,
            });

            community_id += 1;
        }
    }

    communities.sort_by(|a, b| b.member_count.cmp(&a.member_count));
    communities.truncate(limit);

    Ok(communities)
}

/// Find bridge symbols that connect otherwise separate communities.
pub fn find_bridge_symbols(
    conn: &Connection,
    repo_id: &str,
    limit: usize,
) -> Result<Vec<BridgeSymbol>> {
    // First detect communities
    let communities = detect_communities(conn, repo_id, 2, 200)?;

    // Build node-to-community map
    let mut node_community: HashMap<String, usize> = HashMap::new();
    for (idx, comm) in communities.iter().enumerate() {
        for member in &comm.members {
            // We need symbol IDs, but communities currently store names
            // Query IDs for these names
            let ids: Vec<String> = conn
                .prepare("SELECT id FROM symbols WHERE name = ?1 AND repo_id = ?2")?
                .query_map(params![member, repo_id], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;

            for id in ids {
                node_community.insert(id, idx);
            }
        }
    }

    // Find symbols that have edges spanning different communities
    let mut stmt = conn.prepare(
        "SELECT e.source_id, e.target_id, s.name, s.kind, s.file_path
         FROM edges e
         JOIN symbols s ON e.source_id = s.id
         WHERE e.repo_id = ?1",
    )?;

    let mut bridge_counts: HashMap<String, (String, String, String, HashSet<usize>)> =
        HashMap::new();

    let rows: Vec<(String, String, String, String, String)> = stmt
        .query_map(params![repo_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    for (source_id, target_id, name, kind, file_path) in rows {
        let src_comm = node_community.get(&source_id).copied();
        let tgt_comm = node_community.get(&target_id).copied();

        if let (Some(sc), Some(tc)) = (src_comm, tgt_comm) {
            if sc != tc {
                let entry = bridge_counts.entry(source_id.clone()).or_insert_with(|| {
                    (
                        name.clone(),
                        kind.clone(),
                        file_path.clone(),
                        HashSet::new(),
                    )
                });
                entry.3.insert(sc);
                entry.3.insert(tc);
            }
        }
    }

    let mut results: Vec<BridgeSymbol> = bridge_counts
        .into_values()
        .map(|(name, kind, file_path, comms)| BridgeSymbol {
            name,
            kind,
            file_path,
            communities_connected: comms.len(),
        })
        .collect();

    results.sort_by(|a, b| b.communities_connected.cmp(&a.communities_connected));
    results.truncate(limit);

    Ok(results)
}
