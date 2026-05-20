//! Shared entry points for CLI and hooks.

use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};

use crate::core::config::Config;

use super::config::AgentMemoryConfig;
use super::context::ContextPackBuilder;
use super::extraction::{ExtractionJob, MemoryExtractionQueue};
use super::scope::resolve_scope;
use super::sqlite::SqliteMemoryProvider;
use super::types::TokMemoryScope;

static EXTRACTION_QUEUE: OnceLock<Mutex<Option<MemoryExtractionQueue>>> = OnceLock::new();

pub fn memory_config() -> AgentMemoryConfig {
    Config::load().map(|c| c.memory).unwrap_or_default()
}

pub fn is_enabled() -> bool {
    memory_config().enabled
}

pub fn open_provider() -> Result<SqliteMemoryProvider> {
    SqliteMemoryProvider::open().context("open agent memory database")
}

pub fn resolve_current_scope() -> TokMemoryScope {
    let cfg = memory_config();
    resolve_scope(&cfg.scopes)
}

pub fn build_context_pack(query: &str) -> Result<super::context::ContextPack> {
    let cfg = memory_config();
    if !cfg.enabled {
        return Ok(super::context::ContextPack {
            markdown: String::new(),
            estimated_tokens: 0,
            injected_count: 0,
            rejected: vec!["memory disabled".to_string()],
        });
    }
    let provider = open_provider()?;
    let scope = resolve_current_scope();
    ContextPackBuilder::build(&provider, &scope, query, &cfg.context)
}

pub fn enqueue_extraction(user_message: String, assistant_message: String) {
    let cfg = memory_config();
    if !cfg.enabled || !cfg.extraction.enabled {
        return;
    }
    let queue = EXTRACTION_QUEUE.get_or_init(|| Mutex::new(None));
    let mut guard = queue.lock().expect("extraction queue lock");
    if guard.is_none() {
        *guard = Some(MemoryExtractionQueue::spawn(cfg.clone()));
    }
    if let Some(q) = guard.as_ref() {
        q.enqueue(ExtractionJob {
            scope: resolve_current_scope(),
            user_message,
            assistant_message,
        });
    }
}

pub fn set_memory_enabled(enabled: bool) -> Result<()> {
    let mut config = Config::load().unwrap_or_default();
    config.memory.enabled = enabled;
    config.save()
}

pub fn set_extraction_enabled(enabled: bool) -> Result<()> {
    let mut config = Config::load().unwrap_or_default();
    config.memory.extraction.enabled = enabled;
    config.save()
}

/// Called from `tok init -g` to enable memory and create the database directory.
pub fn ensure_on_init() -> Result<()> {
    let mut config = Config::load().unwrap_or_default();
    config.memory.enabled = true;
    config.memory.extraction.enabled = true;
    config.save()?;
    let _ = open_provider()?;
    Ok(())
}
