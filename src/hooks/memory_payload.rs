//! Parse hook stdin and format memory hook stdout per agent vendor.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::core::constants::{AGENT_MEMORY_DIR, TOK_DATA_DIR};

/// Target LLM integration for hook I/O formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryHookAgent {
    Auto,
    Claude,
    Cursor,
    Gemini,
    Copilot,
    Plain,
}

/// Lifecycle point for retrieve / extract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryHookEvent {
    SessionStart,
    UserPrompt,
    Stop,
    AfterAgent,
    CachePrompt,
}

impl MemoryHookAgent {
    pub fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("auto").to_ascii_lowercase().as_str() {
            "claude" => Self::Claude,
            "cursor" => Self::Cursor,
            "gemini" => Self::Gemini,
            "copilot" => Self::Copilot,
            "plain" => Self::Plain,
            _ => Self::Auto,
        }
    }

    pub fn resolve(self) -> Self {
        match self {
            Self::Auto => match std::env::var("TOK_CLIENT")
                .ok()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "claude" => Self::Claude,
                "cursor" => Self::Cursor,
                "gemini" => Self::Gemini,
                "copilot" => Self::Copilot,
                _ => Self::Plain,
            },
            other => other,
        }
    }

    fn client_id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Gemini => "gemini",
            Self::Copilot => "copilot",
            Self::Plain | Self::Auto => "hook",
        }
    }
}

impl MemoryHookEvent {
    pub fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("session_start").to_ascii_lowercase().as_str() {
            "user_prompt" | "userpromptsubmit" | "beforeagent" | "beforesubmitprompt" => {
                Self::UserPrompt
            }
            "stop" | "agentstop" => Self::Stop,
            "afteragent" | "after_agent" => Self::AfterAgent,
            "cache_prompt" | "cacheprompt" => Self::CachePrompt,
            _ => Self::SessionStart,
        }
    }

    fn claude_event_name(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::UserPrompt => "UserPromptSubmit",
            Self::Stop => "Stop",
            Self::AfterAgent => "Stop",
            Self::CachePrompt => "UserPromptSubmit",
        }
    }
}

/// Apply scope env vars from hook JSON (session, cwd, agent).
pub fn apply_scope_from_input(input: &Value, agent: MemoryHookAgent) {
    let agent = agent.resolve();
    std::env::set_var("TOK_MEMORY_CLIENT_ID", agent.client_id());

    if let Some(sid) = pick_str(
        input,
        &[
            "session_id",
            "sessionId",
            "conversation_id",
            "conversationId",
        ],
    ) {
        std::env::set_var("TOK_MEMORY_SESSION_ID", sid);
        std::env::set_var("TOK_SESSION_ID", sid);
    }

    if let Some(cwd) = pick_str(input, &["cwd"]) {
        let _ = std::env::set_current_dir(cwd);
    }

    if let Some(agent_name) = pick_str(
        input,
        &["agent_type", "agentType", "agent_name", "agentName"],
    ) {
        std::env::set_var("TOK_MEMORY_AGENT_ID", agent_name);
    }
}

/// Query string for retrieval (prompt text or initial prompt).
pub fn retrieve_query_from_input(input: &Value, event: MemoryHookEvent) -> String {
    match event {
        MemoryHookEvent::UserPrompt => pick_str(
            input,
            &["prompt", "initial_prompt", "initialPrompt", "user_message"],
        )
        .unwrap_or_default()
        .to_string(),
        MemoryHookEvent::SessionStart => {
            pick_str(input, &["initial_prompt", "initialPrompt", "prompt"])
                .unwrap_or_default()
                .to_string()
        }
        _ => String::new(),
    }
}

/// Format retrieve JSON for the target agent.
pub fn format_retrieve_output(
    agent: MemoryHookAgent,
    event: MemoryHookEvent,
    markdown: &str,
    estimated_tokens: usize,
    injected_count: usize,
) -> Value {
    let agent = agent.resolve();
    let body = markdown;

    let pack_meta = json!({
        "estimated_tokens": estimated_tokens,
        "injected_count": injected_count,
    });

    match agent {
        MemoryHookAgent::Claude => json!({
            "hookSpecificOutput": {
                "hookEventName": event.claude_event_name(),
                "additionalContext": body,
            },
            "additional_context": body,
            "memory": pack_meta,
        }),
        MemoryHookAgent::Cursor => json!({
            "additional_context": body,
            "memory": pack_meta,
        }),
        MemoryHookAgent::Gemini => json!({
            "hookSpecificOutput": {
                "additionalContext": body,
            },
            "memory": pack_meta,
        }),
        MemoryHookAgent::Copilot => json!({
            "additionalContext": body,
            "memory": pack_meta,
        }),
        MemoryHookAgent::Plain | MemoryHookAgent::Auto => json!({
            "additional_context": body,
            "memory": pack_meta,
        }),
    }
}

/// Disabled-memory sentinel per agent.
pub fn format_empty_retrieve(agent: MemoryHookAgent) -> Value {
    format_retrieve_output(agent, MemoryHookEvent::SessionStart, "", 0, 0)
}

