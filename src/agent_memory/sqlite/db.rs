//! SQLite storage for agent memory (`tok-memory.db`).

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::core::constants::{AGENT_MEMORY_DB, AGENT_MEMORY_DIR, TOK_DATA_DIR};

/// Open (or create) the agent memory database.
pub fn open() -> Result<Connection> {
    let db_path = db_path()?;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create directory {:?}", parent))?;
    }

    let conn = Connection::open(&db_path)
        .with_context(|| format!("Cannot open agent memory database at {:?}", db_path))?;

    let _ = conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA busy_timeout=5000;
         PRAGMA foreign_keys=ON;",
    );

    migrate(&conn)?;
    Ok(conn)
}

pub fn db_path() -> Result<std::path::PathBuf> {
    if let Ok(p) = std::env::var("TOK_AGENT_MEMORY_DB_PATH") {
        return Ok(std::path::PathBuf::from(p));
    }

    let data_dir = dirs::data_local_dir()
        .context("Cannot determine local data directory")?
        .join(TOK_DATA_DIR)
        .join(AGENT_MEMORY_DIR);

    Ok(data_dir.join(AGENT_MEMORY_DB))
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_records (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            content TEXT NOT NULL,
            normalized_content TEXT,

            user_id TEXT NOT NULL,
            workspace_id TEXT,
            project_id TEXT,
            agent_id TEXT,
            session_id TEXT,

            source TEXT NOT NULL,
            source_event_id TEXT,

            status TEXT NOT NULL DEFAULT 'active',
            confidence REAL NOT NULL DEFAULT 0.75,
            priority INTEGER NOT NULL DEFAULT 50,

            entities_json TEXT NOT NULL DEFAULT '[]',
            tags_json TEXT NOT NULL DEFAULT '[]',
            metadata_json TEXT NOT NULL DEFAULT '{}',

            valid_from TEXT,
            valid_to TEXT,
            expires_at TEXT,

            embedding_id TEXT,

            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            last_accessed_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_memory_scope
        ON memory_records(user_id, project_id, session_id, status);

        CREATE INDEX IF NOT EXISTS idx_memory_type ON memory_records(type);
        CREATE INDEX IF NOT EXISTS idx_memory_status ON memory_records(status);
        CREATE INDEX IF NOT EXISTS idx_memory_created ON memory_records(created_at);

        CREATE TABLE IF NOT EXISTS memory_events (
            id TEXT PRIMARY KEY,
            event_type TEXT NOT NULL,
            user_id TEXT NOT NULL,
            workspace_id TEXT,
            project_id TEXT,
            agent_id TEXT,
            session_id TEXT,
            input TEXT,
            output TEXT,
            provider TEXT,
            model TEXT,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_memory_events_scope
        ON memory_events(user_id, project_id, session_id);

        CREATE VIRTUAL TABLE IF NOT EXISTS memory_records_fts USING fts5(
            content,
            normalized_content,
            memory_id UNINDEXED
        );

        CREATE TABLE IF NOT EXISTS memory_supersessions (
            old_memory_id TEXT NOT NULL,
            new_memory_id TEXT NOT NULL,
            reason TEXT,
            created_at TEXT NOT NULL,
            PRIMARY KEY(old_memory_id, new_memory_id)
        );",
    )
    .context("agent memory schema migration")?;

    Ok(())
}

/// Insert or replace FTS row for a memory record.
pub fn upsert_fts(
    conn: &Connection,
    memory_id: &str,
    content: &str,
    normalized: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM memory_records_fts WHERE memory_id = ?1",
        rusqlite::params![memory_id],
    )?;
    conn.execute(
        "INSERT INTO memory_records_fts(content, normalized_content, memory_id)
         VALUES (?1, ?2, ?3)",
        rusqlite::params![content, normalized, memory_id],
    )?;
    Ok(())
}

pub fn delete_fts(conn: &Connection, memory_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM memory_records_fts WHERE memory_id = ?1",
        rusqlite::params![memory_id],
    )?;
    Ok(())
}

pub fn new_id() -> String {
    let mut buf = [0u8; 16];
    if getrandom::fill(&mut buf).is_ok() {
        let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
        return format!("mem_{hex}");
    }
    format!(
        "mem_{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    )
}

pub fn normalize_content(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn content_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(normalize_content(content).as_bytes());
    format!("{:x}", hasher.finalize())
}
