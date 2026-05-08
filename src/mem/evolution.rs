//! Temporal evolution engine — tracks how symbols change over time.
//!
//! Populates the `episodes` table by walking git history, then provides
//! six scoring modes for surfacing the most significant changes in a window.

use std::collections::HashMap;
use std::process::Command;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

use super::symbols::ChangeType;

/// A scored symbol change from an evolution query.
#[derive(Debug, Clone, Serialize)]
pub struct EvolutionEntry {
    pub symbol_name: String,
    pub symbol_kind: String,
    pub file_path: String,
    pub change_count: usize,
    pub change_type: String,
    pub score: f64,
    pub last_commit: String,
    pub last_timestamp: String,
}

/// Module-level rollup for the "overview" scoring mode.
#[derive(Debug, Clone, Serialize)]
pub struct ModuleRollup {
    pub module_path: String,
    pub change_count: usize,
    pub symbol_count: usize,
    pub score: f64,
}

/// Timeline entry for a single symbol's history.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineEntry {
    pub commit_hash: String,
    pub timestamp: String,
    pub change_type: String,
    pub diff_summary: String,
}

/// Session anchor for cursor-based delta queries.
#[derive(Debug, Clone, Serialize)]
pub struct SessionAnchor {
    pub last_episode_id: String,
    pub last_reference_time: String,
}

/// Scoring modes for evolution queries.
#[derive(Debug, Clone, Copy)]
pub enum ScoringMode {
    /// Weighted sum of change frequency, recency, and blast radius
    Compound,
    /// Rank by downstream edge count of changed symbols
    Impact,
    /// Symbols that appeared for the first time in window
    Novel,
    /// Exponential decay weighting toward window end
    Recent,
    /// Group by added/modified/removed
    Directional,
    /// Module-level rollup
    Overview,
}

impl ScoringMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "compound" => Some(Self::Compound),
            "impact" => Some(Self::Impact),
            "novel" => Some(Self::Novel),
            "recent" => Some(Self::Recent),
            "directional" => Some(Self::Directional),
            "overview" => Some(Self::Overview),
            _ => None,
        }
    }
}

