//! The `[graph]` section of `~/.config/tok/config.toml`.
//!
//! Everything here is off unless the user turns it on. The graph itself is
//! local, deterministic, and free; the enrichment layer sends source code to a
//! third party and bills for it, so it cannot be something a user discovers
//! after the fact.

use serde::{Deserialize, Serialize};

/// Where the API key is read from, per provider.
///
/// Keys are taken from the environment, never from the config file: a key in
/// `config.toml` gets committed, shared in a screenshot, or synced to a dotfile
/// repository sooner or later.
pub const OPENAI_KEY_ENV: &str = "OPENAI_API_KEY";
pub const ANTHROPIC_KEY_ENV: &str = "ANTHROPIC_API_KEY";

/// Per-run overrides, for pointing one invocation at a different model or
/// endpoint without editing the config file.
pub const PROVIDER_ENV: &str = "TOK_GRAPH_PROVIDER";
pub const MODEL_ENV: &str = "TOK_GRAPH_MODEL";
pub const BASE_URL_ENV: &str = "TOK_GRAPH_BASE_URL";
pub const API_KEY_ENV: &str = "TOK_GRAPH_API_KEY";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphConfig {
    #[serde(default)]
    pub llm: LlmConfig,
}

/// Which provider serves `--deep`, and the limits it runs under.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Off unless explicitly enabled. `--deep` without this reports how to
    /// turn it on rather than silently sending code somewhere.
    #[serde(default)]
    pub enabled: bool,

    /// `openai` for any OpenAI-compatible endpoint, or `anthropic`.
    #[serde(default = "default_provider")]
    pub provider: String,

    #[serde(default = "default_model")]
    pub model: String,

    /// Overrides the provider's default endpoint. This is what points the
    /// OpenAI-compatible client at a local server, Azure, or a proxy.
    #[serde(default)]
    pub base_url: Option<String>,

    /// Environment variable holding the API key, when it is not the provider's
    /// usual one.
    #[serde(default)]
    pub api_key_env: Option<String>,

    #[serde(default = "default_max_files")]
    pub max_files: usize,

    #[serde(default = "default_max_symbols")]
    pub max_symbols: usize,

    /// How much of a file is sent. Whole files would blow past both the
    /// context window and the budget on anything real.
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,

    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_provider() -> String {
    "openai".to_string()
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

/// Deliberately small. Enrichment costs real money per call, and a first run
/// that quietly bills for a thousand files is not a good introduction.
fn default_max_files() -> usize {
    50
}

fn default_max_symbols() -> usize {
    100
}

fn default_max_chars() -> usize {
    8_000
}

fn default_timeout() -> u64 {
    30
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_provider(),
            model: default_model(),
            base_url: None,
            api_key_env: None,
            max_files: default_max_files(),
            max_symbols: default_max_symbols(),
            max_chars: default_max_chars(),
            timeout_secs: default_timeout(),
        }
    }
}

impl LlmConfig {
    /// The environment variable this configuration reads its key from.
    pub fn key_env(&self) -> &str {
        if let Some(name) = &self.api_key_env {
            return name;
        }

        match self.provider.as_str() {
            "anthropic" => ANTHROPIC_KEY_ENV,
            _ => OPENAI_KEY_ENV,
        }
    }

