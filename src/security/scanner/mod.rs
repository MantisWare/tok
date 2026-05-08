//! Orchestrates scanning: runs regex and secret detectors, merges overlapping findings.

pub mod merge;
pub mod regex_scanner;
pub mod secret_scanner;

use super::config::SecurityConfig;
use super::types::SensitiveFinding;

/// Scan text using all enabled detectors and return deduplicated findings.
pub fn scan(text: &str, config: &SecurityConfig) -> Vec<SensitiveFinding> {
    let mut findings = Vec::new();

    if config.scan.deterministic {
        findings.extend(regex_scanner::scan(text));
        findings.extend(secret_scanner::scan(text));
    }

    merge::deduplicate(&mut findings);
    findings
}