/// Populate episodes by scanning git log for changes to indexed symbols.
pub fn populate_episodes(
    conn: &Connection,
    repo_id: &str,
    repo_path: &str,
    branch: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<usize> {
    let mut git_args = vec![
        "log".to_string(),
        "--name-status".to_string(),
        "--pretty=format:%H|%aI".to_string(),
        format!("--diff-filter=ADMR"),
    ];

    if let (Some(f), Some(t)) = (from, to) {
        git_args.push(format!("--since={}", f));
        git_args.push(format!("--until={}", t));
    }

    let output = Command::new("git")
        .args(&git_args)
        .current_dir(repo_path)
        .output()
        .context("Failed to run git log for episode population")?;

    if !output.status.success() {
        anyhow::bail!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Build a set of known file paths from the index for fast lookup
    let mut known_files: HashMap<String, Vec<(String, String, String)>> = HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT id, name, file_path FROM symbols WHERE repo_id = ?1")?;
        let rows = stmt
            .query_map(params![repo_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        for (id, name, file_path) in rows {
            known_files
                .entry(file_path)
                .or_default()
                .push((id, name, String::new()));
        }
    }

    let mut episode_count = 0usize;
    let mut current_commit = String::new();
    let mut current_time = String::new();

    let mut insert_stmt = conn.prepare(
        "INSERT OR IGNORE INTO episodes (id, repo_id, symbol_id, change_type, commit_hash, timestamp, diff_summary, branch)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Commit line: hash|timestamp
        if line.contains('|') && !line.starts_with(|c: char| c.is_whitespace()) && line.len() > 40 {
            let parts: Vec<&str> = line.splitn(2, '|').collect();
            if parts.len() == 2 && parts[0].len() >= 40 {
                current_commit = parts[0].to_string();
                current_time = parts[1].to_string();
                continue;
            }
        }

        // File change line: A/M/D/R\tpath
        if current_commit.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() != 2 {
            continue;
        }

        let status = parts[0].trim();
        let file_path = parts[1].trim();

        let change_type = match status.chars().next() {
            Some('A') => ChangeType::Added,
            Some('M') => ChangeType::Modified,
            Some('D') => ChangeType::Removed,
            Some('R') => ChangeType::Modified,
            _ => continue,
        };

        // Match to known symbols in this file
        if let Some(symbols) = known_files.get(file_path) {
            for (sym_id, _sym_name, _) in symbols {
                let ep_id = super::symbols::generate_episode_id(
                    repo_id,
                    sym_id,
                    &current_commit,
                    change_type,
                );

                insert_stmt.execute(params![
                    ep_id,
                    repo_id,
                    sym_id,
                    change_type.as_str(),
                    current_commit,
                    current_time,
                    format!("{} {}", status, file_path),
                    branch,
                ])?;

                episode_count += 1;
            }
        }
    }

    // Update last_episode_id on the repository
    if episode_count > 0 {
        conn.execute(
            "UPDATE repositories SET last_episode_id = (
                SELECT id FROM episodes WHERE repo_id = ?1 ORDER BY timestamp DESC LIMIT 1
             ) WHERE repo_id = ?1",
            params![repo_id],
        )?;
    }

    Ok(episode_count)
}

/// Query evolution with a scoring mode.
pub fn query_evolution(
    conn: &Connection,
    repo_id: &str,
    from: &str,
    to: &str,
    mode: ScoringMode,
    max_symbols: usize,
) -> Result<Vec<EvolutionEntry>> {
    match mode {
        ScoringMode::Compound => query_compound(conn, repo_id, from, to, max_symbols),
        ScoringMode::Impact => query_impact(conn, repo_id, from, to, max_symbols),
        ScoringMode::Novel => query_novel(conn, repo_id, from, to, max_symbols),
        ScoringMode::Recent => query_recent(conn, repo_id, from, to, max_symbols),
        ScoringMode::Directional => query_directional(conn, repo_id, from, to, max_symbols),
        ScoringMode::Overview => {
            // Overview returns module rollups, but we adapt to EvolutionEntry
            let rollups = query_overview(conn, repo_id, from, to, max_symbols)?;
            Ok(rollups
                .into_iter()
                .map(|r| EvolutionEntry {
                    symbol_name: r.module_path.clone(),
                    symbol_kind: "Module".to_string(),
                    file_path: r.module_path,
                    change_count: r.change_count,
                    change_type: "mixed".to_string(),
                    score: r.score,
                    last_commit: String::new(),
                    last_timestamp: String::new(),
                })
                .collect())
        }
    }
}

fn query_compound(
    conn: &Connection,
    repo_id: &str,
    from: &str,
    to: &str,
    limit: usize,
) -> Result<Vec<EvolutionEntry>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.kind, s.file_path,
                COUNT(e.id) as change_count,
                MAX(e.change_type) as last_change,
                MAX(e.commit_hash) as last_commit,
                MAX(e.timestamp) as last_ts,
                (COUNT(e.id) * 10.0 +
                 COALESCE((SELECT COUNT(*) FROM edges WHERE source_id = s.id OR target_id = s.id), 0) * 2.0
                ) as score
         FROM episodes e
         JOIN symbols s ON e.symbol_id = s.id
         WHERE e.repo_id = ?1 AND e.timestamp >= ?2 AND e.timestamp <= ?3
         GROUP BY e.symbol_id
         ORDER BY score DESC
         LIMIT ?4",
    )?;

    collect_evolution_rows(&mut stmt, params![repo_id, from, to, limit])
}

