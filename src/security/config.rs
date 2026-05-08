//! Security configuration types for the TOK security layer.

use super::types::{SecurityAction, SensitiveEntityType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Security mode determines how aggressively TOK obfuscates.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityMode {
    /// Scan and report only, do not modify text
    Observe,
    /// Obfuscate common PII and secrets
    #[default]
    Balanced,
    /// Obfuscate aggressively (more entity types, lower threshold)
    Strict,
    /// Preserve code/stack traces/filenames, obfuscate secrets and identifiers
    Developer,
}

impl std::fmt::Display for SecurityMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Observe => write!(f, "observe"),
            Self::Balanced => write!(f, "balanced"),
            Self::Strict => write!(f, "strict"),
            Self::Developer => write!(f, "developer"),
        }
    }
}

impl std::str::FromStr for SecurityMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "observe" => Ok(Self::Observe),
            "balanced" => Ok(Self::Balanced),
            "strict" => Ok(Self::Strict),
            "developer" | "dev" => Ok(Self::Developer),
            _ => Err(format!(
                "unknown security mode '{}' (valid: observe, balanced, strict, developer)",
                s
            )),
        }
    }
}

/// Top-level security configuration (lives under [security] in config.toml).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub mode: SecurityMode,
    #[serde(default)]
    pub scan: ScanConfig,
    #[serde(default)]
    pub actions: ActionsConfig,
    #[serde(default)]
    pub restore: RestoreConfig,
    #[serde(default)]
    pub logging: SecurityLoggingConfig,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: SecurityMode::Balanced,
            scan: ScanConfig::default(),
            actions: ActionsConfig::default(),
            restore: RestoreConfig::default(),
            logging: SecurityLoggingConfig::default(),
        }
    }
}

/// Scanner configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanConfig {
    /// Enable deterministic regex-based scanning
    #[serde(default = "default_true")]
    pub deterministic: bool,
    /// Enable SLM-based semantic scanning
    #[serde(default)]
    pub slm: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            deterministic: true,
            slm: false,
        }
    }
}

/// Per-entity-type action configuration.
/// All values are either "placeholder" or "allow" -- never "block".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionsConfig {
    #[serde(default = "default_placeholder")]
    pub email: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub phone: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub person: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub company: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub client: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub internal_project: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub url: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub hostname: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub ip_address: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub money: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub api_key: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub jwt: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub private_key: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub password: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub database_url: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub credit_card: SecurityAction,
    #[serde(default = "default_placeholder")]
    pub bank_account: SecurityAction,
}

impl Default for ActionsConfig {
    fn default() -> Self {
        Self {
            email: SecurityAction::Placeholder,
            phone: SecurityAction::Placeholder,
            person: SecurityAction::Placeholder,
            company: SecurityAction::Placeholder,
            client: SecurityAction::Placeholder,
            internal_project: SecurityAction::Placeholder,
            url: SecurityAction::Placeholder,
            hostname: SecurityAction::Placeholder,
            ip_address: SecurityAction::Placeholder,
            money: SecurityAction::Placeholder,
            api_key: SecurityAction::Placeholder,
            jwt: SecurityAction::Placeholder,
            private_key: SecurityAction::Placeholder,
            password: SecurityAction::Placeholder,
            database_url: SecurityAction::Placeholder,
            credit_card: SecurityAction::Placeholder,
            bank_account: SecurityAction::Placeholder,
        }
    }
}

impl ActionsConfig {
    /// Get the configured action for a given entity type.
    pub fn action_for(&self, entity_type: SensitiveEntityType) -> SecurityAction {
        match entity_type {
            SensitiveEntityType::Email => self.email,
            SensitiveEntityType::Phone => self.phone,
            SensitiveEntityType::Person => self.person,
            SensitiveEntityType::Company => self.company,
            SensitiveEntityType::Client => self.client,
            SensitiveEntityType::InternalProject => self.internal_project,
            SensitiveEntityType::Url => self.url,
            SensitiveEntityType::Hostname => self.hostname,
            SensitiveEntityType::IpAddress => self.ip_address,
            SensitiveEntityType::Money => self.money,
            SensitiveEntityType::ApiKey => self.api_key,
            SensitiveEntityType::Jwt => self.jwt,
            SensitiveEntityType::PrivateKey => self.private_key,
            SensitiveEntityType::Password => self.password,
            SensitiveEntityType::DatabaseUrl => self.database_url,
            SensitiveEntityType::CreditCard => self.credit_card,
            SensitiveEntityType::BankAccount => self.bank_account,
            SensitiveEntityType::Medical => SecurityAction::Placeholder,
            SensitiveEntityType::Legal => SecurityAction::Placeholder,
            SensitiveEntityType::Custom => SecurityAction::Placeholder,
        }
    }
}

