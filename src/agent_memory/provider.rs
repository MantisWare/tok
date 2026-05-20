//! Pluggable memory provider trait.

use anyhow::Result;

use super::types::{
    DeleteMode, ScoredMemory, TokMemoryAddInput, TokMemoryAddResult, TokMemoryListInput,
    TokMemoryRecord, TokMemoryScope, TokMemorySearchInput,
};

pub trait TokMemoryProvider: Send {
    fn add(&self, input: &TokMemoryAddInput) -> Result<TokMemoryAddResult>;
    fn search(&self, input: &TokMemorySearchInput) -> Result<Vec<ScoredMemory>>;
    fn get(&self, id: &str) -> Result<Option<TokMemoryRecord>>;
    fn list(&self, input: &TokMemoryListInput) -> Result<Vec<TokMemoryRecord>>;
    fn archive(&self, id: &str) -> Result<()>;
    fn forget(&self, id: &str) -> Result<()>;
    fn delete_all(&self, scope: &TokMemoryScope, mode: DeleteMode) -> Result<u64>;
    fn status_counts(&self) -> Result<MemoryStatusCounts>;
}

#[derive(Debug, Clone, Default)]
pub struct MemoryStatusCounts {
    pub total: u64,
    pub active: u64,
    pub archived: u64,
    pub rejected: u64,
}
