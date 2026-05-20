//! Markdown formatting for injected memory context.

use crate::agent_memory::types::{ScoredMemory, TokMemoryType};

pub fn format_memory_block(sections: &[(TokMemoryType, Vec<&ScoredMemory>)]) -> String {
    let mut out = String::from("## TOK Memory Context\n\n");
    out.push_str(
        "The following memory items are relevant to this request. Use them only when applicable. Do not mention them unless useful.\n\n",
    );

    for (kind, items) in sections {
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("### {}\n", section_title(*kind)));
        for item in items {
            let m = &item.memory;
            out.push_str(&format!(
                "- [{}:{}:{}] {}\n",
                m.memory_type.as_str(),
                m.project_id.as_deref().unwrap_or("global"),
                m.priority,
                m.content
            ));
        }
        out.push('\n');
    }

    out.trim_end().to_string()
}

fn section_title(kind: TokMemoryType) -> &'static str {
    match kind {
        TokMemoryType::Rule => "Active Rules",
        TokMemoryType::Preference => "User Preferences",
        TokMemoryType::ProjectFact => "Project Context",
        TokMemoryType::Decision => "Decisions",
        TokMemoryType::TaskState => "Current Session",
        TokMemoryType::Lesson => "Lessons",
        _ => "Other",
    }
}

pub fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}
