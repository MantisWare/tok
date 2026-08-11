//! Providers for the `--deep` enrichment layer, and the redaction gate that
//! everything leaving the machine passes through.
//!
//! Two wire formats cover the field: OpenAI-compatible (`/chat/completions`,
//! which Azure, Groq, Together, Ollama, vLLM and most local servers also
//! speak) and Anthropic's native `/v1/messages`, which differs enough — system
//! prompt as a top-level field, `x-api-key` instead of a bearer token, a
//! mandatory version header — that adapting it is not worth the ceremony.
//!
//! Requests are synchronous `ureq`. This binary has no async runtime and adding
//! one for a feature that is off by default would be paid for by every `tok git
//! log` in startup time.
//!
//! **The redaction gate is the point of this module.** `Client::complete` is
//! the only way to reach a provider, and it scans and redacts before the
//! request is built. Source code is exactly the kind of text that has
//! credentials sitting in it, and "we meant to redact that" is not a thing you
//! can say after it has been posted to someone else's server.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::graph::config::LlmConfig;
use crate::security::config::SecurityMode;

const OPENAI_DEFAULT_URL: &str = "https://api.openai.com/v1/chat/completions";
const ANTHROPIC_DEFAULT_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Cap on a single reply. Summaries are a sentence or two; anything longer is
/// a runaway response being paid for by the token.
const MAX_RESPONSE_TOKENS: u32 = 400;

/// A configured provider, ready to answer prompts.
pub struct Client {
    kind: Kind,
    model: String,
    url: String,
    api_key: String,
    timeout: std::time::Duration,
}

/// Written by hand rather than derived, so the key cannot reach a log line or
/// a panic message through the one trait everything gets formatted with.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("kind", &self.kind)
            .field("model", &self.model)
            .field("url", &self.url)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    OpenAi,
    Anthropic,
}

impl Client {
    /// Build a client, or explain what is missing.
    ///
    /// Failures here are configuration mistakes a person can fix, so they carry
    /// the variable name or the accepted values rather than a type error.
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let kind = match config.provider.as_str() {
            "openai" => Kind::OpenAi,
            "anthropic" => Kind::Anthropic,
            other => bail!("Unknown LLM provider {other:?}. Use \"openai\" or \"anthropic\"."),
        };

        let key_env = config.key_env();
        let api_key = std::env::var(key_env)
            .with_context(|| format!("{key_env} is not set, so --deep has no credentials"))?;

        if api_key.trim().is_empty() {
            bail!("{key_env} is set but empty");
        }

        let url = config.base_url.clone().unwrap_or_else(|| {
            match kind {
                Kind::OpenAi => OPENAI_DEFAULT_URL,
                Kind::Anthropic => ANTHROPIC_DEFAULT_URL,
            }
            .to_string()
        });

        Ok(Self {
            kind,
            model: config.model.clone(),
            url,
            api_key,
            timeout: std::time::Duration::from_secs(config.timeout_secs),
        })
    }

    /// Send a prompt and return the reply.
    ///
    /// Both prompts are redacted first. This is not optional and not
    /// configurable: the input is source code, which routinely contains keys
    /// and connection strings, and the destination is a third party.
    pub fn complete(&self, system: &str, user: &str) -> Result<String> {
        let system = redact(system);
        let user = redact(user);

        let request = self.build_request(&system, &user);
        let response = self.send(&request)?;

        extract_text(self.kind, &response)
    }

    fn build_request(&self, system: &str, user: &str) -> Value {
        match self.kind {
            Kind::OpenAi => json!({
                "model": self.model,
                "max_tokens": MAX_RESPONSE_TOKENS,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
            }),
            // Anthropic takes the system prompt as a top-level field; sending
            // it as a message role is rejected.
            Kind::Anthropic => json!({
                "model": self.model,
                "max_tokens": MAX_RESPONSE_TOKENS,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
            }),
        }
    }

    fn send(&self, request: &Value) -> Result<Value> {
        let mut call = ureq::post(&self.url)
            .set("Content-Type", "application/json")
            .timeout(self.timeout);

        call = match self.kind {
            Kind::OpenAi => call.set("Authorization", &format!("Bearer {}", self.api_key)),
            Kind::Anthropic => call
                .set("x-api-key", &self.api_key)
                .set("anthropic-version", ANTHROPIC_VERSION),
        };

        let response = call
            .send_string(&request.to_string())
            .map_err(|error| describe(error, &self.url))?;

        let body = response
            .into_string()
            .context("The provider's response could not be read")?;

        serde_json::from_str(&body).context("The provider returned a response that was not JSON")
    }
}

