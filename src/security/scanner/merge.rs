//! Finding overlap resolution: handles cases where multiple detectors match the same text span.

use crate::security::types::{SensitiveEntityType, SensitiveFinding};

/// Priority order for entity types when spans overlap.
/// Higher-priority types win over lower-priority ones.
fn type_priority(t: SensitiveEntityType) -> u8 {
    match t {
        SensitiveEntityType::PrivateKey => 10,
        SensitiveEntityType::DatabaseUrl => 9,
        SensitiveEntityType::Password => 8,
        SensitiveEntityType::Jwt => 7,
        SensitiveEntityType::ApiKey => 7,
        SensitiveEntityType::CreditCard => 6,
        SensitiveEntityType::BankAccount => 6,
        _ => 1,
    }
}

/// Remove overlapping findings, preferring higher-priority entity types
/// and longer spans when types have equal priority.
pub fn deduplicate(findings: &mut Vec<SensitiveFinding>) {
    if findings.len() <= 1 {
        return;
    }

    // Sort by start position, then by priority (desc), then by span length (desc)
    findings.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| type_priority(b.entity_type).cmp(&type_priority(a.entity_type)))
            .then_with(|| (b.end - b.start).cmp(&(a.end - a.start)))
    });

    let mut keep = vec![true; findings.len()];

    for i in 0..findings.len() {
        if !keep[i] {
            continue;
        }
        for j in (i + 1)..findings.len() {
            if !keep[j] {
                continue;
            }
            // Check overlap
            if findings[j].start < findings[i].end {
                // j overlaps with i -- drop j since i has higher priority/earlier position
                keep[j] = false;
            } else {
                // No more overlaps possible from i (sorted by start)
                break;
            }
        }
    }

    let mut idx = 0;
    keep.iter().for_each(|&k| {
        if !k {
            findings.remove(idx);
        } else {
            idx += 1;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::types::DetectionSource;

    fn finding(t: SensitiveEntityType, start: usize, end: usize, val: &str) -> SensitiveFinding {
        SensitiveFinding {
            entity_type: t,
            value: val.to_string(),
            start,
            end,
            confidence: 0.9,
            source: DetectionSource::Regex,
        }
    }

    #[test]
    fn test_no_overlap_preserved() {
        let mut findings = vec![
            finding(SensitiveEntityType::Email, 0, 15, "a@b.com"),
            finding(SensitiveEntityType::Phone, 20, 34, "555-1234"),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn test_database_url_wins_over_url_and_hostname() {
        let mut findings = vec![
            finding(
                SensitiveEntityType::Url,
                0,
                50,
                "postgres://admin:secret@host:5432/db",
            ),
            finding(SensitiveEntityType::Hostname, 23, 27, "host"),
            finding(
                SensitiveEntityType::DatabaseUrl,
                0,
                50,
                "postgres://admin:secret@host:5432/db",
            ),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].entity_type, SensitiveEntityType::DatabaseUrl);
    }

    #[test]
    fn test_longer_span_preferred_at_equal_priority() {
        let mut findings = vec![
            finding(SensitiveEntityType::Email, 0, 10, "short@a.co"),
            finding(SensitiveEntityType::Email, 0, 20, "longername@domain.com"),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].value, "longername@domain.com");
    }
}
