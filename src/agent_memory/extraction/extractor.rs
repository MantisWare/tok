//! Heuristic memory extraction from user/assistant turns (SLM optional later).

use crate::agent_memory::types::{MemorySource, TokMemoryType};

use super::validator::MemoryCandidate;

lazy_static::lazy_static! {
    static ref REMEMBER_RE: regex::Regex = regex::Regex::new(
        r"(?i)(from now on|remember(?:\s+that)?|always|never|don't ever)"
    ).expect("valid remember regex");
}

pub fn extract_heuristic(user: &str, assistant: &str) -> Vec<MemoryCandidate> {
    let mut out = Vec::new();
    let combined = format!("{user}\n{assistant}");

    if REMEMBER_RE.is_match(user) || REMEMBER_RE.is_match(&combined) {
        let content = if user.len() > assistant.len() {
            user.trim().to_string()
        } else {
            format!("{}\n{}", user.trim(), assistant.trim())
        };
        let is_rule = user.to_lowercase().contains("always")
            || user.to_lowercase().contains("never")
            || user.to_lowercase().contains("from now on");
        out.push(MemoryCandidate {
            content,
            memory_type: if is_rule {
                TokMemoryType::Rule
            } else {
                TokMemoryType::Preference
            },
            confidence: 0.85,
            priority: if is_rule { 90 } else { 75 },
            should_store: true,
        });
    }

    out
}

pub fn candidates_to_add_input(
    scope: crate::agent_memory::types::TokMemoryScope,
    candidates: Vec<MemoryCandidate>,
) -> Vec<crate::agent_memory::types::TokMemoryAddInput> {
    candidates
        .into_iter()
        .map(|c| crate::agent_memory::types::TokMemoryAddInput {
            scope: scope.clone(),
            content: c.content,
            memory_type: c.memory_type,
            source: MemorySource::Inferred,
            confidence: c.confidence,
            priority: c.priority,
            tags: vec!["auto-extracted".to_string()],
            metadata: std::collections::HashMap::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_extracts_remember_phrase() {
        let cands = extract_heuristic(
            "From now on, always give me one Cursor-ready markdown file",
            "Sure, I will do that.",
        );
        assert!(!cands.is_empty());
        assert_eq!(cands[0].memory_type, TokMemoryType::Rule);
    }
}
