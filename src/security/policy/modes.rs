//! Mode-specific behavior for observe/balanced/strict/developer.
//! No mode ever blocks the pipeline.

use crate::security::config::{ActionsConfig, SecurityMode};
use crate::security::types::{SecurityAction, SensitiveEntityType, SeverityLevel};

/// Assign a severity level to a finding based on its entity type and confidence.
pub fn severity_for(entity_type: SensitiveEntityType, confidence: f64) -> SeverityLevel {
    let base = match entity_type {
        SensitiveEntityType::PrivateKey
        | SensitiveEntityType::DatabaseUrl
        | SensitiveEntityType::CreditCard
        | SensitiveEntityType::BankAccount => SeverityLevel::Critical,

        SensitiveEntityType::ApiKey | SensitiveEntityType::Jwt | SensitiveEntityType::Password => {
            SeverityLevel::High
        }

        SensitiveEntityType::Email
        | SensitiveEntityType::Phone
        | SensitiveEntityType::IpAddress
        | SensitiveEntityType::Hostname => SeverityLevel::Medium,

        SensitiveEntityType::Person
        | SensitiveEntityType::Company
        | SensitiveEntityType::Client
        | SensitiveEntityType::InternalProject
        | SensitiveEntityType::Url
        | SensitiveEntityType::Money
        | SensitiveEntityType::Medical
        | SensitiveEntityType::Legal
        | SensitiveEntityType::Custom => SeverityLevel::Low,
    };

    // Downgrade by one level if confidence is below threshold
    if confidence < 0.6 {
        match base {
            SeverityLevel::Critical => SeverityLevel::High,
            SeverityLevel::High => SeverityLevel::Medium,
            SeverityLevel::Medium => SeverityLevel::Low,
            SeverityLevel::Low => SeverityLevel::None,
            SeverityLevel::None => SeverityLevel::None,
        }
    } else {
        base
    }
}

/// Determine the action for a finding based on security mode and config.
pub fn action_for(
    entity_type: SensitiveEntityType,
    mode: SecurityMode,
    actions: &ActionsConfig,
) -> SecurityAction {
    match mode {
        SecurityMode::Observe => SecurityAction::Allow,

        SecurityMode::Developer => {
            // Developer mode preserves technical context but obfuscates secrets
            match entity_type {
                SensitiveEntityType::ApiKey
                | SensitiveEntityType::Jwt
                | SensitiveEntityType::PrivateKey
                | SensitiveEntityType::Password
                | SensitiveEntityType::DatabaseUrl
                | SensitiveEntityType::CreditCard
                | SensitiveEntityType::BankAccount
                | SensitiveEntityType::Hostname
                | SensitiveEntityType::IpAddress => SecurityAction::Placeholder,

                // Allow URLs, code-related content through in developer mode
                SensitiveEntityType::Url => SecurityAction::Allow,

                // For everything else, defer to config
                _ => actions.action_for(entity_type),
            }
        }

        SecurityMode::Balanced => actions.action_for(entity_type),

        SecurityMode::Strict => {
            // Strict mode: obfuscate everything regardless of config
            SecurityAction::Placeholder
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_critical_for_private_key() {
        let sev = severity_for(SensitiveEntityType::PrivateKey, 0.99);
        assert_eq!(sev, SeverityLevel::Critical);
    }

    #[test]
    fn test_severity_downgrade_on_low_confidence() {
        let sev = severity_for(SensitiveEntityType::ApiKey, 0.5);
        assert_eq!(sev, SeverityLevel::Medium); // High -> Medium
    }

    #[test]
    fn test_observe_mode_allows_everything() {
        let actions = ActionsConfig::default();
        let action = action_for(
            SensitiveEntityType::PrivateKey,
            SecurityMode::Observe,
            &actions,
        );
        assert_eq!(action, SecurityAction::Allow);
    }

    #[test]
    fn test_strict_mode_obfuscates_everything() {
        let actions = ActionsConfig::default();
        let action = action_for(SensitiveEntityType::Url, SecurityMode::Strict, &actions);
        assert_eq!(action, SecurityAction::Placeholder);
    }

    #[test]
    fn test_developer_mode_allows_urls() {
        let actions = ActionsConfig::default();
        let action = action_for(SensitiveEntityType::Url, SecurityMode::Developer, &actions);
        assert_eq!(action, SecurityAction::Allow);
    }

    #[test]
    fn test_developer_mode_obfuscates_secrets() {
        let actions = ActionsConfig::default();
        let action = action_for(
            SensitiveEntityType::ApiKey,
            SecurityMode::Developer,
            &actions,
        );
        assert_eq!(action, SecurityAction::Placeholder);
    }

    #[test]
    fn test_balanced_mode_uses_config() {
        let actions = ActionsConfig {
            email: SecurityAction::Allow,
            ..ActionsConfig::default()
        };
        let action = action_for(SensitiveEntityType::Email, SecurityMode::Balanced, &actions);
        assert_eq!(action, SecurityAction::Allow);
    }
}
