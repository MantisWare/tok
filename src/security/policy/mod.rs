//! Policy evaluation: classifies findings by severity and determines obfuscation strategy.
//! The pipeline NEVER blocks -- all findings are either obfuscated or allowed through.

pub mod modes;

use super::config::{ActionsConfig, SecurityMode};
use super::types::{SecurityAction, SensitiveFinding, SeverityLevel};

/// Classification result for a single finding.
pub struct ClassifiedFinding {
    pub finding: SensitiveFinding,
    pub severity: SeverityLevel,
    pub action: SecurityAction,
}

/// Classify all findings according to the active security mode and actions config.
pub fn classify(
    findings: &[SensitiveFinding],
    mode: SecurityMode,
    actions: &ActionsConfig,
) -> Vec<ClassifiedFinding> {
    findings
        .iter()
        .map(|f| {
            let severity = modes::severity_for(f.entity_type, f.confidence);
            let action = modes::action_for(f.entity_type, mode, actions);
            ClassifiedFinding {
                finding: f.clone(),
                severity,
                action,
            }
        })
        .collect()
}

/// Determine the overall severity level from a set of classified findings.
pub fn overall_severity(classified: &[ClassifiedFinding]) -> SeverityLevel {
    classified
        .iter()
        .map(|c| c.severity)
        .max()
        .unwrap_or(SeverityLevel::None)
}
