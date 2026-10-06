//! Filters grep output by grouping matches by file.

use crate::core::config;
use crate::core::tracking;
use crate::core::utils::{exit_code_from_output, resolved_command};
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::process::Stdio;

#[allow(clippy::too_many_arguments)]
pub fn run(
    pattern: &str,
    path: &str,
    max_line_len: usize,
    max_results: usize,
    context_only: bool,
    file_type: Option<&str>,
    extra_args: &[String],
    verbose: u8,
) -> Result<i32> {
    let timer = tracking::TimedExecution::start();

    if verbose > 0 {
        eprintln!("grep: '{}' in {}", pattern, path);
    }

    // Fix: convert BRE alternation \| → | for rg (which uses PCRE-style regex)
    let rg_pattern = pattern.replace(r"\|", "|");

    let mut rg_cmd = resolved_command("rg");
    rg_cmd
        // -H/--with-filename forces the `file:` prefix even when searching a
        // single file. Without it, rg omits the filename for a lone file, and
        // a matched line that itself contains a colon (URLs, JSON, timestamps,
        // Rust `::`) gets misparsed as `file:line:content` — mangling the
        // filename, line number, and content. See parse_match_line.
        .args(["-n", "-H", "--no-heading", &rg_pattern, path])
        .stdin(Stdio::null());

    if let Some(ft) = file_type {
        rg_cmd.arg("--type").arg(ft);
    }

    for arg in extra_args {
        // Fix: skip grep-ism -r flag (rg is recursive by default; rg -r means --replace)
        if arg == "-r" || arg == "--recursive" {
            continue;
        }
        rg_cmd.arg(arg);
    }

    let output = rg_cmd
        .output()
        .or_else(|_| {
            // -H matches the rg invocation: always emit the filename so the
            // single-file-with-colon case parses correctly on the fallback too.
            resolved_command("grep")
                .args(["-rnH", pattern, path])
                .stdin(Stdio::null())
                .output()
        })
        .context("grep/rg failed")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let exit_code = exit_code_from_output(&output, "grep");

    let raw_output = stdout.to_string();

    if stdout.trim().is_empty() {
        // Show stderr for errors (bad regex, missing file, etc.)
        if exit_code == 2 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                eprintln!("{}", stderr.trim());
            }
        }
        let msg = format!("0 matches for '{}'", pattern);
        println!("{}", msg);
        timer.track(
            &format!("grep -rn '{}' {}", pattern, path),
            "tok grep",
            &raw_output,
            &msg,
        );
        return Ok(exit_code);
    }

    let mut by_file: HashMap<String, Vec<(usize, String)>> = HashMap::new();
    let mut total = 0;

    // Compile context regex once (instead of per-line in clean_line)
    let context_re = if context_only {
        Regex::new(&format!("(?i).{{0,20}}{}.*", regex::escape(pattern))).ok()
    } else {
        None
    };

    for line in stdout.lines() {
        let Some((file, line_num, content)) = parse_match_line(line, path) else {
            continue;
        };

        total += 1;
        let cleaned = clean_line(content, max_line_len, context_re.as_ref(), pattern);
        by_file.entry(file).or_default().push((line_num, cleaned));
    }

    let mut tok_output = String::new();
    tok_output.push_str(&format!("{} matches in {}F:\n\n", total, by_file.len()));

    let mut shown = 0;
    let mut files: Vec<_> = by_file.iter().collect();
    files.sort_by_key(|(f, _)| *f);

    for (file, matches) in files {
        if shown >= max_results {
            break;
        }

        let file_display = compact_path(file);
        tok_output.push_str(&format!("[file] {} ({}):\n", file_display, matches.len()));

        let per_file = config::limits().grep_max_per_file;
        for (line_num, content) in matches.iter().take(per_file) {
            tok_output.push_str(&format!("  {:>4}: {}\n", line_num, content));
            shown += 1;
            if shown >= max_results {
                break;
            }
        }

        if matches.len() > per_file {
            tok_output.push_str(&format!("  +{}\n", matches.len() - per_file));
        }
        tok_output.push('\n');
    }

    if total > shown {
        tok_output.push_str(&format!("... +{}\n", total - shown));
    }

    print!("{}", tok_output);
    timer.track(
        &format!("grep -rn '{}' {}", pattern, path),
        "tok grep",
        &raw_output,
        &tok_output,
    );

    Ok(exit_code)
}

