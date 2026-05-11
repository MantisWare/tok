//! Formatting helpers for ForgeMap headers and manifests.

use chrono::Utc;

use super::constants::EXPORTS_CAP;

/// Format exports list for a header line. Pipe-separated, capped at `EXPORTS_CAP`.
pub fn fmt_exports(exports: &[String]) -> String {
    if exports.is_empty() {
        return "none".to_string();
    }
    if exports.len() <= EXPORTS_CAP {
        return exports.join(" | ");
    }
    let truncated: Vec<&str> = exports
        .iter()
        .take(EXPORTS_CAP)
        .map(|s| s.as_str())
        .collect();
    let remaining = exports.len() - EXPORTS_CAP;
    format!("{} (+{} more)", truncated.join(" | "), remaining)
}

/// Format `used_by` map for a header block.
///
/// Multi-line, continuation indented to align with the `used_by:` value column
/// (9 spaces for `//` prefix languages).
pub fn fmt_used_by(ub: &std::collections::BTreeMap<String, Vec<String>>, prefix: &str) -> String {
    if ub.is_empty() {
        return "none".to_string();
    }

    let indent = format!("{}          ", prefix);
    let mut lines = Vec::new();

    for (importer, syms) in ub {
        let sym_part = if syms.is_empty() {
            String::new()
        } else {
            format!(" → {}", syms.join(", "))
        };
        lines.push(format!("{}{}", importer, sym_part));
    }

    if lines.len() == 1 {
        return lines[0].clone();
    }

    let first = lines[0].clone();
    let rest: Vec<String> = lines[1..]
        .iter()
        .map(|l| format!("{}{}", indent, l))
        .collect();

    let mut result = first;
    for r in rest {
        result.push('\n');
        result.push_str(&r);
    }
    result
}

/// Derive provider string from a model ID.
pub fn detect_provider(model_id: &str) -> String {
    let lower = model_id.to_lowercase();
    if lower.contains("forgemap-cli") || lower.contains("codedna-cli") || lower.contains("no-llm") {
        return "forgemap-cli".to_string();
    }
    if lower.starts_with("ollama/") || lower.starts_with("ollama-") {
        return "ollama".to_string();
    }
    if lower.starts_with("gpt") || lower.starts_with("o1") || lower.starts_with("o3") {
        return "openai".to_string();
    }
    if lower.starts_with("claude") {
        return "anthropic".to_string();
    }
    if lower.starts_with("gemini") {
        return "gemini".to_string();
    }
    if lower.starts_with("deepseek") {
        return "deepseek".to_string();
    }
    "unknown".to_string()
}

/// Generate a unique session ID: `s_YYYYMMDD_<hex6>`.
pub fn gen_session_id() -> String {
    let date = Utc::now().format("%Y%m%d");
    let hex: String = format!("{:06x}", rand_u32() & 0x00FF_FFFF);
    format!("s_{}_{}", date, hex)
}

/// Truncate a purpose string to at most 15 words.
pub fn truncate_purpose(purpose: &str) -> String {
    let words: Vec<&str> = purpose.split_whitespace().collect();
    if words.len() <= 15 {
        return words.join(" ");
    }
    words[..15].join(" ")
}

/// Heuristic purpose from a file's relative path.
pub fn file_purpose_heuristic(rel: &str) -> String {
    let basename = std::path::Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let cleaned = basename.replace(['-', '_'], " ").trim().to_string();

    if cleaned.is_empty() || cleaned == "index" || cleaned == "mod" {
        let parent = std::path::Path::new(rel)
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("root");
        return format!("{} module", parent);
    }

    format!("{} module", cleaned)
}

/// Heuristic purpose for a package key.
pub fn package_purpose_heuristic(pkg_key: &str, files: &[String]) -> String {
    let stems: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let s = std::path::Path::new(f).file_stem()?.to_str()?.to_string();
            if s == "index" || s == "mod" || s.starts_with('_') {
                return None;
            }
            Some(s.replace(['-', '_'], " "))
        })
        .take(3)
        .collect();

    if stems.is_empty() {
        let key_label = if pkg_key.is_empty() { "root" } else { pkg_key };
        return format!("{} package", key_label);
    }
    format!("{} module", stems.join(", "))
}

/// Simple pseudo-random u32 using `getrandom`.
fn rand_u32() -> u32 {
    let mut buf = [0u8; 4];
    let _ = getrandom::fill(&mut buf);
    u32::from_le_bytes(buf)
}
