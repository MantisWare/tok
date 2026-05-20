//! Validate memory candidates before storage.

use crate::agent_memory::privacy::{check_content, PrivacyRejectReason};
use crate::agent_memory::sqlite::db;

#[derive(Debug, Clone)]
pub struct MemoryCandidate {
    pub content: String,
    pub memory_type: crate::agent_memory::types::TokMemoryType,
    pub confidence: f64,
    pub priority: i32,
    pub should_store: bool,
}

pub struct CandidateValidator {
    pub reject_secrets: bool,
    pub min_confidence: f64,
}

impl CandidateValidator {
    pub fn validate(&self, candidate: &MemoryCandidate) -> Result<(), &'static str> {
        if !candidate.should_store {
            return Err("extractor marked should_store false");
        }
        if candidate.confidence < self.min_confidence {
            return Err("below min confidence");
        }
        if let Some(reason) = check_content(&candidate.content, self.reject_secrets) {
            return Err(match reason {
                PrivacyRejectReason::Secret => "secret detected",
                PrivacyRejectReason::PromptInjection => "prompt injection pattern",
            });
        }
        Ok(())
    }

    pub fn is_duplicate(&self, conn: &rusqlite::Connection, user_id: &str, content: &str) -> bool {
        let normalized = db::normalize_content(content);
        conn.query_row(
            "SELECT 1 FROM memory_records
             WHERE user_id = ?1 AND status = 'active'
               AND normalized_content = ?2 LIMIT 1",
            rusqlite::params![user_id, normalized],
            |_| Ok(()),
        )
        .is_ok()
    }
}
