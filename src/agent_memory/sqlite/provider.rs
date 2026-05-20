//! SQLite-backed agent memory provider.

use anyhow::{bail, Result};
use rusqlite::{params, Connection};

use super::db::{self, delete_fts, new_id, normalize_content, upsert_fts};
use crate::agent_memory::provider::{MemoryStatusCounts, TokMemoryProvider};
use crate::agent_memory::types::{
    DeleteMode, MemorySource, MemoryStatus, ScoredMemory, TokMemoryAddInput, TokMemoryAddResult,
    TokMemoryListInput, TokMemoryRecord, TokMemoryScope, TokMemorySearchInput, TokMemoryType,
};

pub struct SqliteMemoryProvider {
    conn: Connection,
}

impl SqliteMemoryProvider {
    pub fn open() -> Result<Self> {
        Ok(Self { conn: db::open()? })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<TokMemoryRecord> {
        let type_str: String = row.get(1)?;
        Ok(TokMemoryRecord {
            id: row.get(0)?,
            memory_type: TokMemoryType::from_str(&type_str).unwrap_or(TokMemoryType::ProjectFact),
            content: row.get(2)?,
            normalized_content: row.get(3)?,
            user_id: row.get(4)?,
            workspace_id: row.get(5)?,
            project_id: row.get(6)?,
            agent_id: row.get(7)?,
            session_id: row.get(8)?,
            source: MemorySource::from_str(&row.get::<_, String>(9)?).unwrap_or(MemorySource::User),
            source_event_id: row.get(10)?,
            status: MemoryStatus::from_str(&row.get::<_, String>(11)?)
                .unwrap_or(MemoryStatus::Active),
            confidence: row.get(12)?,
            priority: row.get(13)?,
            entities_json: row.get(14)?,
            tags_json: row.get(15)?,
            metadata_json: row.get(16)?,
            valid_from: row.get(17)?,
            valid_to: row.get(18)?,
            expires_at: row.get(19)?,
            embedding_id: row.get(20)?,
            created_at: row.get(21)?,
            updated_at: row.get(22)?,
            last_accessed_at: row.get(23)?,
        })
    }

    const SELECT_COLS: &'static str = "id, type, content, normalized_content,
        user_id, workspace_id, project_id, agent_id, session_id,
        source, source_event_id, status, confidence, priority,
        entities_json, tags_json, metadata_json,
        valid_from, valid_to, expires_at, embedding_id,
        created_at, updated_at, last_accessed_at";

    fn scope_where(scope: &TokMemoryScope) -> (String, Vec<String>) {
        let mut clauses = vec!["user_id = ?".to_string()];
        let mut extras = Vec::new();

        if let Some(ref p) = scope.project_id {
            clauses.push("(project_id = ? OR project_id IS NULL)".to_string());
            extras.push(p.clone());
        }
        if let Some(ref s) = scope.session_id {
            clauses.push("(session_id = ? OR session_id IS NULL)".to_string());
            extras.push(s.clone());
        }
        if let Some(ref a) = scope.agent_id {
            clauses.push("(agent_id = ? OR agent_id IS NULL)".to_string());
            extras.push(a.clone());
        }

        (clauses.join(" AND "), extras)
    }

    fn log_event(
        conn: &Connection,
        event_type: &str,
        scope: &TokMemoryScope,
        memory_id: Option<&str>,
    ) -> Result<()> {
        let id = new_id().replace("mem_", "evt_");
        let now = chrono::Utc::now().to_rfc3339();
        let meta = memory_id
            .map(|m| format!(r#"{{"memory_id":"{m}"}}"#))
            .unwrap_or_else(|| "{}".to_string());
        conn.execute(
            "INSERT INTO memory_events
             (id, event_type, user_id, workspace_id, project_id, agent_id, session_id,
              metadata_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                event_type,
                scope.user_id,
                scope.workspace_id,
                scope.project_id,
                scope.agent_id,
                scope.session_id,
                meta,
                now,
            ],
        )?;
        Ok(())
    }
}

impl TokMemoryProvider for SqliteMemoryProvider {
    fn add(&self, input: &TokMemoryAddInput) -> Result<TokMemoryAddResult> {
        let normalized = normalize_content(&input.content);
        let hash = db::content_hash(&input.content);

        let dup: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM memory_records
             WHERE user_id = ?1 AND status = 'active'
               AND COALESCE(normalized_content, '') = ?2
             LIMIT 1",
                params![input.scope.user_id, normalized],
                |r| r.get(0),
            )
            .optional()?;

        if let Some(existing) = dup {
            return Ok(TokMemoryAddResult {
                id: existing,
                created: false,
            });
        }

        let _ = hash; // used by validator externally

        let id = new_id();
        let now = chrono::Utc::now().to_rfc3339();
        let tags_json = serde_json::to_string(&input.tags)?;
        let metadata_json = serde_json::to_string(&input.metadata)?;

        self.conn.execute(
            "INSERT INTO memory_records (
                id, type, content, normalized_content,
                user_id, workspace_id, project_id, agent_id, session_id,
                source, status, confidence, priority,
                entities_json, tags_json, metadata_json,
                created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'active', ?11, ?12,
                '[]', ?13, ?14, ?15, ?15
            )",
            params![
                id,
                input.memory_type.as_str(),
                input.content,
                normalized,
                input.scope.user_id,
                input.scope.workspace_id,
                input.scope.project_id,
                input.scope.agent_id,
                input.scope.session_id,
                input.source.as_str(),
                input.confidence,
                input.priority,
                tags_json,
                metadata_json,
                now,
            ],
        )?;

        upsert_fts(&self.conn, &id, &input.content, &normalized)?;
        Self::log_event(&self.conn, "memory_added", &input.scope, Some(&id))?;

        Ok(TokMemoryAddResult { id, created: true })
    }

    fn search(&self, input: &TokMemorySearchInput) -> Result<Vec<ScoredMemory>> {
        crate::agent_memory::retrieval::hybrid::search(&self.conn, input)
    }

    fn get(&self, id: &str) -> Result<Option<TokMemoryRecord>> {
        let sql = format!(
            "SELECT {} FROM memory_records WHERE id = ?1",
            Self::SELECT_COLS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let result = stmt
            .query_row(params![id], Self::row_to_record)
            .optional()?;
        Ok(result)
    }

    fn list(&self, input: &TokMemoryListInput) -> Result<Vec<TokMemoryRecord>> {
        let status = input.status.unwrap_or(MemoryStatus::Active).as_str();
        let (scope_sql, scope_params) = Self::scope_where(&input.scope);

        let mut sql = format!(
            "SELECT {} FROM memory_records WHERE {} AND status = ?",
            Self::SELECT_COLS,
            scope_sql
        );
        if input.memory_type.is_some() {
            sql.push_str(" AND type = ?");
        }
        sql.push_str(" ORDER BY priority DESC, confidence DESC, created_at DESC LIMIT ?");

        let _stmt = self.conn.prepare(&sql)?;
        let mut records = Vec::new();
        let type_filter = input.memory_type.map(|t| t.as_str().to_string());

        let base = format!(
            "SELECT {} FROM memory_records
             WHERE user_id = ?1 AND status = ?2",
            Self::SELECT_COLS
        );

        if let (Some(proj), Some(typ)) = (&input.scope.project_id, &type_filter) {
            let sql = format!(
                "{base} AND (project_id = ?3 OR project_id IS NULL) AND type = ?4
                 ORDER BY priority DESC LIMIT ?5"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(
                params![input.scope.user_id, status, proj, typ, input.limit as i64],
                Self::row_to_record,
            )?;
            for row in rows {
                records.push(row?);
            }
        } else if let Some(proj) = &input.scope.project_id {
            let sql = format!(
                "{base} AND (project_id = ?3 OR project_id IS NULL)
                 ORDER BY priority DESC LIMIT ?4"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(
                params![input.scope.user_id, status, proj, input.limit as i64],
                Self::row_to_record,
            )?;
            for row in rows {
                records.push(row?);
            }
        } else if let Some(typ) = &type_filter {
            let sql = format!("{base} AND type = ?3 ORDER BY priority DESC LIMIT ?4");
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(
                params![input.scope.user_id, status, typ, input.limit as i64],
                Self::row_to_record,
            )?;
            for row in rows {
                records.push(row?);
            }
        } else {
            let sql = format!("{base} ORDER BY priority DESC LIMIT ?3");
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(
                params![input.scope.user_id, status, input.limit as i64],
                Self::row_to_record,
            )?;
            for row in rows {
                records.push(row?);
            }
        }

        let _ = scope_params;
        Ok(records)
    }

    fn archive(&self, id: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let n = self.conn.execute(
            "UPDATE memory_records SET status = 'archived', updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        if n == 0 {
            bail!("memory not found: {id}");
        }
        Ok(())
    }

    fn forget(&self, id: &str) -> Result<()> {
        delete_fts(&self.conn, id)?;
        let n = self
            .conn
            .execute("DELETE FROM memory_records WHERE id = ?1", params![id])?;
        if n == 0 {
            bail!("memory not found: {id}");
        }
        Ok(())
    }

    fn delete_all(&self, scope: &TokMemoryScope, mode: DeleteMode) -> Result<u64> {
        let n = match mode {
            DeleteMode::Session => {
                let Some(ref sid) = scope.session_id else {
                    bail!("session_id required for session clear");
                };
                let ids: Vec<String> = self
                    .conn
                    .prepare(
                        "SELECT id FROM memory_records WHERE user_id = ?1 AND session_id = ?2",
                    )?
                    .query_map(params![scope.user_id, sid], |r| r.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();
                for id in &ids {
                    delete_fts(&self.conn, id)?;
                }
                self.conn.execute(
                    "DELETE FROM memory_records WHERE user_id = ?1 AND session_id = ?2",
                    params![scope.user_id, sid],
                )?
            }
            DeleteMode::Project => {
                let Some(ref pid) = scope.project_id else {
                    bail!("project_id required for project clear");
                };
                let ids: Vec<String> = self
                    .conn
                    .prepare(
                        "SELECT id FROM memory_records WHERE user_id = ?1 AND project_id = ?2",
                    )?
                    .query_map(params![scope.user_id, pid], |r| r.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();
                for id in &ids {
                    delete_fts(&self.conn, id)?;
                }
                self.conn.execute(
                    "DELETE FROM memory_records WHERE user_id = ?1 AND project_id = ?2",
                    params![scope.user_id, pid],
                )?
            }
            DeleteMode::User => self.conn.execute(
                "DELETE FROM memory_records WHERE user_id = ?1",
                params![scope.user_id],
            )?,
            DeleteMode::AllScoped => {
                let ids: Vec<String> = self
                    .conn
                    .prepare("SELECT id FROM memory_records WHERE user_id = ?1")?
                    .query_map(params![scope.user_id], |r| r.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();
                for id in &ids {
                    delete_fts(&self.conn, id)?;
                }
                self.conn.execute(
                    "DELETE FROM memory_records WHERE user_id = ?1",
                    params![scope.user_id],
                )?
            }
        };
        Ok(n as u64)
    }

    fn status_counts(&self) -> Result<MemoryStatusCounts> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM memory_records", [], |r| r.get(0))?;
        let active: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM memory_records WHERE status = 'active'",
            [],
            |r| r.get(0),
        )?;
        let archived: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM memory_records WHERE status = 'archived'",
            [],
            |r| r.get(0),
        )?;
        let rejected: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM memory_records WHERE status = 'rejected'",
            [],
            |r| r.get(0),
        )?;
        Ok(MemoryStatusCounts {
            total: total as u64,
            active: active as u64,
            archived: archived as u64,
            rejected: rejected as u64,
        })
    }
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_memory::types::{
        MemorySource, MemoryStatus, TokMemoryAddInput, TokMemoryListInput, TokMemorySearchInput,
        TokMemoryType,
    };
    use std::env;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let dir = TempDir::new().expect("tempdir");
        env::set_var("TOK_AGENT_MEMORY_DB_PATH", dir.path().join("tok-memory.db"));
        f();
        env::remove_var("TOK_AGENT_MEMORY_DB_PATH");
    }

