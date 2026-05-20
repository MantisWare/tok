//! FTS5 keyword search for agent memories.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::agent_memory::sqlite::provider::SqliteMemoryProvider;
use crate::agent_memory::types::{ScoreParts, ScoredMemory, TokMemoryScope, TokMemoryType};

pub fn sanitize_fts_query(query: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    for word in query.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '*')
            .collect();
        if !clean.is_empty() {
            tokens.push(clean);
        }
    }
    tokens.join(" OR ")
}

pub fn fts_search(
    conn: &Connection,
    query: &str,
    scope: &TokMemoryScope,
    types: Option<&[TokMemoryType]>,
    limit: usize,
) -> Result<Vec<ScoredMemory>> {
    let fts_query = sanitize_fts_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    let sql = "SELECT m.id, m.type, m.content, m.normalized_content,
                      m.user_id, m.workspace_id, m.project_id, m.agent_id, m.session_id,
                      m.source, m.source_event_id, m.status, m.confidence, m.priority,
                      m.entities_json, m.tags_json, m.metadata_json,
                      m.valid_from, m.valid_to, m.expires_at, m.embedding_id,
                      m.created_at, m.updated_at, m.last_accessed_at,
                      bm25(memory_records_fts) AS rank
               FROM memory_records_fts
               JOIN memory_records m ON memory_records_fts.memory_id = m.id
               WHERE memory_records_fts MATCH ?1
                 AND m.user_id = ?2
                 AND m.status = 'active'";

    let mut stmt = conn.prepare(sql).context("prepare memory FTS")?;
    let rows = stmt.query_map(params![fts_query, scope.user_id], |row| {
        let rank: f64 = row.get(24)?;
        let keyword_score = (-rank).clamp(0.0, 1.0);
        let memory = SqliteMemoryProvider::row_to_record(row)?;
        Ok(ScoredMemory {
            memory,
            score: keyword_score,
            score_parts: ScoreParts {
                keyword: Some(keyword_score),
                ..Default::default()
            },
            reason: Some("keyword match".to_string()),
        })
    })?;

    let mut results: Vec<ScoredMemory> = rows.filter_map(|r| r.ok()).collect();

    if let Some(type_filter) = types {
        results.retain(|s| type_filter.contains(&s.memory.memory_type));
    }
    if let Some(ref pid) = scope.project_id {
        results.retain(|s| {
            s.memory.project_id.as_deref() == Some(pid.as_str()) || s.memory.project_id.is_none()
        });
    }
    if let Some(ref sid) = scope.session_id {
        results.retain(|s| {
            s.memory.session_id.as_deref() == Some(sid.as_str()) || s.memory.session_id.is_none()
        });
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(limit);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_operators() {
        assert_eq!(sanitize_fts_query("hello world"), "hello OR world");
        assert_eq!(sanitize_fts_query(""), "");
    }
}
