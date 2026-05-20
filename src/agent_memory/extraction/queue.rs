//! Background extraction queue (in-process worker thread).

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use anyhow::Result;

use super::extractor::{candidates_to_add_input, extract_heuristic};
use super::validator::{CandidateValidator, MemoryCandidate};
use crate::agent_memory::config::AgentMemoryConfig;
use crate::agent_memory::provider::TokMemoryProvider;
use crate::agent_memory::sqlite::provider::SqliteMemoryProvider;
use crate::agent_memory::types::TokMemoryScope;

pub struct ExtractionJob {
    pub scope: TokMemoryScope,
    pub user_message: String,
    pub assistant_message: String,
}

pub struct MemoryExtractionQueue {
    sender: mpsc::Sender<ExtractionJob>,
    _handle: JoinHandle<()>,
}

impl MemoryExtractionQueue {
    pub fn spawn(config: AgentMemoryConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                if let Err(e) = process_job(&config, job) {
                    eprintln!("tok memory extraction: {e}");
                }
            }
        });
        Self {
            sender: tx,
            _handle: handle,
        }
    }

    pub fn enqueue(&self, job: ExtractionJob) {
        let _ = self.sender.send(job);
    }
}

fn process_job(config: &AgentMemoryConfig, job: ExtractionJob) -> Result<()> {
    if !config.enabled || !config.extraction.enabled {
        return Ok(());
    }

    let provider = SqliteMemoryProvider::open()?;
    let validator = CandidateValidator {
        reject_secrets: config.privacy.reject_secrets,
        min_confidence: config.extraction.min_confidence,
    };

    let candidates: Vec<MemoryCandidate> =
        extract_heuristic(&job.user_message, &job.assistant_message);

    let conn = provider.connection();
    for input in candidates_to_add_input(job.scope.clone(), candidates) {
        let candidate = MemoryCandidate {
            content: input.content.clone(),
            memory_type: input.memory_type,
            confidence: input.confidence,
            priority: input.priority,
            should_store: true,
        };
        if validator.validate(&candidate).is_err() {
            continue;
        }
        if validator.is_duplicate(conn, &job.scope.user_id, &input.content) {
            continue;
        }
        TokMemoryProvider::add(&provider, &input)?;
    }
    Ok(())
}