    #[test]
    fn add_and_search_respects_project_scope() {
        with_temp_db(|| {
            let provider = SqliteMemoryProvider::open().expect("open");
            let scope_a = TokMemoryScope {
                user_id: "test-user".into(),
                project_id: Some("project-a".into()),
                ..Default::default()
            };
            let scope_b = TokMemoryScope {
                user_id: "test-user".into(),
                project_id: Some("project-b".into()),
                ..Default::default()
            };
            provider
                .add(&TokMemoryAddInput {
                    scope: scope_a,
                    content: "Always use Cursor-ready markdown".into(),
                    memory_type: TokMemoryType::Rule,
                    source: MemorySource::User,
                    confidence: 0.95,
                    priority: 90,
                    tags: vec![],
                    metadata: Default::default(),
                })
                .expect("add");
            let results = provider
                .search(&TokMemorySearchInput {
                    scope: scope_b,
                    query: "Cursor".into(),
                    types: None,
                    top_k: 10,
                    threshold: 0.1,
                    include_core: false,
                })
                .expect("search");
            assert!(results.is_empty());
        });
    }

    #[test]
    fn archived_excluded_from_active_list() {
        with_temp_db(|| {
            let provider = SqliteMemoryProvider::open().expect("open");
            let scope = TokMemoryScope {
                user_id: "u".into(),
                ..Default::default()
            };
            let id = provider
                .add(&TokMemoryAddInput {
                    scope: scope.clone(),
                    content: "formatting rule".into(),
                    memory_type: TokMemoryType::Rule,
                    source: MemorySource::User,
                    confidence: 0.9,
                    priority: 50,
                    tags: vec![],
                    metadata: Default::default(),
                })
                .expect("add")
                .id;
            provider.archive(&id).expect("archive");
            let active = provider
                .list(&TokMemoryListInput {
                    scope,
                    memory_type: None,
                    status: Some(MemoryStatus::Active),
                    limit: 50,
                })
                .expect("list");
            assert!(active.iter().all(|r| r.id != id));
        });
    }
}
