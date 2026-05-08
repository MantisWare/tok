//! Optional local-first security/privacy layer for TOK.
//!
//! When enabled, this module scans text for sensitive data (PII, secrets, credentials),
//! obfuscates findings with deterministic placeholders, and restores them after
//! the LLM provider returns a response. The pipeline never blocks -- all findings
//! are obfuscated transparently.

pub mod config;
pub mod obfuscation;
pub mod policy;
pub mod report;
pub mod scanner;
pub mod slm;
pub mod types;

use config::{SecurityConfig, SecurityMode, SlmConfig};
use types::{ObfuscationMap, RestorationResult};

/// Process input text through the security pipeline (scan + classify + obfuscate).
/// Returns the sanitized text and the obfuscation map needed for restoration.
pub fn process_input(
    text: &str,
    security_config: &SecurityConfig,
    _slm_config: &SlmConfig,
    mode: SecurityMode,
) -> SecurityInput {
    if !security_config.scan.deterministic && !security_config.scan.slm {
        return SecurityInput {
            sanitized_text: text.to_string(),
            map: ObfuscationMap::new(),
            findings_count: 0,
        };
    }

    let findings = scanner::scan(text, security_config);

    if mode == SecurityMode::Observe {
        return SecurityInput {
            sanitized_text: text.to_string(),
            map: ObfuscationMap::new(),
            findings_count: findings.len(),
        };
    }

    let (sanitized, map) = obfuscation::obfuscate(text, &findings, &security_config.actions, mode);

    SecurityInput {
        sanitized_text: sanitized,
        findings_count: findings.len(),
        map,
    }
}

/// Restore placeholders in the response text using the obfuscation map.
pub fn process_output(text: &str, map: &ObfuscationMap) -> RestorationResult {
    map.restore(text)
}

/// Result of processing input through the security pipeline.
pub struct SecurityInput {
    pub sanitized_text: String,
    pub map: ObfuscationMap,
    pub findings_count: usize,
}

/// Convenience: check if security is enabled based on CLI flags and config.
/// Returns (enabled, mode) tuple.
pub fn resolve_from_cli(
    cli_security: bool,
    cli_no_security: bool,
    cli_mode: Option<&str>,
) -> (bool, SecurityMode) {
    let cfg = crate::core::config::Config::load().unwrap_or_default();

    let enabled = if cli_security {
        true
    } else if cli_no_security {
        false
    } else {
        cfg.security.enabled
    };

    let mode = config::resolve_security_mode(&cfg.security, cli_mode);
    (enabled, mode)
}

/// Apply security obfuscation to text if security is enabled.
/// Returns the (possibly sanitized) text. Use this as a simple one-shot
/// for commands that have already captured their output.
pub fn maybe_sanitize(text: &str, enabled: bool, mode: SecurityMode) -> String {
    if !enabled {
        return text.to_string();
    }
    let cfg = crate::core::config::Config::load().unwrap_or_default();
    let result = process_input(text, &cfg.security, &cfg.slm, mode);
    result.sanitized_text
}