/// Restoration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub exact: bool,
    #[serde(default)]
    pub validate_with_slm: bool,
}

impl Default for RestoreConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            exact: true,
            validate_with_slm: false,
        }
    }
}

/// Logging rules for security operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityLoggingConfig {
    #[serde(default)]
    pub store_original_prompts: bool,
    #[serde(default = "default_true")]
    pub store_sanitized_prompts: bool,
    #[serde(default = "default_true")]
    pub redact_logs: bool,
}

impl Default for SecurityLoggingConfig {
    fn default() -> Self {
        Self {
            store_original_prompts: false,
            store_sanitized_prompts: true,
            redact_logs: true,
        }
    }
}

/// SLM (Small Language Model) runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlmConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_runtime")]
    pub runtime: String,
    #[serde(default = "default_model_path")]
    pub model_path: PathBuf,
    #[serde(default = "default_context_size")]
    pub context_size: usize,
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout_ms: u64,
    #[serde(default = "default_bind_host")]
    pub bind_host: String,
}

impl Default for SlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            runtime: "embedded-llamacpp".into(),
            model_path: PathBuf::from("./models/tok-security-slm/model.gguf"),
            context_size: 8192,
            temperature: 0.1,
            max_tokens: 1200,
            startup_timeout_ms: 30_000,
            bind_host: "127.0.0.1".into(),
        }
    }
}

/// Resolve final security enablement from config + CLI flags.
pub fn resolve_security_enabled(config: &SecurityConfig, cli_flag: Option<bool>) -> bool {
    match cli_flag {
        Some(explicit) => explicit,
        None => config.enabled,
    }
}

/// Resolve final security mode from config + CLI override.
pub fn resolve_security_mode(config: &SecurityConfig, cli_mode: Option<&str>) -> SecurityMode {
    match cli_mode {
        Some(mode_str) => mode_str.parse().unwrap_or(config.mode),
        None => config.mode,
    }
}

fn default_true() -> bool {
    true
}
fn default_placeholder() -> SecurityAction {
    SecurityAction::Placeholder
}
fn default_runtime() -> String {
    "embedded-llamacpp".into()
}
fn default_model_path() -> PathBuf {
    PathBuf::from("./models/tok-security-slm/model.gguf")
}
fn default_context_size() -> usize {
    8192
}
fn default_temperature() -> f64 {
    0.1
}
fn default_max_tokens() -> usize {
    1200
}
fn default_startup_timeout() -> u64 {
    30_000
}
fn default_bind_host() -> String {
    "127.0.0.1".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_config_defaults() {
        let config = SecurityConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.mode, SecurityMode::Balanced);
        assert!(config.scan.deterministic);
        assert!(!config.scan.slm);
    }

    #[test]
    fn test_security_mode_parsing() {
        assert_eq!(
            "observe".parse::<SecurityMode>().unwrap(),
            SecurityMode::Observe
        );
        assert_eq!(
            "balanced".parse::<SecurityMode>().unwrap(),
            SecurityMode::Balanced
        );
        assert_eq!(
            "strict".parse::<SecurityMode>().unwrap(),
            SecurityMode::Strict
        );
        assert_eq!(
            "developer".parse::<SecurityMode>().unwrap(),
            SecurityMode::Developer
        );
        assert_eq!(
            "dev".parse::<SecurityMode>().unwrap(),
            SecurityMode::Developer
        );
        assert!("invalid".parse::<SecurityMode>().is_err());
    }

    #[test]
    fn test_resolve_security_enabled() {
        let config = SecurityConfig::default();
        assert!(!resolve_security_enabled(&config, None));
        assert!(resolve_security_enabled(&config, Some(true)));
        assert!(!resolve_security_enabled(&config, Some(false)));
    }

    #[test]
    fn test_actions_config_lookup() {
        let actions = ActionsConfig::default();
        assert_eq!(
            actions.action_for(SensitiveEntityType::Email),
            SecurityAction::Placeholder
        );
        assert_eq!(
            actions.action_for(SensitiveEntityType::ApiKey),
            SecurityAction::Placeholder
        );
    }

    #[test]
    fn test_toml_deserialization() {
        let toml_str = r#"
enabled = true
mode = "strict"

[scan]
deterministic = true
slm = false

[actions]
email = "placeholder"
api_key = "allow"

[restore]
enabled = true
exact = true

[logging]
store_original_prompts = false
redact_logs = true
"#;
        let config: SecurityConfig = toml::from_str(toml_str).expect("valid toml");
        assert!(config.enabled);
        assert_eq!(config.mode, SecurityMode::Strict);
        assert_eq!(config.actions.api_key, SecurityAction::Allow);
        assert_eq!(config.actions.email, SecurityAction::Placeholder);
    }
}