fn query_impact(
    conn: &Connection,
    repo_id: &str,
    from: &str,
    to: &str,
    limit: usize,
) -> Result<Vec<EvolutionEntry>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.kind, s.file_path,
                COUNT(e.id) as change_count,
                MAX(e.change_type) as last_change,
                MAX(e.commit_hash) as last_commit,
                MAX(e.timestamp) as last_ts,
                COALESCE((SELECT COUNT(*) FROM edges WHERE source_id = s.id), 0) as score
         FROM episodes e
         JOIN symbols s ON e.symbol_id = s.id
         WHERE e.repo_id = ?1 AND e.timestamp >= ?2 AND e.timestamp <= ?3
         GROUP BY e.symbol_id
         ORDER BY score DESC
         LIMIT ?4",
    )?;

    collect_evolution_rows(&mut stmt, params![repo_id, from, to, limit])
}

fn query_novel(
    conn: &Connection,
    repo_id: &str,
    from: &str,
    to: &str,
    limit: usize,
) -> Result<Vec<EvolutionEntry>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.kind, s.file_path,
                1 as change_count,
                'added' as last_change,
                e.commit_hash as last_commit,
                e.timestamp as last_ts,
                1.0 as score
         FROM episodes e
         JOIN symbols s ON e.symbol_id = s.id
         WHERE e.repo_id = ?1 AND e.change_type = 'added'
               AND e.timestamp >= ?2 AND e.timestamp <= ?3
         ORDER BY e.timestamp DESC
         LIMIT ?4",
    )?;

    collect_evolution_rows(&mut stmt, params![repo_id, from, to, limit])
}

fn query_recent(
    conn: &Connection,
    repo_id: &str,
    from: &str,
    to: &str,
    limit: usize,
) -> Result<Vec<EvolutionEntry>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.kind, s.file_path,
                COUNT(e.id) as change_count,
                MAX(e.change_type) as last_change,
                MAX(e.commit_hash) as last_commit,
                MAX(e.timestamp) as last_ts,
                COUNT(e.id) as score
         FROM episodes e
         JOIN symbols s ON e.symbol_id = s.id
         WHERE e.repo_id = ?1 AND e.timestamp >= ?2 AND e.timestamp <= ?3
         GROUP BY e.symbol_id
         ORDER BY last_ts DESC
         LIMIT ?4",
    )?;

    collect_evolution_rows(&mut stmt, params![repo_id, from, to, limit])
}

fn query_directional(
    conn: &Connection,
    repo_id: &str,
    from: &str,
    to: &str,
    limit: usize,
) -> Result<Vec<EvolutionEntry>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.kind, s.file_path,
                COUNT(e.id) as change_count,
                e.change_type as last_change,
                MAX(e.commit_hash) as last_commit,
                MAX(e.timestamp) as last_ts,
                COUNT(e.id) as score
         FROM episodes e
         JOIN symbols s ON e.symbol_id = s.id
         WHERE e.repo_id = ?1 AND e.timestamp >= ?2 AND e.timestamp <= ?3
         GROUP BY e.symbol_id, e.change_type
         ORDER BY e.change_type, score DESC
         LIMIT ?4",
    )?;

    collect_evolution_rows(&mut stmt, params![repo_id, from, to, limit])
}