/// Turn a transport error into something actionable.
///
/// `ureq`'s own message for a 401 is "status code 401", which tells a user
/// nothing about which of their several API keys is wrong.
fn describe(error: ureq::Error, url: &str) -> anyhow::Error {
    match error {
        ureq::Error::Status(401 | 403, _) => {
            anyhow::anyhow!(
                "The provider rejected the API key (check the key environment variable)"
            )
        }
        ureq::Error::Status(429, _) => {
            anyhow::anyhow!("The provider is rate limiting; try again or lower max_files")
        }
        ureq::Error::Status(code, response) => {
            let detail = response
                .into_string()
                .unwrap_or_else(|_| "no response body".to_string());
            anyhow::anyhow!("The provider returned {code}: {}", detail.trim())
        }
        ureq::Error::Transport(transport) => {
            anyhow::anyhow!("Could not reach {url}: {transport}")
        }
    }
}

/// Pull the reply text out of whichever envelope came back.
fn extract_text(kind: Kind, response: &Value) -> Result<String> {
    let text = match kind {
        Kind::OpenAi => response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str),
        Kind::Anthropic => response.pointer("/content/0/text").and_then(Value::as_str),
    };

    text.map(|text| text.trim().to_string())
        .context("The provider's response contained no message text")
}

/// Scan for secrets and replace what is found.
///
/// Reuses the scanner behind `tok --security` so there is one definition of
/// what counts as sensitive, and anything taught to that scanner protects this
/// path for free.
pub fn redact(text: &str) -> String {
    let config = crate::core::config::Config::load().unwrap_or_default();
    let findings = crate::security::scanner::scan(text, &config.security);

    if findings.is_empty() {
        return text.to_string();
    }

    let (redacted, _) = crate::security::obfuscation::obfuscate(
        text,
        &findings,
        &config.security.actions,
        // Always the strictest mode, whatever the user runs commands in.
        // Observe means "tell me, do not change my output" — a reasonable
        // default for a terminal and the wrong one for an outbound request.
        SecurityMode::Strict,
    );

    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(provider: &str) -> LlmConfig {
        LlmConfig {
            provider: provider.to_string(),
            ..LlmConfig::default()
        }
    }

    fn client(kind: Kind) -> Client {
        Client {
            kind,
            model: "test-model".to_string(),
            url: "https://example.invalid".to_string(),
            api_key: "test-key".to_string(),
            timeout: std::time::Duration::from_secs(1),
        }
    }

    // ------------------------------------------------------------ redaction

    /// The single most important property in this module: a credential in the
    /// source must not reach the provider.
    #[test]
    fn secrets_are_removed_before_a_request_is_built() {
        let source = "let key = \"sk-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefgh\";";

        let request = client(Kind::OpenAi).build_request("system", &redact(source));

        let rendered = request.to_string();
        assert!(
            !rendered.contains("sk-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefgh"),
            "the key survived into the request: {rendered}"
        );
    }

    #[test]
    fn redaction_leaves_ordinary_code_alone() {
        let source = "pub fn add(a: i32, b: i32) -> i32 { a + b }";

        assert_eq!(redact(source), source);
    }

    #[test]
    fn redaction_handles_empty_input() {
        assert_eq!(redact(""), "");
    }

    // -------------------------------------------------------- request shape

    #[test]
    fn openai_sends_the_system_prompt_as_a_message() {
        let request = client(Kind::OpenAi).build_request("be brief", "explain this");

        assert_eq!(request["messages"][0]["role"], "system");
        assert_eq!(request["messages"][0]["content"], "be brief");
        assert_eq!(request["messages"][1]["role"], "user");
        assert_eq!(request["model"], "test-model");
    }

    /// Anthropic rejects a system message sent as a role.
    #[test]
    fn anthropic_sends_the_system_prompt_as_a_top_level_field() {
        let request = client(Kind::Anthropic).build_request("be brief", "explain this");

        assert_eq!(request["system"], "be brief");
        assert_eq!(request["messages"].as_array().expect("messages").len(), 1);
        assert_eq!(request["messages"][0]["role"], "user");
    }

    /// An unbounded reply is billed by the token.
    #[test]
    fn both_providers_cap_the_response_length() {
        for kind in [Kind::OpenAi, Kind::Anthropic] {
            let request = client(kind).build_request("s", "u");
            assert_eq!(request["max_tokens"], MAX_RESPONSE_TOKENS);
        }
    }

    // ------------------------------------------------------- response shape

    #[test]
    fn an_openai_reply_is_read_from_the_choices_array() {
        let response = json!({
            "choices": [{ "message": { "content": "  a summary  " } }]
        });

        assert_eq!(
            extract_text(Kind::OpenAi, &response).expect("text"),
            "a summary"
        );
    }

    #[test]
    fn an_anthropic_reply_is_read_from_the_content_blocks() {
        let response = json!({ "content": [{ "type": "text", "text": "a summary" }] });

        assert_eq!(
            extract_text(Kind::Anthropic, &response).expect("text"),
            "a summary"
        );
    }

    #[test]
    fn a_reply_with_no_text_is_an_error_not_an_empty_string() {
        let response = json!({ "choices": [] });

        let error = extract_text(Kind::OpenAi, &response).expect_err("should fail");

        assert!(format!("{error}").contains("no message text"));
    }

    // ------------------------------------------------------- configuration

    #[test]
    fn an_unknown_provider_names_the_ones_that_work() {
        let error = Client::new(&config("gemini")).expect_err("should fail");

        assert!(format!("{error}").contains("openai"));
        assert!(format!("{error}").contains("anthropic"));
    }

    #[test]
    fn a_missing_key_names_the_variable_to_set() {
        let mut settings = config("openai");
        settings.api_key_env = Some("TOK_TEST_ABSENT_KEY".to_string());

        let error = Client::new(&settings).expect_err("should fail");

        assert!(format!("{error}").contains("TOK_TEST_ABSENT_KEY"));
    }

    /// `Debug` is the one trait everything gets formatted with, including
    /// panic messages and log lines.
    #[test]
    fn debug_output_never_contains_the_api_key() {
        let rendered = format!("{:?}", client(Kind::OpenAi));

        assert!(!rendered.contains("test-key"), "{rendered}");
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn each_provider_has_a_default_endpoint() {
        assert!(OPENAI_DEFAULT_URL.contains("openai.com"));
        assert!(ANTHROPIC_DEFAULT_URL.contains("anthropic.com"));
    }

    #[test]
    fn a_rejected_key_is_reported_as_a_key_problem() {
        let response = ureq::Response::new(401, "Unauthorized", "{}").expect("response");

        let error = describe(
            ureq::Error::Status(401, response),
            "https://example.invalid",
        );

        assert!(format!("{error}").contains("API key"));
    }

    #[test]
    fn rate_limiting_suggests_a_smaller_run() {
        let response = ureq::Response::new(429, "Too Many Requests", "{}").expect("response");

        let error = describe(
            ureq::Error::Status(429, response),
            "https://example.invalid",
        );

        assert!(format!("{error}").contains("max_files"));
    }
}