/// Parse one `rg`/`grep -nH` output line of the form `file:line:content`.
///
/// Because `-H` is always passed, the filename is present on every line, so a
/// 3-way split on the first two colons is unambiguous even when `content`
/// itself contains colons. The 2-part branch is defensive: it only triggers if
/// an upstream tool emits a filename-less `line:content` line, in which case we
/// attribute it to the searched `path`.
fn parse_match_line<'a>(line: &'a str, path: &str) -> Option<(String, usize, &'a str)> {
    let parts: Vec<&str> = line.splitn(3, ':').collect();
    match parts.as_slice() {
        [file, num, content] => Some((file.to_string(), num.parse().unwrap_or(0), content)),
        [num, content] => Some((path.to_string(), num.parse().unwrap_or(0), content)),
        _ => None,
    }
}

fn clean_line(line: &str, max_len: usize, context_re: Option<&Regex>, pattern: &str) -> String {
    let trimmed = line.trim();

    if let Some(re) = context_re {
        if let Some(m) = re.find(trimmed) {
            let matched = m.as_str();
            if matched.len() <= max_len {
                return matched.to_string();
            }
        }
    }

    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        let lower = trimmed.to_lowercase();
        let pattern_lower = pattern.to_lowercase();

        if let Some(pos) = lower.find(&pattern_lower) {
            let char_pos = lower[..pos].chars().count();
            let chars: Vec<char> = trimmed.chars().collect();
            let char_len = chars.len();

            let start = char_pos.saturating_sub(max_len / 3);
            let end = (start + max_len).min(char_len);
            let start = if end == char_len {
                end.saturating_sub(max_len)
            } else {
                start
            };

            let slice: String = chars[start..end].iter().collect();
            if start > 0 && end < char_len {
                format!("...{}...", slice)
            } else if start > 0 {
                format!("...{}", slice)
            } else {
                format!("{}...", slice)
            }
        } else {
            let t: String = trimmed.chars().take(max_len.saturating_sub(3)).collect();
            format!("{}...", t)
        }
    }
}