    /// Apply the `TOK_GRAPH_*` overrides on top of the config file.
    ///
    /// `enabled` is deliberately not overridable: turning on paid network
    /// calls should take an edit to a file the user owns, not a variable that
    /// could be inherited from a parent shell or a CI job.
    pub fn with_env_overrides(&self) -> Self {
        let mut settings = self.clone();

        if let Ok(provider) = std::env::var(PROVIDER_ENV) {
            settings.provider = provider;
        }
        if let Ok(model) = std::env::var(MODEL_ENV) {
            settings.model = model;
        }
        if let Ok(base_url) = std::env::var(BASE_URL_ENV) {
            settings.base_url = Some(base_url);
        }
        // Names the variable rather than copying the key, so the secret stays
        // in the environment and out of every struct that gets logged.
        if std::env::var_os(API_KEY_ENV).is_some() {
            settings.api_key_env = Some(API_KEY_ENV.to_string());
        }

        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Enrichment sends source code to a third party and bills for it, so an
    /// unconfigured TOK must never do it.
    #[test]
    fn enrichment_is_off_by_default() {
        assert!(!LlmConfig::default().enabled);
    }

    #[test]
    fn an_absent_section_deserializes_to_the_defaults() {
        let config: GraphConfig = toml::from_str("").expect("parse");

        assert!(!config.llm.enabled);
        assert_eq!(config.llm.provider, "openai");
    }

    #[test]
    fn a_partial_section_keeps_the_other_defaults() {
        let config: GraphConfig = toml::from_str(
            r#"
            [llm]
            enabled = true
            model = "gpt-4o"
            "#,
        )
        .expect("parse");

        assert!(config.llm.enabled);
        assert_eq!(config.llm.model, "gpt-4o");
        assert_eq!(config.llm.max_files, default_max_files());
        assert_eq!(config.llm.timeout_secs, default_timeout());
    }

    #[test]
    fn each_provider_has_its_own_default_key_variable() {
        let openai = LlmConfig::default();
        assert_eq!(openai.key_env(), OPENAI_KEY_ENV);

        let anthropic = LlmConfig {
            provider: "anthropic".to_string(),
            ..LlmConfig::default()
        };
        assert_eq!(anthropic.key_env(), ANTHROPIC_KEY_ENV);
    }

    #[test]
    fn an_explicit_key_variable_wins() {
        let config = LlmConfig {
            api_key_env: Some("MY_KEY".to_string()),
            ..LlmConfig::default()
        };

        assert_eq!(config.key_env(), "MY_KEY");
    }

    /// Environment variables are process-global, so these run one at a time.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_env(vars: &[(&str, &str)], body: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        for (name, value) in vars {
            std::env::set_var(name, value);
        }

        body();

        for (name, _) in vars {
            std::env::remove_var(name);
        }
    }

    #[test]
    fn env_overrides_replace_the_configured_provider_and_model() {
        with_env(
            &[
                (PROVIDER_ENV, "anthropic"),
                (MODEL_ENV, "claude-sonnet-4"),
                (BASE_URL_ENV, "http://localhost:8080/v1"),
            ],
            || {
                let settings = LlmConfig::default().with_env_overrides();

                assert_eq!(settings.provider, "anthropic");
                assert_eq!(settings.model, "claude-sonnet-4");
                assert_eq!(
                    settings.base_url.as_deref(),
                    Some("http://localhost:8080/v1")
                );
            },
        );
    }

    #[test]
    fn an_unset_environment_leaves_the_config_alone() {
        with_env(&[], || {
            let config = LlmConfig {
                model: "gpt-4o".to_string(),
                ..LlmConfig::default()
            };

            assert_eq!(config.with_env_overrides().model, "gpt-4o");
        });
    }

    /// The override points at the variable rather than copying its contents,
    /// so the key is never held in a struct.
    #[test]
    fn the_key_override_records_only_the_variable_name() {
        with_env(&[(API_KEY_ENV, "sk-secret-value")], || {
            let settings = LlmConfig::default().with_env_overrides();

            assert_eq!(settings.key_env(), API_KEY_ENV);
            assert_eq!(settings.api_key_env.as_deref(), Some(API_KEY_ENV));
        });
    }

    /// Enabling paid network calls should take an edit to a file the user
    /// owns, not a variable inherited from a parent shell or a CI job.
    #[test]
    fn no_environment_variable_can_turn_enrichment_on() {
        with_env(
            &[
                (PROVIDER_ENV, "anthropic"),
                (API_KEY_ENV, "sk-secret-value"),
            ],
            || {
                assert!(!LlmConfig::default().with_env_overrides().enabled);
            },
        );
    }

    /// A key in `config.toml` gets committed or shared sooner or later, so
    /// there must be no field that invites one.
    #[test]
    fn the_config_has_no_field_for_a_literal_key() {
        let rendered = toml::to_string(&LlmConfig::default()).expect("render");

        assert!(!rendered.contains("api_key ="), "{rendered}");
    }
}
