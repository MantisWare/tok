//! ForgeMap header building, parsing, and rebuilding.
//!
//! Headers are comment blocks at the top of source files carrying exports,
//! reverse dependency info, rules, and agent provenance. The comment prefix
//! adapts per language (`//` for most, `#` for Python/Ruby).

use std::collections::BTreeMap;

use super::constants::{comment_prefix_for_ext, AGENT_WINDOW};
use super::fmt::{detect_provider, fmt_exports, fmt_used_by, truncate_purpose};
use super::types::ParsedHeader;

/// Options for building a fresh header.
pub struct BuildHeaderOpts<'a> {
    pub rel: &'a str,
    pub purpose: &'a str,
    pub exports: &'a [String],
    pub used_by: &'a BTreeMap<String, Vec<String>>,
    pub related: Option<&'a str>,
    pub wiki: Option<&'a str>,
    pub rules: &'a str,
    pub model_id: &'a str,
    pub today: &'a str,
    pub session_id: &'a str,
}

/// Build a complete ForgeMap header string.
pub fn build_header(opts: &BuildHeaderOpts<'_>) -> String {
    let ext = opts.rel.rsplit('.').next().unwrap_or("");
    let prefix = comment_prefix_for_ext(ext);
    let purpose = truncate_purpose(opts.purpose);
    let provider = detect_provider(opts.model_id);
    let exports_str = fmt_exports(opts.exports);
    let used_by_str = fmt_used_by(opts.used_by, prefix);

    let mut lines = Vec::new();

    lines.push(format!("{} {} — {}", prefix, opts.rel, purpose));
    lines.push(prefix.to_string());
    lines.push(format!("{} exports: {}", prefix, exports_str));
    lines.push(format!("{} used_by: {}", prefix, used_by_str));

    if let Some(related) = opts.related {
        if !related.is_empty() {
            lines.push(format!("{} related: {}", prefix, related));
        }
    }

    if let Some(wiki) = opts.wiki {
        if !wiki.is_empty() {
            lines.push(format!("{} wiki:    {}", prefix, wiki));
        }
    }

    let rules = if opts.rules.is_empty() {
        "none"
    } else {
        opts.rules
    };
    // Multi-line rules: first line on the `rules:` line, rest indented.
    let rule_lines: Vec<&str> = rules.lines().collect();
    lines.push(format!("{} rules:   {}", prefix, rule_lines[0]));
    let indent = format!("{}          ", prefix);
    for rl in rule_lines.iter().skip(1) {
        lines.push(format!("{}{}", indent, rl));
    }

    lines.push(format!(
        "{} agent:   {} | {} | {} | {} | initial ForgeMap annotation pass",
        prefix, opts.model_id, provider, opts.today, opts.session_id
    ));

    lines.join("\n")
}

