//! Deterministic regex-based PII scanner: email, phone, URL, hostname, IP, money.

use crate::security::types::{DetectionSource, SensitiveEntityType, SensitiveFinding};
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref EMAIL_RE: Regex = Regex::new(
        r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}"
    ).unwrap();

    static ref PHONE_RE: Regex = Regex::new(
        r"(?:\+?1[-.\s]?)?\(?[0-9]{3}\)?[-.\s]?[0-9]{3}[-.\s]?[0-9]{4}"
    ).unwrap();

    static ref URL_RE: Regex = Regex::new(
        r#"https?://[^\s<>"')}\]]+"#
    ).unwrap();

    static ref HOSTNAME_RE: Regex = Regex::new(
        r"\b(?:[a-zA-Z0-9](?:[a-zA-Z0-9\-]*[a-zA-Z0-9])?\.)+(?:internal|local|corp|intra|private|lan)\b"
    ).unwrap();

    static ref IP_V4_RE: Regex = Regex::new(
        r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b"
    ).unwrap();

    static ref MONEY_RE: Regex = Regex::new(
        r"(?:\$|€|£|¥|USD|EUR|GBP|CAD|AUD)\s?[\d,]+(?:\.\d{1,2})?"
    ).unwrap();
}

pub fn scan(text: &str) -> Vec<SensitiveFinding> {
    let mut findings = Vec::new();

    for m in EMAIL_RE.find_iter(text) {
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::Email,
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.95,
            source: DetectionSource::Regex,
        });
    }

    for m in PHONE_RE.find_iter(text) {
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::Phone,
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.80,
            source: DetectionSource::Regex,
        });
    }

    for m in URL_RE.find_iter(text) {
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::Url,
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.90,
            source: DetectionSource::Regex,
        });
    }

    for m in HOSTNAME_RE.find_iter(text) {
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::Hostname,
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.85,
            source: DetectionSource::Regex,
        });
    }

    for m in IP_V4_RE.find_iter(text) {
        let ip = m.as_str();
        // Skip common non-sensitive IPs
        if ip == "127.0.0.1" || ip == "0.0.0.0" || ip.starts_with("255.") {
            continue;
        }
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::IpAddress,
            value: ip.to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.85,
            source: DetectionSource::Regex,
        });
    }

    for m in MONEY_RE.find_iter(text) {
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::Money,
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.90,
            source: DetectionSource::Regex,
        });
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_email() {
        let findings = scan("Contact john@example.com for details.");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].entity_type, SensitiveEntityType::Email);
        assert_eq!(findings[0].value, "john@example.com");
    }

    #[test]
    fn test_detects_multiple_emails() {
        let findings = scan("From alice@test.org to bob@corp.co.uk");
        let emails: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::Email)
            .collect();
        assert_eq!(emails.len(), 2);
    }

    #[test]
    fn test_detects_phone() {
        let findings = scan("Call me at (555) 123-4567 or +1-555-987-6543");
        let phones: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::Phone)
            .collect();
        assert!(!phones.is_empty());
    }

    #[test]
    fn test_detects_url() {
        let findings = scan("Visit https://internal.company.com/api/v2 for docs");
        let urls: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::Url)
            .collect();
        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn test_detects_internal_hostname() {
        let findings = scan("Connect to db-prod-01.internal on port 5432");
        let hosts: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::Hostname)
            .collect();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].value, "db-prod-01.internal");
    }

    #[test]
    fn test_detects_ip_address() {
        let findings = scan("Server at 192.168.1.100 is down");
        let ips: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::IpAddress)
            .collect();
        assert_eq!(ips.len(), 1);
        assert_eq!(ips[0].value, "192.168.1.100");
    }

    #[test]
    fn test_skips_localhost() {
        let findings = scan("Listening on 127.0.0.1:8080");
        let ips: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::IpAddress)
            .collect();
        assert_eq!(ips.len(), 0);
    }

    #[test]
    fn test_detects_money() {
        let findings = scan("Invoice total: $45,000.00 due next week");
        let money: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::Money)
            .collect();
        assert_eq!(money.len(), 1);
    }
}
