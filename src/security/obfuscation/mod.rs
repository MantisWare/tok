//! Obfuscation engine: replaces sensitive findings with deterministic placeholders.

use super::config::{ActionsConfig, SecurityMode};
use super::policy;
use super::types::{ObfuscationMap, SecurityAction, SensitiveFinding};

/// Obfuscate all findings in the input text according to the policy.
/// Returns the sanitized text and the obfuscation map for later restoration.
pub fn obfuscate(
    text: &str,
    findings: &[SensitiveFinding],
    actions: &ActionsConfig,
    mode: SecurityMode,
) -> (String, ObfuscationMap) {
    let classified = policy::classify(findings, mode, actions);
    let mut map = ObfuscationMap::new();

    // Sort classified findings by start position (descending) so we can replace
    // from the end of the string without invalidating earlier positions.
    let mut to_replace: Vec<_> = classified
        .into_iter()
        .filter(|c| c.action == SecurityAction::Placeholder)
        .collect();
    to_replace.sort_by(|a, b| b.finding.start.cmp(&a.finding.start));

    let mut result = text.to_string();

    for classified_finding in &to_replace {
        let placeholder = map.add(&classified_finding.finding, classified_finding.severity);
        let start = classified_finding.finding.start;
        let end = classified_finding.finding.end;

        if start <= result.len() && end <= result.len() && start <= end {
            result.replace_range(start..end, &placeholder);
        }
    }

    (result, map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::types::{DetectionSource, SensitiveEntityType};

    #[test]
    fn test_obfuscate_single_email() {
        let text = "Contact john@example.com for details.";
        let findings = vec![SensitiveFinding {
            entity_type: SensitiveEntityType::Email,
            value: "john@example.com".into(),
            start: 8,
            end: 24,
            confidence: 0.95,
            source: DetectionSource::Regex,
        }];

        let (sanitized, map) = obfuscate(
            text,
            &findings,
            &ActionsConfig::default(),
            SecurityMode::Balanced,
        );

        assert!(!sanitized.contains("john@example.com"));
        assert!(sanitized.contains("{{TOK_EMAIL_001}}"));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn test_obfuscate_preserves_surrounding_text() {
        let text = "Hello alice@test.org goodbye";
        let findings = vec![SensitiveFinding {
            entity_type: SensitiveEntityType::Email,
            value: "alice@test.org".into(),
            start: 6,
            end: 20,
            confidence: 0.95,
            source: DetectionSource::Regex,
        }];

        let (sanitized, _) = obfuscate(
            text,
            &findings,
            &ActionsConfig::default(),
            SecurityMode::Balanced,
        );

        assert!(sanitized.starts_with("Hello "));
        assert!(sanitized.ends_with(" goodbye"));
    }

    #[test]
    fn test_obfuscate_multiple_findings() {
        let text = "Email: a@b.com Phone: 555-123-4567";
        let findings = vec![
            SensitiveFinding {
                entity_type: SensitiveEntityType::Email,
                value: "a@b.com".into(),
                start: 7,
                end: 14,
                confidence: 0.95,
                source: DetectionSource::Regex,
            },
            SensitiveFinding {
                entity_type: SensitiveEntityType::Phone,
                value: "555-123-4567".into(),
                start: 22,
                end: 34,
                confidence: 0.80,
                source: DetectionSource::Regex,
            },
        ];

        let (sanitized, map) = obfuscate(
            text,
            &findings,
            &ActionsConfig::default(),
            SecurityMode::Balanced,
        );

        assert!(!sanitized.contains("a@b.com"));
        assert!(!sanitized.contains("555-123-4567"));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_observe_mode_does_not_obfuscate() {
        let fake_key = format!("sk_test_{}", "FAKE00000000000000000000");
        let text = format!("Secret: {fake_key}");
        let findings = vec![SensitiveFinding {
            entity_type: SensitiveEntityType::ApiKey,
            value: fake_key.clone(),
            start: 8,
            end: 40,
            confidence: 0.95,
            source: DetectionSource::Secret,
        }];

        let (sanitized, map) = obfuscate(
            &text,
            &findings,
            &ActionsConfig::default(),
            SecurityMode::Observe,
        );

        // Observe mode allows everything through - no obfuscation
        assert!(sanitized.contains(&fake_key));
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn test_roundtrip_obfuscate_restore() {
        let text = "Send $45,000 to alice@corp.com on server db-prod.internal";
        let findings = vec![
            SensitiveFinding {
                entity_type: SensitiveEntityType::Money,
                value: "$45,000".into(),
                start: 5,
                end: 12,
                confidence: 0.90,
                source: DetectionSource::Regex,
            },
            SensitiveFinding {
                entity_type: SensitiveEntityType::Email,
                value: "alice@corp.com".into(),
                start: 16,
                end: 30,
                confidence: 0.95,
                source: DetectionSource::Regex,
            },
            SensitiveFinding {
                entity_type: SensitiveEntityType::Hostname,
                value: "db-prod.internal".into(),
                start: 41,
                end: 57,
                confidence: 0.85,
                source: DetectionSource::Regex,
            },
        ];

        let (sanitized, map) = obfuscate(
            text,
            &findings,
            &ActionsConfig::default(),
            SecurityMode::Balanced,
        );

        // Verify nothing sensitive in sanitized
        assert!(!sanitized.contains("$45,000"));
        assert!(!sanitized.contains("alice@corp.com"));
        assert!(!sanitized.contains("db-prod.internal"));

        // Restore
        let result = map.restore(&sanitized);
        assert_eq!(result.text, text);
        assert!(result.unresolved_placeholders.is_empty());
    }
}
