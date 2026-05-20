//! Build token-budgeted context packs for hook injection.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use super::format::{estimate_tokens, format_memory_block};
use crate::agent_memory::config::MemoryContextConfig;
use crate::agent_memory::provider::TokMemoryProvider;
use crate::agent_memory::types::{ScoredMemory, TokMemorySearchInput, TokMemoryType};

pub struct ContextPack {
    pub markdown: String,
    pub estimated_tokens: usize,
    pub injected_count: usize,
    pub rejected: Vec<String>,
}

pub struct ContextPackBuilder;

impl ContextPackBuilder {
    pub fn build(
        provider: &dyn TokMemoryProvider,
        scope: &crate::agent_memory::types::TokMemoryScope,
        query: &str,
        cfg: &MemoryContextConfig,
    ) -> Result<ContextPack> {
        let search_input = TokMemorySearchInput {
            scope: scope.clone(),
            query: query.to_string(),
            types: None,
            top_k: cfg.top_k,
            threshold: cfg.threshold,
            include_core: cfg.include_core,
        };

        let mut scored = provider.search(&search_input)?;
        let mut rejected = Vec::new();

        scored.retain(|s| {
            if s.score < cfg.threshold && s.reason.as_deref() != Some("core memory") {
                rejected.push(format!(
                    "{} rejected: score {:.2} below threshold",
                    s.memory.id, s.score
                ));
                return false;
            }
            true
        });

        scored = dedupe_memories(scored);
        scored = apply_type_caps(scored, cfg);
        let sections = group_by_type(&scored);

        let mut markdown = format_memory_block(&sections);
        let mut estimated = estimate_tokens(&markdown);

        while estimated > cfg.max_tokens && !markdown.is_empty() {
            if let Some(trimmed) = trim_last_item(&mut scored) {
                rejected.push(trimmed);
                let sections = group_by_type(&scored);
                markdown = format_memory_block(&sections);
                estimated = estimate_tokens(&markdown);
            } else {
                markdown.clear();
                estimated = 0;
                break;
            }
        }

        Ok(ContextPack {
            injected_count: scored.len(),
            estimated_tokens: estimated,
            markdown,
            rejected,
        })
    }
}

fn dedupe_memories(items: Vec<ScoredMemory>) -> Vec<ScoredMemory> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let key = item
            .memory
            .normalized_content
            .clone()
            .unwrap_or_else(|| item.memory.content.clone());
        if seen.insert(key) {
            out.push(item);
        }
    }
    out
}

fn apply_type_caps(items: Vec<ScoredMemory>, cfg: &MemoryContextConfig) -> Vec<ScoredMemory> {
    let mut counts: HashMap<TokMemoryType, usize> = HashMap::new();
    let mut out = Vec::new();
    for item in items {
        let limit = match item.memory.memory_type {
            TokMemoryType::Rule => cfg.max_core_rules,
            TokMemoryType::Preference => cfg.max_preferences,
            TokMemoryType::ProjectFact | TokMemoryType::Decision => cfg.max_project_facts,
            TokMemoryType::TaskState => cfg.max_session_items,
            _ => cfg.top_k,
        };
        let n = counts.entry(item.memory.memory_type).or_insert(0);
        if *n < limit {
            *n += 1;
            out.push(item);
        }
    }
    out
}

fn group_by_type(items: &[ScoredMemory]) -> Vec<(TokMemoryType, Vec<&ScoredMemory>)> {
    let order = [
        TokMemoryType::Rule,
        TokMemoryType::Preference,
        TokMemoryType::ProjectFact,
        TokMemoryType::Decision,
        TokMemoryType::TaskState,
        TokMemoryType::Lesson,
    ];
    let mut map: HashMap<TokMemoryType, Vec<&ScoredMemory>> = HashMap::new();
    for item in items {
        map.entry(item.memory.memory_type).or_default().push(item);
    }
    order
        .into_iter()
        .filter_map(|t| map.get(&t).map(|v| (t, v.clone())))
        .collect()
}

fn trim_last_item(scored: &mut Vec<ScoredMemory>) -> Option<String> {
    let last = scored.pop()?;
    Some(format!("{} dropped for token budget", last.memory.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_memory::config::MemoryContextConfig;
    use crate::agent_memory::provider::TokMemoryProvider;
    use crate::agent_memory::sqlite::SqliteMemoryProvider;
    use crate::agent_memory::types::{
        MemorySource, TokMemoryAddInput, TokMemoryScope, TokMemoryType,
    };
    use std::env;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn respects_token_budget() {
        let _guard = ENV_LOCK.lock().expect("lock");
        let dir = TempDir::new().expect("tempdir");
        env::set_var("TOK_AGENT_MEMORY_DB_PATH", dir.path().join("tok-memory.db"));

        let provider = SqliteMemoryProvider::open().expect("open");
        let scope = TokMemoryScope {
            user_id: "u".into(),
            ..Default::default()
        };
        for i in 0..15 {
            provider
                .add(&TokMemoryAddInput {
                    scope: scope.clone(),
                    content: format!("Rule {i}: always check tests before commit"),
                    memory_type: TokMemoryType::Rule,
                    source: MemorySource::User,
                    confidence: 0.9,
                    priority: 70 + i,
                    tags: vec![],
                    metadata: Default::default(),
                })
                .expect("add");
        }
        let cfg = MemoryContextConfig {
            max_tokens: 150,
            top_k: 15,
            threshold: 0.01,
            include_core: true,
            ..Default::default()
        };
        let pack = ContextPackBuilder::build(&provider, &scope, "tests", &cfg).expect("pack");
        assert!(pack.estimated_tokens <= cfg.max_tokens);
        env::remove_var("TOK_AGENT_MEMORY_DB_PATH");
    }
}
