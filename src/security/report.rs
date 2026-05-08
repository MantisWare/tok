//! Security report formatting for CLI output.

use super::types::{ObfuscationMap, SecurityReport, SeverityLevel};
use colored::Colorize;
use std::collections::HashMap;

/// Build a SecurityReport from the obfuscation map.
pub fn build_report(mode: &str, map: &ObfuscationMap, unresolved_count: usize) -> SecurityReport {
    let mut entity_counts: HashMap<String, usize> = HashMap::new();
    let mut max_severity = SeverityLevel::None;

    for entry in &map.entries {
        *entity_counts
            .entry(format!("{:?}", entry.entity_type).to_lowercase())
            .or_insert(0) += 1;
        if entry.severity > max_severity {
            max_severity = entry.severity;
        }
    }

    SecurityReport {
        enabled: true,
        mode: mode.to_string(),
        severity: max_severity,
        entity_counts,
        total_obfuscated: map.len(),
        total_allowed: 0,
        unresolved_placeholders: unresolved_count,
    }
}

/// Format the security report for CLI display.
pub fn format_report(report: &SecurityReport) -> String {
    let mut lines = Vec::new();

    lines.push("TOK Security Report".bold().to_string());
    lines.push(String::new());
    lines.push(format!("  Security: {}", "enabled".green()));
    lines.push(format!("  Mode:     {}", report.mode));
    lines.push(format!("  Risk:     {}", format_severity(report.severity)));
    lines.push(String::new());

    if report.total_obfuscated > 0 {
        lines.push(format!(
            "  Obfuscated: {} sensitive value{}",
            report.total_obfuscated,
            if report.total_obfuscated == 1 {
                ""
            } else {
                "s"
            }
        ));

        let mut sorted: Vec<_> = report.entity_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (entity_type, count) in sorted {
            lines.push(format!("    - {}: {}", entity_type, count));
        }
    } else {
        lines.push("  No sensitive data detected.".to_string());
    }

    if report.unresolved_placeholders > 0 {
        lines.push(String::new());
        lines.push(format!(
            "  {} unresolved placeholder{} in response",
            report.unresolved_placeholders.to_string().yellow(),
            if report.unresolved_placeholders == 1 {
                ""
            } else {
                "s"
            }
        ));
    }

    lines.join("\n")
}

fn format_severity(severity: SeverityLevel) -> String {
    match severity {
        SeverityLevel::None => "none".dimmed().to_string(),
        SeverityLevel::Low => "low".green().to_string(),
        SeverityLevel::Medium => "medium".yellow().to_string(),
        SeverityLevel::High => "high".red().to_string(),
        SeverityLevel::Critical => "critical".red().bold().to_string(),
    }
}