fn query_overview(
    conn: &Connection,
    repo_id: &str,
    from: &str,
    to: &str,
    limit: usize,
) -> Result<Vec<ModuleRollup>> {
    let mut stmt = conn.prepare(
        "SELECT
            CASE
                WHEN instr(s.file_path, '/') > 0
                THEN substr(s.file_path, 1, instr(s.file_path, '/') - 1)
                ELSE s.file_path
            END as module,
            COUNT(e.id) as change_count,
            COUNT(DISTINCT e.symbol_id) as sym_count,
            COUNT(e.id) * 1.0 as score
         FROM episodes e
         JOIN symbols s ON e.symbol_id = s.id
         WHERE e.repo_id = ?1 AND e.timestamp >= ?2 AND e.timestamp <= ?3
         GROUP BY module
         ORDER BY score DESC
         LIMIT ?4",
    )?;

    let rows = stmt
        .query_map(params![repo_id, from, to, limit], |row| {
            Ok(ModuleRollup {
                module_path: row.get(0)?,
                change_count: row.get(1)?,
                symbol_count: row.get(2)?,
                score: row.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to query overview")?;

    Ok(rows)
}

/// Get the full change timeline for a specific symbol.
pub fn get_timeline(conn: &Connection, symbol_id: &str) -> Result<Vec<TimelineEntry>> {
    let mut stmt = conn.prepare(
        "SELECT commit_hash, timestamp, change_type, diff_summary
         FROM episodes
         WHERE symbol_id = ?1
         ORDER BY timestamp DESC",
    )?;

    let rows = stmt
        .query_map(params![symbol_id], |row| {
            Ok(TimelineEntry {
                commit_hash: row.get(0)?,
                timestamp: row.get(1)?,
                change_type: row.get(2)?,
                diff_summary: row.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to query timeline")?;

    Ok(rows)
}

/// Get changes since a session anchor (episode ID or timestamp).
pub fn get_changes_since(
    conn: &Connection,
    repo_id: &str,
    last_episode_id: Option<&str>,
    last_reference_time: Option<&str>,
) -> Result<(Vec<EvolutionEntry>, SessionAnchor)> {
    let since_time = if let Some(ep_id) = last_episode_id {
        let ts: Option<String> = conn
            .query_row(
                "SELECT timestamp FROM episodes WHERE id = ?1",
                params![ep_id],
                |row| row.get(0),
            )
            .ok();
        ts.unwrap_or_else(|| {
            last_reference_time
                .unwrap_or("1970-01-01T00:00:00Z")
                .to_string()
        })
    } else {
        last_reference_time
            .unwrap_or("1970-01-01T00:00:00Z")
            .to_string()
    };

    let now = chrono::Utc::now().to_rfc3339();

    let entries = query_compound(conn, repo_id, &since_time, &now, 500)?;

    // Build new anchor
    let new_anchor = conn
        .query_row(
            "SELECT id, timestamp FROM episodes WHERE repo_id = ?1 ORDER BY timestamp DESC LIMIT 1",
            params![repo_id],
            |row| {
                Ok(SessionAnchor {
                    last_episode_id: row.get(0)?,
                    last_reference_time: row.get(1)?,
                })
            },
        )
        .unwrap_or(SessionAnchor {
            last_episode_id: String::new(),
            last_reference_time: now,
        });

    Ok((entries, new_anchor))
}

/// Detect which indexed symbols are affected by a set of changed files.
pub fn detect_changes(
    conn: &Connection,
    repo_id: &str,
    changed_files: &[String],
) -> Result<Vec<EvolutionEntry>> {
    let mut results = Vec::new();

    for file in changed_files {
        let mut stmt = conn.prepare(
            "SELECT name, kind, file_path FROM symbols WHERE repo_id = ?1 AND file_path = ?2",
        )?;

        let symbols = stmt
            .query_map(params![repo_id, file], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        for (name, kind, path) in symbols {
            results.push(EvolutionEntry {
                symbol_name: name,
                symbol_kind: kind,
                file_path: path,
                change_count: 1,
                change_type: "modified".to_string(),
                score: 1.0,
                last_commit: String::new(),
                last_timestamp: String::new(),
            });
        }
    }

    Ok(results)
}

fn collect_evolution_rows(
    stmt: &mut rusqlite::Statement,
    params: impl rusqlite::Params,
) -> Result<Vec<EvolutionEntry>> {
    let rows = stmt
        .query_map(params, |row| {
            Ok(EvolutionEntry {
                symbol_name: row.get(0)?,
                symbol_kind: row.get(1)?,
                file_path: row.get(2)?,
                change_count: row.get(3)?,
                change_type: row.get(4)?,
                score: row.get(7)?,
                last_commit: row.get(5)?,
                last_timestamp: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to query evolution")?;

    Ok(rows)
}
