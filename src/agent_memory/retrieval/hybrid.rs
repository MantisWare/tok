//! Hybrid retrieval: core + keyword + recency/confidence fusion.

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::Connection;

use super::core::fetch_core;
use super::fts::fts_search;
use crate::agent_memory::types::{ScoreParts, ScoredMemory, TokMemorySearchInput};

const W_SEMANTIC: f64 = 0.45;
const W_KEYWORD: f64 = 0.20;
const W_ENTITY: f64 = 0.15;
const W_RECENCY: f64 = 0.10;
const W_CONFIDENCE: f64 = 0.05;
const W_PRIORITY: f64 = 0.05;

pub fn search(conn: &Connection, input: &TokMemorySearchInput) -> Result<Vec<ScoredMemory>> {
    let mut by_id: HashMap<String, ScoredMemory> = HashMap::new();

    if input.include_core {
        let core = fetch_core(conn, &input.scope, 8, 8, 10)?;
        for item in core {
            by_id.entry(item.memory.id.clone()).or_insert(item);
        }
    }

    if !input.query.trim().is_empty() {
        let kw = fts_search(
            conn,
            &input.query,
            &input.scope,
            input.types.as_deref(),
            input.top_k * 2,
        )?;
        for mut item in kw {
            let recency = recency_score(&item.memory.created_at);
            let conf = item.memory.confidence;
            let pri = (item.memory.priority as f64) / 100.0;
            let keyword = item.score_parts.keyword.unwrap_or(item.score);
            let semantic = 0.0_f64;
            let entity = 0.0_f64;

            let fused = semantic * W_SEMANTIC
                + keyword * W_KEYWORD
                + entity * W_ENTITY
                + recency * W_RECENCY
                + conf * W_CONFIDENCE
                + pri * W_PRIORITY;

            item.score = fused;
            item.score_parts = ScoreParts {
                semantic: Some(semantic),
                keyword: Some(keyword),
                entity: Some(entity),
                recency: Some(recency),
                confidence: Some(conf),
                priority: Some(pri),
            };

            if fused >= input.threshold {
                by_id
                    .entry(item.memory.id.clone())
                    .and_modify(|existing| {
                        if item.score > existing.score {
                            *existing = item.clone();
                        }
                    })
                    .or_insert(item);
            }
        }
    } else {
        for item in by_id.values_mut() {
            if item.score < input.threshold {
                continue;
            }
        }
    }

    let mut results: Vec<ScoredMemory> = by_id.into_values().collect();
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(input.top_k);
    Ok(results)
}

fn recency_score(created_at: &str) -> f64 {
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return 0.5;
    };
    let age_days = (chrono::Utc::now() - parsed.with_timezone(&chrono::Utc)).num_days();
    if age_days <= 1 {
        1.0
    } else if age_days <= 7 {
        0.8
    } else if age_days <= 30 {
        0.5
    } else {
        0.3
    }
}
