//! High-risk secret detector: API keys, JWT, private keys, passwords, DB URLs, credit cards.

use crate::security::types::{DetectionSource, SensitiveEntityType, SensitiveFinding};
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    // API key prefixes from major providers
    static ref API_KEY_RE: Regex = Regex::new(
        r"(?:sk_live_[a-zA-Z0-9]{20,}|sk_test_[a-zA-Z0-9]{20,}|ghp_[a-zA-Z0-9]{36,}|github_pat_[a-zA-Z0-9_]{22,}|xoxb-[0-9]{10,}-[a-zA-Z0-9]+|xoxp-[0-9]{10,}-[a-zA-Z0-9]+|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z\-_]{35}|sk-[a-zA-Z0-9]{20,})"
    ).unwrap();

    // JWT: three base64url segments separated by dots
    static ref JWT_RE: Regex = Regex::new(
        r"\beyJ[a-zA-Z0-9_\-]+\.eyJ[a-zA-Z0-9_\-]+\.[a-zA-Z0-9_\-]+"
    ).unwrap();

    // Private key headers
    static ref PRIVATE_KEY_RE: Regex = Regex::new(
        r"-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----"
    ).unwrap();

    // Password assignments
    static ref PASSWORD_RE: Regex = Regex::new(
        r#"(?i)(?:password|passwd|pwd|secret)\s*[:=]\s*["']?([^\s"']{4,})["']?"#
    ).unwrap();

    // Database URLs with credentials
    static ref DATABASE_URL_RE: Regex = Regex::new(
        r#"(?:postgres(?:ql)?|mysql|mongodb(?:\+srv)?|redis|mariadb)://[^\s"'`]+"#
    ).unwrap();

    // Credit card numbers (13-19 digits, optionally separated by spaces/dashes)
    static ref CREDIT_CARD_RE: Regex = Regex::new(
        r"\b(?:\d[ \-]?){13,19}\b"
    ).unwrap();
}

pub fn scan(text: &str) -> Vec<SensitiveFinding> {
    let mut findings = Vec::new();

    for m in API_KEY_RE.find_iter(text) {
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::ApiKey,
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.95,
            source: DetectionSource::Secret,
        });
    }

    for m in JWT_RE.find_iter(text) {
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::Jwt,
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.92,
            source: DetectionSource::Secret,
        });
    }

    for m in PRIVATE_KEY_RE.find_iter(text) {
        // Capture the full key block if possible
        let key_start = m.start();
        let end_marker = "-----END";
        let key_end = if let Some(end_pos) = text[key_start..].find(end_marker) {
            let after_end = &text[key_start + end_pos..];
            if let Some(line_end) = after_end.find('\n') {
                key_start + end_pos + line_end + 1
            } else {
                key_start + end_pos + after_end.len()
            }
        } else {
            m.end()
        };

        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::PrivateKey,
            value: text[key_start..key_end].to_string(),
            start: key_start,
            end: key_end,
            confidence: 0.99,
            source: DetectionSource::Secret,
        });
    }

    for caps in PASSWORD_RE.captures_iter(text) {
        let m = caps.get(0).unwrap();
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::Password,
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.88,
            source: DetectionSource::Secret,
        });
    }

    for m in DATABASE_URL_RE.find_iter(text) {
        findings.push(SensitiveFinding {
            entity_type: SensitiveEntityType::DatabaseUrl,
            value: m.as_str().to_string(),
            start: m.start(),
            end: m.end(),
            confidence: 0.95,
            source: DetectionSource::Secret,
        });
    }

    for m in CREDIT_CARD_RE.find_iter(text) {
        let digits: String = m.as_str().chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() >= 13 && digits.len() <= 19 && luhn_check(&digits) {
            findings.push(SensitiveFinding {
                entity_type: SensitiveEntityType::CreditCard,
                value: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
                confidence: 0.90,
                source: DetectionSource::Secret,
            });
        }
    }

    findings
}

/// Luhn algorithm validation for credit card numbers.
fn luhn_check(digits: &str) -> bool {
    let mut sum: u32 = 0;
    let mut double = false;

    for ch in digits.chars().rev() {
        let Some(d) = ch.to_digit(10) else {
            return false;
        };
        let val = if double {
            let doubled = d * 2;
            if doubled > 9 {
                doubled - 9
            } else {
                doubled
            }
        } else {
            d
        };
        sum += val;
        double = !double;
    }

    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detects_stripe_api_key() {
        let fake_key = format!("sk_test_{}", "FAKE0000000000000000000");
        let input = format!("Using key {fake_key} for production");
        let findings = scan(&input);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].entity_type, SensitiveEntityType::ApiKey);
    }

    #[test]
    fn test_detects_github_pat() {
        let findings = scan("export GITHUB_TOKEN=ghp_abcdefghijklmnopqrstuvwxyz1234567890");
        let keys: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::ApiKey)
            .collect();
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn test_detects_aws_key() {
        let findings = scan("aws_access_key_id = AKIAIOSFODNN7EXAMPLE");
        let keys: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::ApiKey)
            .collect();
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn test_detects_jwt() {
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let findings = scan(&format!("Bearer {}", token));
        let jwts: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::Jwt)
            .collect();
        assert_eq!(jwts.len(), 1);
    }

    #[test]
    fn test_detects_private_key_header() {
        let text =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKC...\n-----END RSA PRIVATE KEY-----\n";
        let findings = scan(text);
        let keys: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::PrivateKey)
            .collect();
        assert_eq!(keys.len(), 1);
    }

    #[test]
    fn test_detects_password_assignment() {
        let findings = scan("DB_PASSWORD=super_secret_123");
        let passwords: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::Password)
            .collect();
        assert_eq!(passwords.len(), 1);
    }

    #[test]
    fn test_detects_database_url() {
        let findings = scan("DATABASE_URL=postgres://admin:secret@prod-db.company.local:5432/app");
        let dbs: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::DatabaseUrl)
            .collect();
        assert_eq!(dbs.len(), 1);
    }

    #[test]
    fn test_luhn_valid_card() {
        assert!(luhn_check("4111111111111111")); // Visa test
        assert!(luhn_check("5500000000000004")); // Mastercard test
    }

    #[test]
    fn test_luhn_invalid() {
        assert!(!luhn_check("1234567890123456"));
    }

    #[test]
    fn test_detects_credit_card() {
        let findings = scan("Card number: 4111 1111 1111 1111 exp 12/26");
        let cards: Vec<_> = findings
            .iter()
            .filter(|f| f.entity_type == SensitiveEntityType::CreditCard)
            .collect();
        assert_eq!(cards.len(), 1);
    }
}
