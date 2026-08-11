//! Merging regenerated content into files a human may have edited.
//!
//! The markdown layer writes files that people are explicitly invited to
//! annotate. That makes regeneration a data-loss risk: the naive implementation
//! overwrites the file and silently destroys whatever a developer wrote in it,
//! and they only find out later when they go looking for the note.
//!
//! So generated content is fenced by markers:
//!
//! ```text
//! <!-- tok:generated:start -->
//! ...replaced on every run...
//! <!-- tok:generated:end -->
//!
//! ## Notes
//! Anything here survives. This is the point.
//! ```
//!
//! [`merge`] replaces only the fenced region and leaves every other byte of the
//! file alone. The failure modes are handled explicitly rather than optimised
//! away, because each one corresponds to a real edit a person might make:
//!
//! - **No markers** (someone deleted them, or the file predates them): the file
//!   is treated as entirely hand-written and the generated block is prepended
//!   rather than dropped, so nothing is lost in either direction.
//! - **Start without end**, or **end before start**: the file is damaged, and
//!   guessing where the block ends could delete prose. The merge refuses and
//!   reports, leaving the file untouched.
//! - **Repeated markers**: only the first pair is treated as the generated
//!   block; a second pair is almost certainly quoted documentation.

use std::fmt;

pub const START_MARKER: &str = "<!-- tok:generated:start -->";
pub const END_MARKER: &str = "<!-- tok:generated:end -->";

/// The default trailing section, written once and never rewritten.
pub const NOTES_HEADING: &str = "## Notes";

/// Why a merge could not be performed safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeError {
    /// A start marker with no matching end.
    UnterminatedBlock,
    /// An end marker appearing before its start.
    MisorderedMarkers,
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MergeError::UnterminatedBlock => write!(
                f,
                "generated block starts but never ends; refusing to guess where it stops"
            ),
            MergeError::MisorderedMarkers => write!(
                f,
                "generated end marker precedes its start marker; file is damaged"
            ),
        }
    }
}

impl std::error::Error for MergeError {}

/// Wrap content in generated markers.
pub fn wrap(generated: &str) -> String {
    format!(
        "{START_MARKER}\n{}\n{END_MARKER}",
        generated.trim_end_matches('\n')
    )
}

/// Produce the full contents for a file that does not exist yet.
pub fn initial(generated: &str) -> String {
    format!(
        "{}\n\n{NOTES_HEADING}\n\n_Anything you write below is preserved when this file is regenerated._\n",
        wrap(generated)
    )
}

/// Replace the generated region of `existing` with `generated`, preserving
/// everything outside it.
pub fn merge(existing: &str, generated: &str) -> Result<String, MergeError> {
    let start = existing.find(START_MARKER);
    let end = existing.find(END_MARKER);

    match (start, end) {
        (Some(start), Some(end)) => {
            if end < start {
                return Err(MergeError::MisorderedMarkers);
            }

            let after = end + END_MARKER.len();
            Ok(format!(
                "{}{}{}",
                &existing[..start],
                wrap(generated),
                &existing[after..]
            ))
        }

        (Some(_), None) => Err(MergeError::UnterminatedBlock),

        // An end marker alone is indistinguishable from prose that happens to
        // quote it, so treat the file as hand-written.
        (None, _) => Ok(format!("{}\n\n{}", wrap(generated), existing.trim_start())),
    }
}

/// Merge into a file's contents, or produce initial contents when absent.
pub fn merge_or_initial(existing: Option<&str>, generated: &str) -> Result<String, MergeError> {
    match existing {
        Some(existing) if !existing.trim().is_empty() => merge(existing, generated),
        _ => Ok(initial(generated)),
    }
}

/// Compose a full document: frontmatter, then the generated block, then
/// whatever a human wrote.
///
/// Frontmatter is handled separately from the generated block rather than
/// nested inside it, because YAML frontmatter is only recognised when it is the
/// very first thing in the file. Putting it inside the markers would render it
/// as a horizontal rule and a stray line of text in every viewer, and no
/// note-taking tool would index it.
///
/// Both regions are regenerated; only the tail is preserved.
pub fn compose(
    existing: Option<&str>,
    frontmatter: &str,
    generated: &str,
) -> Result<String, MergeError> {
    let body = existing
        .map(|document| crate::markdown::frontmatter::split(document).1)
        .filter(|body| !body.trim().is_empty());

    let merged = merge_or_initial(body, generated)?;

    Ok(format!("{frontmatter}{}", merged.trim_start()))
}

/// Extract the generated region, for tests and drift checks.
pub fn generated_section(contents: &str) -> Option<&str> {
    let start = contents.find(START_MARKER)? + START_MARKER.len();
    let end = contents.find(END_MARKER)?;

    if end < start {
        return None;
    }

    Some(contents[start..end].trim_matches('\n'))
}