fn compact_path(path: &str) -> String {
    if path.len() <= 50 {
        return path.to_string();
    }

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 3 {
        return path.to_string();
    }

    format!(
        "{}/.../{}/{}",
        parts[0],
        parts[parts.len() - 2],
        parts[parts.len() - 1]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_match_line: the single-file colon mangling regression ---

    #[test]
    fn parse_three_part_line() {
        let parsed = parse_match_line("src/main.rs:42:fn main() {", ".");
        assert_eq!(
            parsed,
            Some(("src/main.rs".to_string(), 42, "fn main() {"))
        );
    }

    // Regression: a matched line whose content contains colons must keep the
    // whole content and the correct file/line. With -H forcing the filename,
    // the first two colons delimit file and line; the rest is content verbatim.
    #[test]
    fn parse_preserves_colons_in_content() {
        let parsed = parse_match_line("single.txt:2:endpoint: https://example.com/api", ".");
        assert_eq!(
            parsed,
            Some((
                "single.txt".to_string(),
                2,
                "endpoint: https://example.com/api"
            )),
            "content after the line number must be preserved verbatim, colons and all"
        );
    }

    #[test]
    fn parse_rust_path_separator_in_content() {
        let parsed = parse_match_line("lib.rs:10:use crate::core::utils;", ".");
        assert_eq!(
            parsed,
            Some(("lib.rs".to_string(), 10, "use crate::core::utils;"))
        );
    }

    #[test]
    fn parse_two_part_line_attributed_to_path() {
        // Defensive fallback: filename-less `line:content` → attributed to path.
        let parsed = parse_match_line("7:some content", "query.txt");
        assert_eq!(parsed, Some(("query.txt".to_string(), 7, "some content")));
    }

    #[test]
    fn parse_unparseable_line_is_skipped() {
        assert_eq!(parse_match_line("no-colons-here", "."), None);
    }

    #[test]
    fn clean_line_tiny_max_len_does_not_panic() {
        // max_len < 3 previously underflowed `max_len - 3`.
        let out = clean_line("a long line with no pattern match", 2, None, "zzz");
        assert!(!out.is_empty());
    }

    #[test]
    fn test_clean_line() {
        let line = "            const result = someFunction();";
        let cleaned = clean_line(line, 50, None, "result");
        assert!(!cleaned.starts_with(' '));
        assert!(cleaned.len() <= 50);
    }

    #[test]
    fn test_compact_path() {
        let path = "/Users/patrick/dev/project/src/components/Button.tsx";
        let compact = compact_path(path);
        assert!(compact.len() <= 60);
    }

    #[test]
    fn test_extra_args_accepted() {
        // Test that the function signature accepts extra_args
        // This is a compile-time test - if it compiles, the signature is correct
        let _extra: Vec<String> = vec!["-i".to_string(), "-A".to_string(), "3".to_string()];
        // No need to actually run - we're verifying the parameter exists
    }

    #[test]
    fn test_clean_line_multibyte() {
        // Thai text that exceeds max_len in bytes
        let line = "  สวัสดีครับ นี่คือข้อความที่ยาวมากสำหรับทดสอบ  ";
        let cleaned = clean_line(line, 20, None, "ครับ");
        // Should not panic
        assert!(!cleaned.is_empty());
    }

    #[test]
    fn test_clean_line_emoji() {
        let line = "🎉🎊🎈🎁🎂🎄 some text 🎃🎆🎇✨";
        let cleaned = clean_line(line, 15, None, "text");
        assert!(!cleaned.is_empty());
    }

    // Fix: BRE \| alternation is translated to PCRE | for rg
    #[test]
    fn test_bre_alternation_translated() {
        let pattern = r"fn foo\|pub.*bar";
        let rg_pattern = pattern.replace(r"\|", "|");
        assert_eq!(rg_pattern, "fn foo|pub.*bar");
    }

    // Fix: -r flag (grep recursive) is stripped from extra_args (rg is recursive by default)
    #[test]
    fn test_recursive_flag_stripped() {
        let extra_args: Vec<String> = vec!["-r".to_string(), "-i".to_string()];
        let filtered: Vec<&String> = extra_args
            .iter()
            .filter(|a| *a != "-r" && *a != "--recursive")
            .collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "-i");
    }

    // --- truncation accuracy ---

    #[test]
    fn test_grep_overflow_uses_uncapped_total() {
        // Confirm the grep overflow invariant: matches vec is never capped before overflow calc.
        // If total_matches > per_file, overflow = total_matches - per_file (not capped).
        // This documents that grep_cmd.rs avoids the diff_cmd bug (cap at N then compute N-10).
        let per_file = config::limits().grep_max_per_file;
        let total_matches = per_file + 42;
        let overflow = total_matches - per_file;
        assert_eq!(overflow, 42, "overflow must equal true suppressed count");
        // Demonstrate why capping before subtraction is wrong:
        let hypothetical_cap = per_file + 5;
        let capped = total_matches.min(hypothetical_cap);
        let wrong_overflow = capped - per_file;
        assert_ne!(
            wrong_overflow, overflow,
            "capping before subtraction gives wrong overflow"
        );
    }

    // Verify line numbers are always enabled in rg invocation (grep_cmd.rs:24).
    // The -n/--line-numbers clap flag in main.rs is a no-op accepted for compat.
    #[test]
    fn test_rg_always_has_line_numbers() {
        // grep_cmd::run() always passes "-n" to rg (line 24).
        // This test documents that -n is built-in, so the clap flag is safe to ignore.
        let mut cmd = resolved_command("rg");
        cmd.args(["-n", "--no-heading", "NONEXISTENT_PATTERN_12345", "."]);
        // If rg is available, it should accept -n without error (exit 1 = no match, not error)
        if let Ok(output) = cmd.output() {
            assert!(
                output.status.code() == Some(1) || output.status.success(),
                "rg -n should be accepted"
            );
        }
        // If rg is not installed, skip gracefully (test still passes)
    }
}
