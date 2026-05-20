//! Agent memory record types (conversation memory — not structural `tok mem`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokMemoryType {
    Identity,
    Preference,
    Rule,
    ProjectFact,
    Decision,
    Lesson,
    TaskState,
    Workflow,
    ToolUsage,
    CredentialRef,
    ConversationSummary,
    Temporary,
}

impl TokMemoryType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Preference => "preference",
            Self::Rule => "rule",
            Self::ProjectFact => "project_fact",
            Self::Decision => "decision",
            Self::Lesson => "lesson",
            Self::TaskState => "task_state",
            Self::Workflow => "workflow",
            Self::ToolUsage => "tool_usage",
            Self::CredentialRef => "credential_ref",
            Self::ConversationSummary => "conversation_summary",
            Self::Temporary => "temporary",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "identity" => Some(Self::Identity),
            "preference" => Some(Self::Preference),
            "rule" => Some(Self::Rule),
            "project_fact" => Some(Self::ProjectFact),
            "decision" => Some(Self::Decision),
            "lesson" => Some(Self::Lesson),
            "task_state" => Some(Self::TaskState),
            "workflow" => Some(Self::Workflow),
            "tool_usage" => Some(Self::ToolUsage),
            "credential_ref" => Some(Self::CredentialRef),
            "conversation_summary" => Some(Self::ConversationSummary),
            "temporary" => Some(Self::Temporary),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    User,
    Assistant,
    Tool,
    System,
    Inferred,
}

impl MemorySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::System => "system",
            Self::Inferred => "inferred",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "tool" => Some(Self::Tool),
            "system" => Some(Self::System),
            "inferred" => Some(Self::Inferred),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    Archived,
    Rejected,
    Superseded,
    Expired,
}

impl MemoryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
            Self::Expired => "expired",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "archived" => Some(Self::Archived),
            "rejected" => Some(Self::Rejected),
            "superseded" => Some(Self::Superseded),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokMemoryScope {
    pub user_id: String,
    pub workspace_id: Option<String>,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokMemoryRecord {
    pub id: String,
    pub memory_type: TokMemoryType,
    pub content: String,
    pub normalized_content: Option<String>,
    pub user_id: String,
    pub workspace_id: Option<String>,
    pub project_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub source: MemorySource,
    pub source_event_id: Option<String>,
    pub status: MemoryStatus,
    pub confidence: f64,
    pub priority: i32,
    pub entities_json: String,
    pub tags_json: String,
    pub metadata_json: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub expires_at: Option<String>,
    pub embedding_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoreParts {
    pub semantic: Option<f64>,
    pub keyword: Option<f64>,
    pub entity: Option<f64>,
    pub recency: Option<f64>,
    pub confidence: Option<f64>,
    pub priority: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct ScoredMemory {
    pub memory: TokMemoryRecord,
    pub score: f64,
    pub score_parts: ScoreParts,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokMemoryAddInput {
    pub scope: TokMemoryScope,
    pub content: String,
    pub memory_type: TokMemoryType,
    pub source: MemorySource,
    pub confidence: f64,
    pub priority: i32,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct TokMemoryAddResult {
    pub id: String,
    pub created: bool,
}

#[derive(Debug, Clone)]
pub struct TokMemorySearchInput {
    pub scope: TokMemoryScope,
    pub query: String,
    pub types: Option<Vec<TokMemoryType>>,
    pub top_k: usize,
    pub threshold: f64,
    pub include_core: bool,
}

#[derive(Debug, Clone)]
pub struct TokMemoryListInput {
    pub scope: TokMemoryScope,
    pub memory_type: Option<TokMemoryType>,
    pub status: Option<MemoryStatus>,
    pub limit: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum DeleteMode {
    Session,
    Project,
    #[allow(dead_code)]
    User,
    #[allow(dead_code)]
    AllScoped,
}