/// Everything outside the generated region: the human's contribution.
pub fn preserved_section(contents: &str) -> String {
    let Some(start) = contents.find(START_MARKER) else {
        return contents.to_string();
    };
    let Some(end) = contents.find(END_MARKER) else {
        return contents.to_string();
    };
    if end < start {
        return contents.to_string();
    }

    let after = end + END_MARKER.len();
    format!("{}{}", &contents[..start], &contents[after..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_file_gets_a_block_and_a_notes_section() {
        let contents = initial("generated body");

        assert!(contents.contains(START_MARKER));
        assert!(contents.contains(END_MARKER));
        assert!(contents.contains("generated body"));
        assert!(contents.contains(NOTES_HEADING));
    }

    /// The whole reason the marker scheme exists.
    #[test]
    fn human_notes_survive_regeneration() {
        let existing = initial("old body").replace(
            "_Anything you write below is preserved when this file is regenerated._",
            "This module is load-bearing for billing. Ask Priya before changing it.",
        );

        let merged = merge(&existing, "new body").expect("merge");

        assert!(merged.contains("Ask Priya before changing it."));
        assert!(merged.contains("new body"));
        assert!(!merged.contains("old body"));
    }

    #[test]
    fn content_before_the_block_is_preserved_too() {
        let existing = format!("# Hand-written title\n\n{}\n\ntail", wrap("old"));

        let merged = merge(&existing, "new").expect("merge");

        assert!(merged.starts_with("# Hand-written title"));
        assert!(merged.contains("new"));
        assert!(merged.trim_end().ends_with("tail"));
    }

    #[test]
    fn merging_twice_is_idempotent() {
        let once = merge(&initial("v1"), "v2").expect("merge");
        let twice = merge(&once, "v2").expect("merge");

        assert_eq!(once, twice);
    }

    /// Deleting the markers must not mean deleting the prose.
    #[test]
    fn a_file_with_no_markers_keeps_its_content() {
        let merged = merge("# Just my notes\n\nnothing generated here", "body").expect("merge");

        assert!(merged.contains("# Just my notes"));
        assert!(merged.contains("nothing generated here"));
        assert!(merged.contains("body"));
    }

    #[test]
    fn an_unterminated_block_is_refused_rather_than_guessed() {
        let existing = format!("{START_MARKER}\nold body\n\n## Notes\nimportant");

        let error = merge(&existing, "new").expect_err("should refuse");

        assert_eq!(error, MergeError::UnterminatedBlock);
    }

    #[test]
    fn misordered_markers_are_refused() {
        let existing = format!("{END_MARKER}\nstuff\n{START_MARKER}");

        assert_eq!(
            merge(&existing, "new").expect_err("should refuse"),
            MergeError::MisorderedMarkers
        );
    }

    #[test]
    fn only_the_first_marker_pair_is_treated_as_generated() {
        let existing = format!(
            "{}\n\n## Notes\n\nQuoting the format: {START_MARKER} example {END_MARKER}\n",
            wrap("old")
        );

        let merged = merge(&existing, "new").expect("merge");

        assert!(merged.contains("Quoting the format:"));
        assert!(merged.contains("new"));
    }

    #[test]
    fn an_empty_file_is_treated_as_absent() {
        let merged = merge_or_initial(Some("   \n  "), "body").expect("merge");

        assert!(merged.contains(NOTES_HEADING));
    }

    #[test]
    fn a_missing_file_produces_initial_contents() {
        let merged = merge_or_initial(None, "body").expect("merge");

        assert!(merged.contains(NOTES_HEADING));
        assert!(merged.contains("body"));
    }

    #[test]
    fn the_generated_section_can_be_read_back() {
        let contents = initial("body line one\nbody line two");

        assert_eq!(
            generated_section(&contents),
            Some("body line one\nbody line two")
        );
    }

    #[test]
    fn reading_a_generated_section_from_an_unmarked_file_returns_nothing() {
        assert_eq!(generated_section("no markers here"), None);
    }

    #[test]
    fn the_preserved_section_excludes_generated_content() {
        let contents = initial("generated body");

        let preserved = preserved_section(&contents);

        assert!(!preserved.contains("generated body"));
        assert!(preserved.contains(NOTES_HEADING));
    }

    #[test]
    fn an_empty_generated_body_still_round_trips() {
        let merged = merge(&initial("x"), "").expect("merge");

        assert_eq!(generated_section(&merged), Some(""));
    }

    /// Frontmatter is only recognised at the very start of a file, so it has to
    /// sit outside the generated markers.
    #[test]
    fn composed_documents_lead_with_frontmatter() {
        let composed = compose(None, "---\nkind: card\n---\n", "body").expect("compose");

        assert!(composed.starts_with("---\nkind: card\n---\n"));
        assert!(composed.contains(START_MARKER));
        assert!(composed.contains("body"));
    }

    #[test]
    fn composing_over_an_existing_document_replaces_its_frontmatter() {
        let first = compose(None, "---\nsymbols: \"1\"\n---\n", "old body").expect("first");
        let second =
            compose(Some(&first), "---\nsymbols: \"2\"\n---\n", "new body").expect("second");

        assert!(second.contains(r#"symbols: "2""#));
        assert!(!second.contains(r#"symbols: "1""#));
        assert!(second.contains("new body"));
    }

    #[test]
    fn composing_preserves_notes_beneath_the_block() {
        let first = compose(None, "---\na: b\n---\n", "old").expect("first");
        let annotated = first.replace("## Notes", "## Notes\n\nKeep me.");

        let second = compose(Some(&annotated), "---\na: b\n---\n", "new").expect("second");

        assert!(second.contains("Keep me."));
        assert!(second.contains("new"));
    }

    #[test]
    fn composing_twice_is_idempotent() {
        let once = compose(None, "---\na: b\n---\n", "body").expect("first");
        let twice = compose(Some(&once), "---\na: b\n---\n", "body").expect("second");

        assert_eq!(once, twice);
    }

    #[test]
    fn composing_with_no_frontmatter_still_works() {
        let composed = compose(None, "", "body").expect("compose");

        assert!(composed.starts_with(START_MARKER));
    }

    /// Markdown files routinely contain non-ASCII; slicing by byte offset must
    /// land on character boundaries.
    #[test]
    fn multibyte_content_does_not_panic() {
        let existing = initial("café ☕ naïve");

        let merged = merge(&existing, "résumé — em dash").expect("merge");

        assert!(merged.contains("résumé — em dash"));
    }
}
