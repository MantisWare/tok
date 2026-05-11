//! Idempotent header injection, replacement, and structural-only refresh.
//!
//! Implements the critical invariants from FORGEMAP.md §19:
//! - R1: CRLF normalization before any line splitting
//! - R4: Never degrade (`exports`/`used_by` only rewritten if the new value is non-empty)
//! - R7: Idempotent injection — never duplicate headers
//! - R12: Atomic writes (caller responsibility via `safe_write_file`)

use super::constants::comment_prefix_for_ext;
use super::fmt::{fmt_exports, fmt_used_by};
use super::header::{parse_header, rebuild_header};
use super::scan::normalize_lf;
use super::types::RefreshFileResult;
use std::collections::BTreeMap;

/// Inject a header into a source file for the first time.
///
/// - Normalizes CRLF → LF.
/// - If a header already exists, returns source unchanged (idempotent).
/// - Preserves shebang on line 0.
/// - Ensures exactly one blank line between header and first code.
pub fn inject_header(source: &str, new_header: &str, ext: &str) -> String {
    let source = normalize_lf(source);

    // Already has a header? Return unchanged.
    if parse_header(&source, ext).is_some() {
        return source;
    }

    let lines: Vec<&str> = source.lines().collect();
    let mut result = String::new();

    // Preserve shebang.
    let code_start = if !lines.is_empty() && lines[0].starts_with("#!") {
        result.push_str(lines[0]);
        result.push('\n');
        1
    } else {
        0
    };

    result.push_str(new_header);
    result.push('\n');
    result.push('\n');

    // Skip leading blank lines after shebang (avoid double blanks).
    let mut i = code_start;
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }

    // Append remaining code.
    let mut first = true;
    for line in &lines[i..] {
        if !first {
            result.push('\n');
        }
        result.push_str(line);
        first = false;
    }

    // Preserve trailing newline if original had one.
    if source.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }

    result
}

/// Force-replace an existing header with a new one.
///
/// Used by `--force` mode. If no existing header, injects normally.
pub fn replace_header(source: &str, new_header: &str, ext: &str) -> String {
    let source = normalize_lf(source);

    let parsed = match parse_header(&source, ext) {
        Some(p) => p,
        None => return inject_header(&source, new_header, ext),
    };

    let lines: Vec<&str> = source.lines().collect();
    let mut result = String::new();

    // Preserve shebang.
    if parsed.start_line > 0 {
        for line in &lines[..parsed.start_line] {
            result.push_str(line);
            result.push('\n');
        }
    }

    result.push_str(new_header);
    result.push('\n');
    result.push('\n');

    // Skip old header and any blank lines right after it.
    let mut i = parsed.end_line + 1;
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }

    let mut first = true;
    for line in &lines[i..] {
        if !first {
            result.push('\n');
        }
        result.push_str(line);
        first = false;
    }

    if source.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }

    result
}

/// Structural-only refresh: update `exports:` and `used_by:` without touching
/// `rules:`, `agent:`, `message:`, etc.
///
/// Implements the "never degrade" rule (R4): if the new value resolves to "none"
/// but the existing value has real content, the existing value is preserved.
pub fn refresh_header(
    source: &str,
    new_exports: &[String],
    new_used_by: &BTreeMap<String, Vec<String>>,
    ext: &str,
) -> RefreshFileResult {
    let source = normalize_lf(source);
    let prefix = comment_prefix_for_ext(ext);

    let parsed = match parse_header(&source, ext) {
        Some(p) => p,
        None => {
            return RefreshFileResult {
                source,
                changed: false,
                changed_fields: Vec::new(),
            };
        }
    };

    let fmt_new_exports = fmt_exports(new_exports);
    let fmt_new_used_by = fmt_used_by(new_used_by, prefix);

    // Never-degrade rule (R4).
    let final_exports = if fmt_new_exports == "none"
        && !parsed.exports.trim().is_empty()
        && parsed.exports.trim().to_lowercase() != "none"
    {
        parsed.exports.clone()
    } else {
        fmt_new_exports.clone()
    };

    let final_used_by = if fmt_new_used_by == "none"
        && !parsed.used_by.trim().is_empty()
        && parsed.used_by.trim().to_lowercase() != "none"
    {
        parsed.used_by.clone()
    } else {
        fmt_new_used_by.clone()
    };

    // Check if anything actually changed.
    let mut changed_fields = Vec::new();
    if final_exports != parsed.exports {
        changed_fields.push("exports".to_string());
    }
    if final_used_by != parsed.used_by {
        changed_fields.push("used_by".to_string());
    }

    if changed_fields.is_empty() {
        return RefreshFileResult {
            source,
            changed: false,
            changed_fields: Vec::new(),
        };
    }

    let new_header = rebuild_header(&parsed, &final_exports, &final_used_by, ext);

    let lines: Vec<&str> = source.lines().collect();
    let mut result = String::new();

    // Lines before header.
    for line in &lines[..parsed.start_line] {
        result.push_str(line);
        result.push('\n');
    }

    result.push_str(&new_header);
    result.push('\n');

    // Lines after header (skip old header lines).
    let after_start = parsed.end_line + 1;
    if after_start < lines.len() {
        for line in &lines[after_start..] {
            result.push('\n');
            result.push_str(line);
        }
    }

    if source.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }

    RefreshFileResult {
        source: result,
        changed: true,
        changed_fields,
    }
}

/// Write a file atomically: write to a temp file, then rename (R12).
pub fn safe_write_file(path: &std::path::Path, content: &str) -> anyhow::Result<()> {
    let tmp_path = path.with_extension("forgemap-tmp");
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}