/// Parse user/assistant pair for extraction from hook stdin.
pub fn extract_turn_from_input(input: &Value, agent: MemoryHookAgent) -> (String, String) {
    let user = pick_str(input, &["user", "user_message", "prompt", "userMessage"])
        .unwrap_or_default()
        .to_string();

    let assistant = pick_str(
        input,
        &[
            "assistant",
            "assistant_message",
            "last_assistant_message",
            "lastAssistantMessage",
            "prompt_response",
            "promptResponse",
            "text",
        ],
    )
    .unwrap_or_default()
    .to_string();

    if !user.is_empty() || !assistant.is_empty() {
        return (user, assistant);
    }

    if let Some(path) = pick_str(
        input,
        &["transcript_path", "transcriptPath", "agent_transcript_path"],
    ) {
        if let Some((u, a)) = last_turn_from_transcript(Path::new(path)) {
            return (u, a);
        }
    }

    // Cursor: pair cached prompt with afterAgentResponse text
    if agent.resolve() == MemoryHookAgent::Cursor {
        let session = pick_str(input, &["session_id", "sessionId"]).unwrap_or("default");
        if let Some(cached) = read_prompt_cache(session) {
            let a = pick_str(input, &["text"]).unwrap_or_default().to_string();
            if !cached.is_empty() || !a.is_empty() {
                return (cached, a);
            }
        }
    }

    (String::new(), String::new())
}

/// Store the latest user prompt for Cursor turn pairing.
pub fn cache_prompt_from_input(input: &Value) -> Result<()> {
    let prompt = pick_str(input, &["prompt"]).unwrap_or_default();
    if prompt.is_empty() {
        return Ok(());
    }
    let session = pick_str(input, &["session_id", "sessionId"]).unwrap_or("default");
    write_prompt_cache(session, prompt)
}

fn pick_str<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| input.get(*k).and_then(|v| v.as_str()))
}

fn prompt_cache_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from(TOK_DATA_DIR))
        .join(AGENT_MEMORY_DIR)
        .join("prompt-cache")
}

fn prompt_cache_path(session_id: &str) -> PathBuf {
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    prompt_cache_dir().join(format!("{safe}.txt"))
}

fn write_prompt_cache(session_id: &str, prompt: &str) -> Result<()> {
    let path = prompt_cache_path(session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create prompt cache dir {}", parent.display()))?;
    }
    fs::write(&path, prompt).with_context(|| format!("write prompt cache {}", path.display()))?;
    Ok(())
}

fn read_prompt_cache(session_id: &str) -> Option<String> {
    fs::read_to_string(prompt_cache_path(session_id)).ok()
}

/// Read last user/assistant messages from a JSONL transcript (Claude / Copilot).
pub fn last_turn_from_transcript(path: &Path) -> Option<(String, String)> {
    let content = fs::read_to_string(path).ok()?;
    let mut last_user = String::new();
    let mut last_assistant = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
            match t {
                "user" => {
                    if let Some(text) = message_text(&v) {
                        last_user = text;
                    }
                }
                "assistant" => {
                    if let Some(text) = message_text(&v) {
                        last_assistant = text;
                    }
                }
                _ => {}
            }
        }

        if let Some(role) = v.get("role").and_then(|x| x.as_str()) {
            if let Some(text) = message_text(&v) {
                match role {
                    "user" => last_user = text,
                    "assistant" | "model" => last_assistant = text,
                    _ => {}
                }
            }
        }
    }

    if last_user.is_empty() && last_assistant.is_empty() {
        None
    } else {
        Some((last_user, last_assistant))
    }
}

fn message_text(v: &Value) -> Option<String> {
    if let Some(s) = v.get("content").and_then(|c| c.as_str()) {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    if let Some(msg) = v.get("message") {
        if let Some(parts) = msg.pointer("/content") {
            if let Some(s) = parts.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
            if let Some(arr) = parts.as_array() {
                let mut out = String::new();
                for part in arr {
                    if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
                        out.push_str(t);
                    }
                }
                if !out.is_empty() {
                    return Some(out);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extract_direct_fields() {
        let v = json!({"user": "hi", "assistant": "hello"});
        let (u, a) = extract_turn_from_input(&v, MemoryHookAgent::Claude);
        assert_eq!(u, "hi");
        assert_eq!(a, "hello");
    }

    #[test]
    fn extract_gemini_after_agent() {
        let v = json!({"prompt": "fix tests", "prompt_response": "done"});
        let (u, a) = extract_turn_from_input(&v, MemoryHookAgent::Gemini);
        assert_eq!(u, "fix tests");
        assert_eq!(a, "done");
    }

    #[test]
    fn transcript_jsonl_last_turn() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","message":{{"content":[{{"type":"text","text":"first"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"ok"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"user","message":{{"content":[{{"type":"text","text":"second"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"done"}}]}}}}"#
        )
        .unwrap();
        let (u, a) = last_turn_from_transcript(&path).unwrap();
        assert_eq!(u, "second");
        assert_eq!(a, "done");
    }
}