/// Parse a ForgeMap header from source text.
///
/// Scans the first `HEADER_SCAN_LINES` lines for field markers and extracts
/// all header fields into a `ParsedHeader`.
pub fn parse_header(source: &str, ext: &str) -> Option<ParsedHeader> {
    let prefix = comment_prefix_for_ext(ext);

    let lines: Vec<&str> = source.lines().collect();
    let scan_limit = lines.len().min(super::constants::HEADER_SCAN_LINES);

    let has_field = lines[..scan_limit].iter().any(|line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with(prefix) {
            return false;
        }
        let content = trimmed[prefix.len()..].trim();
        super::constants::HEADER_FIELDS
            .iter()
            .any(|f| content.starts_with(f))
    });

    if !has_field {
        return None;
    }

    let mut start_line = 0;

    if !lines.is_empty() && lines[0].starts_with("#!") {
        start_line = 1;
    }

    while start_line < scan_limit {
        let trimmed = lines[start_line].trim();
        if trimmed.starts_with(prefix) {
            break;
        }
        if !trimmed.is_empty() {
            return None;
        }
        start_line += 1;
    }

    if start_line >= scan_limit {
        return None;
    }

    let mut end_line = start_line;
    for (i, line) in lines[start_line..scan_limit].iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(prefix) || trimmed.is_empty() {
            end_line = start_line + i;
        } else {
            break;
        }
    }

    let mut first_line = String::new();
    let mut exports = String::new();
    let mut used_by = String::new();
    let mut related: Option<String> = None;
    let mut wiki: Option<String> = None;
    let mut rules = String::new();
    let mut agent = String::new();
    let mut message: Option<String> = None;

    let mut current_field: Option<&str> = None;
    let mut is_first_comment = true;

    for line in &lines[start_line..=end_line] {
        let trimmed = line.trim();

        let content = match trimmed.strip_prefix(prefix) {
            Some(c) => c.trim_start(),
            None => continue,
        };

        let mut found_field = false;
        for &field in super::constants::HEADER_FIELDS {
            if let Some(rest) = content.strip_prefix(field) {
                let value = rest.trim().to_string();
                match field {
                    "exports:" => {
                        exports = value;
                        current_field = Some("exports");
                    }
                    "used_by:" => {
                        used_by = value;
                        current_field = Some("used_by");
                    }
                    "related:" => {
                        related = Some(value);
                        current_field = Some("related");
                    }
                    "wiki:" => {
                        wiki = Some(value);
                        current_field = Some("wiki");
                    }
                    "rules:" => {
                        rules = value;
                        current_field = Some("rules");
                    }
                    "agent:" => {
                        if agent.is_empty() {
                            agent = value;
                        } else {
                            agent.push('\n');
                            agent.push_str(&value);
                        }
                        current_field = Some("agent");
                    }
                    "message:" => {
                        message = Some(value);
                        current_field = Some("message");
                    }
                    _ => {}
                }
                found_field = true;
                break;
            }
        }

        if found_field {
            is_first_comment = false;
            continue;
        }

        if is_first_comment && !content.is_empty() {
            first_line = content.to_string();
            is_first_comment = false;
            continue;
        }

        if let Some(field) = current_field {
            if !content.is_empty() {
                let cont = content.to_string();
                match field {
                    "used_by" => {
                        used_by.push('\n');
                        used_by.push_str(&cont);
                    }
                    "rules" => {
                        rules.push('\n');
                        rules.push_str(&cont);
                    }
                    "agent" => {
                        agent.push('\n');
                        agent.push_str(&cont);
                    }
                    "related" => {
                        if let Some(ref mut r) = related {
                            r.push('\n');
                            r.push_str(&cont);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if exports.is_empty() && used_by.is_empty() && rules.is_empty() && agent.is_empty() {
        return None;
    }

    Some(ParsedHeader {
        first_line,
        exports,
        used_by,
        related,
        wiki,
        rules,
        agent,
        message,
        start_line,
        end_line,
    })
}

/// Rebuild a header with new structural fields (`exports`, `used_by`) while
/// preserving all other fields (`related`, `wiki`, `rules`, `agent`, `message`) verbatim.
pub fn rebuild_header(
    parsed: &ParsedHeader,
    new_exports: &str,
    new_used_by: &str,
    ext: &str,
) -> String {
    let prefix = comment_prefix_for_ext(ext);

    let mut lines = Vec::new();

    lines.push(format!("{} {}", prefix, parsed.first_line));
    lines.push(prefix.to_string());
    lines.push(format!("{} exports: {}", prefix, new_exports));

    // Multi-line used_by.
    let ub_lines: Vec<&str> = new_used_by.lines().collect();
    if ub_lines.is_empty() {
        lines.push(format!("{} used_by: none", prefix));
    } else {
        lines.push(format!("{} used_by: {}", prefix, ub_lines[0]));
        let indent = format!("{}          ", prefix);
        for ul in ub_lines.iter().skip(1) {
            lines.push(format!("{}{}", indent, ul));
        }
    }

    if let Some(ref related) = parsed.related {
        let rl: Vec<&str> = related.lines().collect();
        lines.push(format!("{} related: {}", prefix, rl[0]));
        let indent = format!("{}          ", prefix);
        for r in rl.iter().skip(1) {
            lines.push(format!("{}{}", indent, r));
        }
    }

    if let Some(ref wiki) = parsed.wiki {
        lines.push(format!("{} wiki:    {}", prefix, wiki));
    }

    let rule_lines: Vec<&str> = parsed.rules.lines().collect();
    if rule_lines.is_empty() {
        lines.push(format!("{} rules:   none", prefix));
    } else {
        lines.push(format!("{} rules:   {}", prefix, rule_lines[0]));
        let indent = format!("{}          ", prefix);
        for rl in rule_lines.iter().skip(1) {
            lines.push(format!("{}{}", indent, rl));
        }
    }

    let all_agent_lines: Vec<&str> = parsed.agent.lines().collect();
    let agent_lines = if all_agent_lines.len() > AGENT_WINDOW {
        &all_agent_lines[all_agent_lines.len() - AGENT_WINDOW..]
    } else {
        &all_agent_lines[..]
    };
    if agent_lines.is_empty() {
        lines.push(format!("{} agent:   unknown", prefix));
    } else {
        lines.push(format!("{} agent:   {}", prefix, agent_lines[0]));
        let indent = format!("{}          ", prefix);
        for al in agent_lines.iter().skip(1) {
            lines.push(format!("{}{}", indent, al));
        }
    }

    if let Some(ref msg) = parsed.message {
        let indent = format!("{}          ", prefix);
        lines.push(format!("{}{}message: {:?}", indent, "", msg));
    }

    lines.join("\n")
}
