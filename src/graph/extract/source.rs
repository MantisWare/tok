//! Byte-safe access to source text.
//!
//! Tree-sitter reports byte offsets, but Rust panics when a string is sliced on
//! a non-char boundary. The release profile sets `panic = "abort"`, so a single
//! multi-byte character in a source file — a Greek identifier, a CJK string
//! literal, an emoji in a comment — could take down the whole process rather
//! than skipping one file. Everything here clamps instead of panicking.

/// Longest signature retained. Long enough for a real generic signature, short
/// enough that a pathological one-line minified file cannot blow up output.
const MAX_SIGNATURE_LEN: usize = 200;

/// Slice `src` by byte range, snapping to the nearest valid char boundaries.
///
/// Out-of-range and inverted inputs yield an empty string rather than panicking.
pub fn slice(src: &str, start: usize, end: usize) -> &str {
    if start >= end || start >= src.len() {
        return "";
    }
    let start = floor_boundary(src, start);
    let end = ceil_boundary(src, end.min(src.len()));
    src.get(start..end).unwrap_or("")
}

/// Largest char boundary at or below `idx`.
fn floor_boundary(src: &str, mut idx: usize) -> usize {
    if idx > src.len() {
        return src.len();
    }
    while idx > 0 && !src.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Smallest char boundary at or above `idx`.
fn ceil_boundary(src: &str, mut idx: usize) -> usize {
    let len = src.len();
    if idx >= len {
        return len;
    }
    while idx < len && !src.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

/// Condense a declaration into a one-line signature.
///
/// Takes text up to the body opener so that `fn f(a: u32) -> u32 { ... }`
/// becomes `fn f(a: u32) -> u32`, collapses internal whitespace, and truncates
/// with an ellipsis. Signatures are shown to agents, so compactness is the
/// whole point.
pub fn signature_from(src: &str, start: usize, end: usize) -> String {
    let text = slice(src, start, end);

    // Stop at the body so multi-line function bodies never reach the output.
    let head = text.find(['{', '\n']).map(|i| &text[..i]).unwrap_or(text);

    let condensed = collapse_whitespace(head);
    truncate_chars(&condensed, MAX_SIGNATURE_LEN)
}

/// Collapse every run of whitespace to a single space and trim the ends.
pub fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            in_space = true;
            continue;
        }
        if in_space && !out.is_empty() {
            out.push(' ');
        }
        in_space = false;
        out.push(ch);
    }
    out
}

/// Truncate to `max` characters (not bytes), appending an ellipsis when cut.
pub fn truncate_chars(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Extract a doc comment immediately preceding `byte_offset`.
///
/// Walks backwards over contiguous comment lines, so only comments genuinely
/// attached to the declaration are picked up — a blank line or any code between
/// the comment and the declaration ends the scan.
pub fn doc_comment_before(src: &str, byte_offset: usize) -> Option<String> {
    let head = slice(src, 0, byte_offset);
    let mut lines: Vec<&str> = Vec::new();

    for line in head.lines().rev() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            // A blank line detaches the comment from the declaration.
            break;
        }

        let content = strip_comment_marker(trimmed);
        match content {
            Some(text) => lines.push(text),
            None => break,
        }
    }

    if lines.is_empty() {
        return None;
    }

    lines.reverse();
    let joined = collapse_whitespace(&lines.join(" "));
    if joined.is_empty() {
        None
    } else {
        Some(truncate_chars(&joined, MAX_SIGNATURE_LEN))
    }
}

/// Strip a leading comment marker, returning the comment body.
///
/// `None` means the line is not a comment, which terminates a doc scan.
fn strip_comment_marker(line: &str) -> Option<&str> {
    // Order matters: `///` and `//!` must be tested before `//`.
    for marker in ["///", "//!", "//", "#", "*/", "*", "/**", "/*"] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest.trim());
        }
    }
    // A Python docstring delimiter on its own line.
    if line == "\"\"\"" || line == "'''" {
        return Some("");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_ascii_normally() {
        assert_eq!(slice("hello world", 0, 5), "hello");
        assert_eq!(slice("hello world", 6, 11), "world");
    }

    /// The case that would abort the process without boundary clamping.
    #[test]
    fn slicing_inside_a_multibyte_char_does_not_panic() {
        let src = "let s = \"日本語\";";
        // Every offset, including ones landing mid-character.
        for start in 0..=src.len() {
            for end in start..=src.len() {
                let _ = slice(src, start, end);
            }
        }
    }

    #[test]
    fn emoji_and_cjk_slices_stay_valid_utf8() {
        let src = "// 🎉 done\nfn f() {}";
        let out = slice(src, 3, 8);
        assert!(src.contains(out));
    }

    #[test]
    fn out_of_range_slices_are_empty_not_panics() {
        assert_eq!(slice("abc", 10, 20), "");
        assert_eq!(slice("abc", 2, 1), "");
        assert_eq!(slice("", 0, 5), "");
        assert_eq!(slice("abc", 0, 999), "abc");
    }

    #[test]
    fn signature_stops_at_the_body() {
        let src = "pub fn warm(store: &mut S, keys: &[String]) -> usize {\n  0\n}";
        assert_eq!(
            signature_from(src, 0, src.len()),
            "pub fn warm(store: &mut S, keys: &[String]) -> usize"
        );
    }

    #[test]
    fn signature_collapses_multiline_parameters() {
        let src = "function f(\n  a: number,\n  b: string\n) {}";
        // Stops at the first newline, which is the honest one-line form.
        assert_eq!(signature_from(src, 0, src.len()), "function f(");
    }

    #[test]
    fn signature_is_truncated_by_characters_not_bytes() {
        let long = format!("fn f({})", "日".repeat(500));
        let sig = signature_from(&long, 0, long.len());
        assert!(sig.chars().count() <= MAX_SIGNATURE_LEN);
        assert!(sig.ends_with('…'));
    }

    #[test]
    fn collapses_runs_of_whitespace() {
        assert_eq!(collapse_whitespace("  a \n\t b  "), "a b");
        assert_eq!(collapse_whitespace(""), "");
        assert_eq!(collapse_whitespace("   "), "");
    }

    #[test]
    fn reads_rust_doc_comments() {
        let src = "/// A cache entry.\n/// Second line.\npub struct Entry {}";
        let offset = src.find("pub struct").unwrap();
        assert_eq!(
            doc_comment_before(src, offset).as_deref(),
            Some("A cache entry. Second line.")
        );
    }

    #[test]
    fn reads_hash_comments() {
        let src = "# Helper module.\ndef f():\n    pass\n";
        let offset = src.find("def f").unwrap();
        assert_eq!(
            doc_comment_before(src, offset).as_deref(),
            Some("Helper module.")
        );
    }

    #[test]
    fn blank_line_detaches_the_comment() {
        let src = "// unrelated note\n\npub struct Entry {}";
        let offset = src.find("pub struct").unwrap();
        assert_eq!(doc_comment_before(src, offset), None);
    }

    #[test]
    fn code_above_detaches_the_comment() {
        let src = "let x = 1;\npub struct Entry {}";
        let offset = src.find("pub struct").unwrap();
        assert_eq!(doc_comment_before(src, offset), None);
    }

    #[test]
    fn no_comment_yields_none() {
        assert_eq!(doc_comment_before("pub fn f() {}", 0), None);
    }
}
