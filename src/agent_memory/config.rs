//! `[memory]` section in config.toml.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMemoryConfig {
    pub enabled: bool,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub context: MemoryContextConfig,
    #[serde(default)]
    pub extraction: MemoryExtractionConfig,
    #[serde(default)]
    pub privacy: MemoryPrivacyConfig,
    #[serde(default)]
    pub scopes: MemoryScopesConfig,
    #[serde(default)]
    pub storage: MemoryStorageConfig,
}

fn default_mode() -> String {
    "local".to_string()
}

impl Default for AgentMemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: default_mode(),
            context: MemoryContextConfig::default(),
            extraction: MemoryExtractionConfig::default(),
            privacy: MemoryPrivacyConfig::default(),
            scopes: MemoryScopesConfig::default(),
            storage: MemoryStorageConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContextConfig {
    pub max_tokens: usize,
    pub top_k: usize,
    pub threshold: f64,
    pub include_core: bool,
    pub rerank: bool,
    pub max_core_rules: usize,
    pub max_preferences: usize,
    pub max_project_facts: usize,
    pub max_session_items: usize,
}

impl Default for MemoryContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 900,
            top_k: 8,
            threshold: 0.25,
            include_core: true,
            rerank: false,
            max_core_rules: 8,
            max_preferences: 8,
            max_project_facts: 10,
            max_session_items: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryExtractionConfig {
    pub enabled: bool,
    #[serde(default = "default_extraction_provider")]
    pub provider: String,
    #[serde(default = "default_extraction_model")]
    pub model: String,
    pub min_confidence: f64,
}

fn default_extraction_provider() -> String {
    "local".to_string()
}

fn default_extraction_model() -> String {
    "heuristic".to_string()
}

impl Default for MemoryExtractionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: default_extraction_provider(),
            model: default_extraction_model(),
            min_confidence: 0.70,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPrivacyConfig {
    pub redact_before_storage: bool,
    pub reject_secrets: bool,
    pub allow_sensitive_memory: bool,
}

impl Default for MemoryPrivacyConfig {
    fn default() -> Self {
        Self {
            redact_before_storage: true,
            reject_secrets: true,
            allow_sensitive_memory: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryScopesConfig {
    pub default_user_id: String,
    pub auto_detect_project: bool,
}

impl Default for MemoryScopesConfig {
    fn default() -> Self {
        Self {
            default_user_id: "local-user".to_string(),
            auto_detect_project: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStorageConfig {
    #[serde(default = "default_storage_provider")]
    pub provider: String,
}

fn default_storage_provider() -> String {
    "sqlite".to_string()
}

impl Default for MemoryStorageConfig {
    fn default() -> Self {
        Self {
            provider: default_storage_provider(),
        }
    }
}
