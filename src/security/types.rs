//! Core types for the TOK security/privacy layer.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Categories of sensitive entities that TOK can detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitiveEntityType {
    Email,
    Phone,
    Person,
    Company,
    Client,
    InternalProject,
    Url,
    Hostname,
    IpAddress,
    ApiKey,
    Jwt,
    PrivateKey,
    Password,
    DatabaseUrl,
    CreditCard,
    BankAccount,
    Money,
    Medical,
    Legal,
    Custom,
}

impl SensitiveEntityType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Email => "EMAIL",
            Self::Phone => "PHONE",
            Self::Person => "PERSON",
            Self::Company => "COMPANY",
            Self::Client => "CLIENT",
            Self::InternalProject => "PROJECT",
            Self::Url => "URL",
            Self::Hostname => "HOST",
            Self::IpAddress => "IP",
            Self::ApiKey => "SECRET",
            Self::Jwt => "SECRET",
            Self::PrivateKey => "SECRET",
            Self::Password => "SECRET",
            Self::DatabaseUrl => "SECRET",
            Self::CreditCard => "SECRET",
            Self::BankAccount => "SECRET",
            Self::Money => "MONEY",
            Self::Medical => "MEDICAL",
            Self::Legal => "LEGAL",
            Self::Custom => "CUSTOM",
        }
    }
}

/// What action to take for a detected entity.
/// Note: there is no "block" action -- TOK always obfuscates, never blocks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityAction {
    /// Leave the value untouched
    Allow,
    /// Replace with a deterministic placeholder like {{TOK_EMAIL_001}}
    #[default]
    Placeholder,
}

/// How the finding was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionSource {
    Regex,
    Secret,
    Slm,
    Custom,
}

/// A single sensitive value found in the input text.
#[derive(Debug, Clone)]
pub struct SensitiveFinding {
    pub entity_type: SensitiveEntityType,
    pub value: String,
    pub start: usize,
    pub end: usize,
    pub confidence: f64,
    pub source: DetectionSource,
}

/// Severity level for reporting purposes only -- does not gate the pipeline.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeverityLevel {
    #[default]
    None,
    Low,
    Medium,
    High,
    Critical,
}

/// One entry in the obfuscation map linking a placeholder to its original value.
#[derive(Debug, Clone)]
pub struct ObfuscationEntry {
    pub entity_type: SensitiveEntityType,
    pub original: String,
    pub placeholder: String,
    pub confidence: f64,
    pub severity: SeverityLevel,
}

/// In-memory-only map of placeholder -> original value.
/// This MUST NEVER be serialized to disk or sent over the network.
#[derive(Debug, Clone, Default)]
pub struct ObfuscationMap {
    pub entries: Vec<ObfuscationEntry>,
    placeholder_counters: HashMap<String, usize>,
}

impl ObfuscationMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Generate the next placeholder for a given entity type and record the mapping.
    pub fn add(&mut self, finding: &SensitiveFinding, severity: SeverityLevel) -> String {
        let label = finding.entity_type.label();
        let counter = self
            .placeholder_counters
            .entry(label.to_string())
            .or_insert(0);
        *counter += 1;
        let placeholder = format!("{{{{TOK_{label}_{:03}}}}}", *counter);

        self.entries.push(ObfuscationEntry {
            entity_type: finding.entity_type,
            original: finding.value.clone(),
            placeholder: placeholder.clone(),
            confidence: finding.confidence,
            severity,
        });

        placeholder
    }

    /// Restore all placeholders in the given text back to their original values.
    pub fn restore(&self, text: &str) -> RestorationResult {
        let mut result = text.to_string();
        for entry in &self.entries {
            result = result.replace(&entry.placeholder, &entry.original);
        }

        let unresolved = find_unresolved_placeholders(&result);
        RestorationResult {
            text: result,
            unresolved_placeholders: unresolved,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Result of restoring placeholders in a response.
#[derive(Debug, Clone)]
pub struct RestorationResult {
    pub text: String,
    pub unresolved_placeholders: Vec<String>,
}

/// Finds any remaining {{TOK_*}} placeholders that were not restored.
fn find_unresolved_placeholders(text: &str) -> Vec<String> {
    let mut unresolved = Vec::new();
    let mut search_from = 0;
    while let Some(start) = text[search_from..].find("{{TOK_") {
        let abs_start = search_from + start;
        if let Some(end) = text[abs_start..].find("}}") {
            let placeholder = &text[abs_start..abs_start + end + 2];
            unresolved.push(placeholder.to_string());
            search_from = abs_start + end + 2;
        } else {
            break;
        }
    }
    unresolved
}

/// Summary report of security actions taken, safe for display/logging.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SecurityReport {
    pub enabled: bool,
    pub mode: String,
    pub severity: SeverityLevel,
    pub entity_counts: HashMap<String, usize>,
    pub total_obfuscated: usize,
    pub total_allowed: usize,
    pub unresolved_placeholders: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_obfuscation_map_generates_sequential_placeholders() {
        let mut map = ObfuscationMap::new();
        let finding1 = SensitiveFinding {
            entity_type: SensitiveEntityType::Email,
            value: "alice@example.com".into(),
            start: 0,
            end: 17,
            confidence: 0.95,
            source: DetectionSource::Regex,
        };
        let finding2 = SensitiveFinding {
            entity_type: SensitiveEntityType::Email,
            value: "bob@example.com".into(),
            start: 20,
            end: 35,
            confidence: 0.95,
            source: DetectionSource::Regex,
        };

        let p1 = map.add(&finding1, SeverityLevel::Low);
        let p2 = map.add(&finding2, SeverityLevel::Low);

        assert_eq!(p1, "{{TOK_EMAIL_001}}");
        assert_eq!(p2, "{{TOK_EMAIL_002}}");
    }

    #[test]
    fn test_restoration_replaces_placeholders() {
        let mut map = ObfuscationMap::new();
        let finding = SensitiveFinding {
            entity_type: SensitiveEntityType::Email,
            value: "alice@example.com".into(),
            start: 0,
            end: 17,
            confidence: 0.95,
            source: DetectionSource::Regex,
        };
        map.add(&finding, SeverityLevel::Low);

        let result = map.restore("Contact {{TOK_EMAIL_001}} for info.");
        assert_eq!(result.text, "Contact alice@example.com for info.");
        assert!(result.unresolved_placeholders.is_empty());
    }

    #[test]
    fn test_unresolved_placeholders_detected() {
        let map = ObfuscationMap::new();
        let result = map.restore("Hello {{TOK_PERSON_001}}, your key is {{TOK_SECRET_001}}.");
        assert_eq!(result.unresolved_placeholders.len(), 2);
    }
}
