//! Token savings accounting for the graph-backed retrieval commands.
//!
//! Every other TOK filter measures savings against the raw command it replaced:
//! `tok git log` against `git log`. Retrieval has no such counterpart, because
//! the thing it replaces is not a command at all — it is an agent opening files
//! until it finds what it needs.
//!
//! So the baseline is exactly that: the bytes of the source files a result
//! touches. `tok mem ask "how does auth work"` returns twenty signature lines
//! drawn from six files; without it the agent reads those six files whole. That
//! comparison is conservative in the direction that matters, since it assumes
//! the agent guesses the right files first try and never reads a wrong one.
//!
//! Files that cannot be read count as nothing rather than as an estimate. An
//! overstated saving is worse than a missing one: the number exists to be
//! trusted.

use std::collections::BTreeSet;
use std::path::Path;

use crate::core::tracking::{self, estimate_tokens};

/// A retrieval command's savings, measured and reported.
pub struct Savings {
    timer: tracking::TimedExecution,
    root: std::path::PathBuf,
}

impl Savings {
    /// Start measuring. Call [`record`](Self::record) once the output is known.
    pub fn start(root: impl AsRef<Path>) -> Self {
        Self {
            timer: tracking::TimedExecution::start(),
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Record what this command cost against what reading the files would have.
    ///
    /// `files` are the repo-relative paths the output points at; duplicates are
    /// fine and are counted once.
    pub fn record<'a>(
        self,
        tok_command: &str,
        files: impl IntoIterator<Item = &'a str>,
        output: &str,
    ) -> Report {
        let (report, baseline) = self.measure(files, output);

        self.timer.track(
            &format!("read {} files", report.files_read),
            tok_command,
            &baseline,
            output,
        );

        report
    }

    /// The measurement on its own, without recording it.
    ///
    /// Split out so it can be tested without writing to the tracking database:
    /// that database is selected by a process-global variable, and a unit test
    /// that writes to it lands its rows in whichever other test happens to own
    /// that variable at the time.
    fn measure<'a>(
        &self,
        files: impl IntoIterator<Item = &'a str>,
        output: &str,
    ) -> (Report, String) {
        let unique: BTreeSet<&str> = files.into_iter().collect();
        let baseline = self.read_all(&unique);

        let report = Report {
            files_read: unique.len(),
            baseline_tokens: estimate_tokens(&baseline),
            output_tokens: estimate_tokens(output),
        };

        (report, baseline)
    }

    /// Concatenate the source of every file a result touched.
    ///
    /// Read rather than stat'd because token count follows content, and a file
    /// that has been deleted since indexing should contribute nothing rather
    /// than its stale recorded size.
    fn read_all(&self, files: &BTreeSet<&str>) -> String {
        let mut combined = String::new();

        for file in files {
            if let Ok(contents) = std::fs::read_to_string(self.root.join(file)) {
                combined.push_str(&contents);
            }
        }

        combined
    }
}

/// What a retrieval command cost, and what it replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub files_read: usize,
    /// Tokens an agent would have spent reading those files whole.
    pub baseline_tokens: usize,
    pub output_tokens: usize,
}

impl Report {
    /// Percentage saved, or `None` when there is nothing meaningful to compare.
    ///
    /// Returns `None` rather than zero when the output is no smaller than the
    /// source, so a command that genuinely saved nothing prints no footer
    /// instead of an apologetic one.
    pub fn percent(&self) -> Option<u32> {
        if self.baseline_tokens == 0 || self.output_tokens >= self.baseline_tokens {
            return None;
        }

        let saved = self.baseline_tokens - self.output_tokens;
        Some(((saved as f64 / self.baseline_tokens as f64) * 100.0).round() as u32)
    }

    /// The footer line, or `None` when there is no honest saving to report.
    pub fn footer(&self) -> Option<String> {
        let percent = self.percent()?;

        Some(format!(
            "{} tokens vs ~{} reading {} file{} ({}% saved)",
            self.output_tokens,
            self.baseline_tokens,
            self.files_read,
            if self.files_read == 1 { "" } else { "s" },
            percent
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    /// Measure without recording — see [`Savings::measure`].
    fn measured<'a>(root: &std::path::Path, files: impl IntoIterator<Item = &'a str>) -> Report {
        Savings::start(root).measure(files, "out").0
    }

    fn report(baseline: usize, output: usize, files: usize) -> Report {
        Report {
            files_read: files,
            baseline_tokens: baseline,
            output_tokens: output,
        }
    }

    #[test]
    fn savings_are_measured_against_the_source_a_result_points_at() {
        let dir = temp();
        std::fs::write(dir.path().join("a.rs"), "x".repeat(4000)).expect("write");

        let report = Savings::start(dir.path())
            .measure(["a.rs"], "short output")
            .0;

        assert_eq!(report.files_read, 1);
        assert_eq!(report.baseline_tokens, 1000);
        assert!(report.percent().unwrap() > 90);
    }

    #[test]
    fn a_file_named_twice_is_counted_once() {
        let dir = temp();
        std::fs::write(dir.path().join("a.rs"), "x".repeat(400)).expect("write");

        let report = measured(dir.path(), ["a.rs", "a.rs", "a.rs"]);

        assert_eq!(report.files_read, 1);
        assert_eq!(report.baseline_tokens, 100);
    }

    /// An overstated saving is worse than a missing one, so an unreadable file
    /// contributes nothing rather than a guess.
    #[test]
    fn an_unreadable_file_contributes_nothing() {
        let dir = temp();
        std::fs::write(dir.path().join("real.rs"), "x".repeat(400)).expect("write");

        let report = measured(dir.path(), ["real.rs", "deleted.rs"]);

        assert_eq!(report.baseline_tokens, 100);
    }

    #[test]
    fn a_result_touching_no_file_reports_nothing() {
        let report = measured(temp().path(), []);

        assert_eq!(report.baseline_tokens, 0);
        assert_eq!(report.percent(), None);
        assert_eq!(report.footer(), None);
    }

    /// A command that saved nothing should say nothing, rather than print an
    /// apologetic zero.
    #[test]
    fn output_larger_than_the_source_reports_no_saving() {
        assert_eq!(report(100, 200, 1).percent(), None);
        assert_eq!(report(100, 100, 1).percent(), None);
        assert_eq!(report(100, 200, 1).footer(), None);
    }

    #[test]
    fn the_reported_percentage_is_the_share_of_tokens_avoided() {
        assert_eq!(report(1000, 250, 3).percent(), Some(75));
        assert_eq!(report(1000, 1, 3).percent(), Some(100));
    }

    #[test]
    fn the_footer_names_what_was_compared() {
        let footer = report(1000, 250, 3).footer().expect("footer");

        assert!(footer.contains("250 tokens"), "{footer}");
        assert!(footer.contains("~1000"), "{footer}");
        assert!(footer.contains("3 files"), "{footer}");
        assert!(footer.contains("75% saved"), "{footer}");
    }

    #[test]
    fn one_file_reads_as_singular() {
        let footer = report(1000, 250, 1).footer().expect("footer");

        assert!(footer.contains("1 file "), "{footer}");
    }
}
