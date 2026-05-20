//! Core memory retrieval (rules, preferences, task state) without a query.

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::agent_memory::sqlite::provider::SqliteMemoryProvider;
use crate::agent_memory::types::{ScoreParts, ScoredMemory, TokMemoryScope, TokMemoryType};

pub fn fetch_core(
    conn: &Connection,
    scope: &TokMemoryScope,
    max_rules: usize,
    max_preferences: usize,
    max_session_items: usize,
) -> Result<Vec<ScoredMemory>> {
    let mut out = Vec::new();
    out.extend(fetch_by_types(
        conn,
        scope,
        &[TokMemoryType::Rule],
        max_rules,
    )?);
    out.extend(fetch_by_types(
        conn,
        scope,
        &[TokMemoryType::Preference],
        max_preferences,
    )?);
    if scope.session_id.is_some() {
        out.extend(fetch_by_types(
            conn,
            scope,
            &[TokMemoryType::TaskState],
            max_session_items,
        )?);
    }
    Ok(out)
}

fn fetch_by_types(
    conn: &Connection,
    scope: &TokMemoryScope,
    types: &[TokMemoryType],
    limit: usize,
) -> Result<Vec<ScoredMemory>> {
    if types.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let type_list: Vec<String> = types.iter().map(|t| format!("'{}'", t.as_str())).collect();
    let type_in = type_list.join(", ");

    let sql = format!(
        "SELECT {}
         FROM memory_records
         WHERE user_id = ?1 AND status = 'active' AND type IN ({type_in})
         ORDER BY priority DESC, confidence DESC
         LIMIT ?2",
        "id, type, content, normalized_content,
         user_id, workspace_id, project_id, agent_id, session_id,
         source, source_event_id, status, confidence, priority,
         entities_json, tags_json, metadata_json,
         valid_from, valid_to, expires_at, embedding_id,
         created_at, updated_at, last_accessed_at"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![scope.user_id, limit as i64], |row| {
        let memory = SqliteMemoryProvider::row_to_record(row)?;
        let conf = memory.confidence;
        let pri = (memory.priority as f64) / 100.0;
        let score = (conf * 0.5 + pri * 0.5).min(1.0);
        Ok(ScoredMemory {
            memory,
            score,
            score_parts: ScoreParts {
                confidence: Some(conf),
                priority: Some(pri),
                ..Default::default()
            },
            reason: Some("core memory".to_string()),
        })
    })?;

    let mut results: Vec<ScoredMemory> = rows.filter_map(|r| r.ok()).collect();
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
    Ok(results)
}
