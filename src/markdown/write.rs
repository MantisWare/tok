//! Writing the card set to disk, merging rather than overwriting.
//!
//! Every write goes through [`crate::markdown::blocks::merge_or_initial`], so a
//! regeneration updates the generated region and leaves human notes intact. A
//! file whose markers are damaged is skipped and reported instead of being
//! repaired by guesswork — the alternative risks deleting prose that only
//! exists in that file.
//!
//! Cards for files that no longer exist are removed, but only when they carry
//! no human notes. A card someone annotated and then deleted the source for is
//! left behind with a warning, because the note may be the only record of why
//! the file went away.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::graph::store;
use crate::graph::types::GraphV1;
use crate::markdown::{blocks, cards, index, manifest, INDEX_FILE, MANIFEST_FILE};

/// What a write run did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WriteStats {
    pub written: usize,
    /// Files whose generated block was unchanged.
    pub unchanged: usize,
    pub removed: usize,
    /// Cards kept because they carry notes despite their source being gone.
    pub orphaned: Vec<String>,
    /// Files skipped because their markers are damaged.
    pub skipped: Vec<String>,
}

/// Where the markdown lives, relative to a repo root.
///
/// Deliberately *not* under `.tok/graph/`, which is gitignored: these files are
/// meant to be committed, reviewed, and annotated.
pub fn markdown_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".tok").join("map")
}

/// Generate and write the full card set.
pub fn write_all(repo_root: &Path, graph: &GraphV1) -> Result<WriteStats> {
    let dir = markdown_dir(repo_root);
    std::fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;

    let cards = cards::build_all(graph);
    let index = index::build(graph, &cards);

    let mut stats = WriteStats::default();
    let mut expected: BTreeSet<String> = BTreeSet::new();

    expected.insert(INDEX_FILE.to_string());
    write_one(
        &dir,
        INDEX_FILE,
        &index.frontmatter,
        &index.body,
        &mut stats,
    )?;

    for card in &cards {
        expected.insert(card.filename.clone());
        write_one(
            &dir,
            &card.filename,
            &card.frontmatter,
            &card.body,
            &mut stats,
        )?;
    }

    prune(&dir, &expected, &mut stats)?;

    let manifest = manifest::build(graph, &cards, &index.body);
    store::write_json(&dir.join(MANIFEST_FILE), &manifest)
        .context("Failed to write the markdown manifest")?;

    Ok(stats)
}

fn write_one(
    dir: &Path,
    filename: &str,
    frontmatter: &str,
    body: &str,
    stats: &mut WriteStats,
) -> Result<()> {
    let path = dir.join(filename);
    let existing = std::fs::read_to_string(&path).ok();

    // An unchanged generated block and unchanged frontmatter mean an unchanged
    // file; skipping the write keeps mtimes stable so watchers and build
    // systems stay quiet.
    if let Some(current) = &existing {
        let same_body = blocks::generated_section(current) == Some(body.trim_end_matches('\n'));
        let same_frontmatter = current.starts_with(frontmatter);

        if same_body && same_frontmatter {
            stats.unchanged += 1;
            return Ok(());
        }
    }

    match blocks::compose(existing.as_deref(), frontmatter, body) {
        Ok(merged) => {
            store::write_atomic(&path, merged.as_bytes())
                .with_context(|| format!("Failed to write {}", path.display()))?;
            stats.written += 1;
        }
        Err(error) => {
            stats.skipped.push(format!("{filename}: {error}"));
        }
    }

    Ok(())
}

/// Remove cards whose source is gone, keeping any that carry human notes.
fn prune(dir: &Path, expected: &BTreeSet<String>, stats: &mut WriteStats) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        if name == MANIFEST_FILE || !name.ends_with(".md") || expected.contains(&name) {
            continue;
        }

        let path = entry.path();
        let contents = std::fs::read_to_string(&path).unwrap_or_default();

        if has_notes(&contents) {
            stats.orphaned.push(name);
            continue;
        }

        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to remove {}", path.display()))?;
        stats.removed += 1;
    }

    Ok(())
}

/// Whether a card carries prose beyond the boilerplate it was created with.
///
/// Frontmatter is stripped first: it sits outside the generated markers, so a
/// naive read of the preserved section would count it as human writing and make
/// every card look annotated.
fn has_notes(contents: &str) -> bool {
    let (_, body) = crate::markdown::frontmatter::split(contents);
    let preserved = blocks::preserved_section(body);

    preserved
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| *line != blocks::NOTES_HEADING)
        .any(|line| {
            line != "_Anything you write below is preserved when this file is regenerated._"
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{FileEntryV1, NodeKind, NodeV1, Span};

    fn graph_with(paths: &[&str]) -> GraphV1 {
        let mut g = GraphV1::new("repo", "test");
        for path in paths {
            g.files.push(FileEntryV1 {
                path: (*path).to_string(),
                hash: "h".to_string(),
                size: 1,
                language: "typescript".to_string(),
                node_count: 1,
            });
            g.nodes.push(NodeV1::new(
                format!("{path}::sym"),
                NodeKind::Function,
                "sym".to_string(),
                (*path).to_string(),
                Span::new(1, 3),
            ));
        }
        g.normalize();
        g
    }

    fn read(dir: &Path, name: &str) -> String {
        std::fs::read_to_string(dir.join(name)).expect("read generated file")
    }

    #[test]
    fn a_card_and_an_index_are_written() {
        let repo = tempfile::tempdir().expect("tempdir");
        let g = graph_with(&["src/a.ts"]);

        write_all(repo.path(), &g).expect("write");
        let dir = markdown_dir(repo.path());

        assert!(dir.join(INDEX_FILE).exists());
        assert!(dir.join("src-a-ts.md").exists());
        assert!(dir.join(MANIFEST_FILE).exists());
    }

    #[test]
    fn markdown_lives_outside_the_gitignored_cache() {
        let repo = tempfile::tempdir().expect("tempdir");

        let dir = markdown_dir(repo.path());

        assert!(dir.ends_with("map"));
        assert!(!dir.to_string_lossy().contains("graph"));
    }

    /// The property the whole marker scheme exists to protect.
    #[test]
    fn regeneration_preserves_human_notes() {
        let repo = tempfile::tempdir().expect("tempdir");
        let g = graph_with(&["src/a.ts"]);
        write_all(repo.path(), &g).expect("first write");

        let dir = markdown_dir(repo.path());
        let path = dir.join("src-a-ts.md");
        let annotated = read(&dir, "src-a-ts.md")
            .replace("## Notes", "## Notes\n\nThis file owns retry policy.");
        std::fs::write(&path, annotated).expect("annotate");

        write_all(repo.path(), &g).expect("second write");

        assert!(read(&dir, "src-a-ts.md").contains("This file owns retry policy."));
    }

    #[test]
    fn an_unchanged_card_is_not_rewritten() {
        let repo = tempfile::tempdir().expect("tempdir");
        let g = graph_with(&["src/a.ts"]);

        write_all(repo.path(), &g).expect("first");
        let second = write_all(repo.path(), &g).expect("second");

        assert!(second.unchanged >= 1);
        assert_eq!(second.written, 0);
    }

    #[test]
    fn a_card_for_a_deleted_file_is_removed() {
        let repo = tempfile::tempdir().expect("tempdir");
        write_all(repo.path(), &graph_with(&["src/a.ts", "src/b.ts"])).expect("first");

        let stats = write_all(repo.path(), &graph_with(&["src/a.ts"])).expect("second");
        let dir = markdown_dir(repo.path());

        assert_eq!(stats.removed, 1);
        assert!(!dir.join("src-b-ts.md").exists());
    }

    /// A note may be the only surviving record of why a file was deleted.
    #[test]
    fn an_annotated_card_survives_its_source_being_deleted() {
        let repo = tempfile::tempdir().expect("tempdir");
        write_all(repo.path(), &graph_with(&["src/a.ts", "src/b.ts"])).expect("first");

        let dir = markdown_dir(repo.path());
        let annotated = read(&dir, "src-b-ts.md")
            .replace("## Notes", "## Notes\n\nRemoved in the 3.0 migration.");
        std::fs::write(dir.join("src-b-ts.md"), annotated).expect("annotate");

        let stats = write_all(repo.path(), &graph_with(&["src/a.ts"])).expect("second");

        assert_eq!(stats.removed, 0);
        assert_eq!(stats.orphaned, vec!["src-b-ts.md".to_string()]);
        assert!(dir.join("src-b-ts.md").exists());
    }

    #[test]
    fn a_card_with_damaged_markers_is_skipped_not_overwritten() {
        let repo = tempfile::tempdir().expect("tempdir");
        let g = graph_with(&["src/a.ts"]);
        write_all(repo.path(), &g).expect("first");

        let dir = markdown_dir(repo.path());
        let damaged = format!(
            "{}\nhalf a block\n\n## Notes\nirreplaceable\n",
            blocks::START_MARKER
        );
        std::fs::write(dir.join("src-a-ts.md"), &damaged).expect("damage");

        let stats = write_all(repo.path(), &g).expect("second");

        assert_eq!(stats.skipped.len(), 1);
        assert!(stats.skipped[0].contains("src-a-ts.md"));
        assert_eq!(read(&dir, "src-a-ts.md"), damaged);
    }

    #[test]
    fn writing_twice_produces_identical_files() {
        let repo = tempfile::tempdir().expect("tempdir");
        let g = graph_with(&["src/a.ts", "src/b.ts"]);

        write_all(repo.path(), &g).expect("first");
        let dir = markdown_dir(repo.path());
        let before = read(&dir, "src-a-ts.md");

        write_all(repo.path(), &g).expect("second");

        assert_eq!(read(&dir, "src-a-ts.md"), before);
    }

    #[test]
    fn the_manifest_covers_every_card() {
        let repo = tempfile::tempdir().expect("tempdir");
        write_all(repo.path(), &graph_with(&["src/a.ts", "src/b.ts"])).expect("write");

        let dir = markdown_dir(repo.path());
        let raw = read(&dir, MANIFEST_FILE);
        let parsed: manifest::Manifest = serde_json::from_str(&raw).expect("parse manifest");

        assert_eq!(parsed.covered_sources(), vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn an_empty_graph_still_writes_an_index() {
        let repo = tempfile::tempdir().expect("tempdir");

        write_all(repo.path(), &graph_with(&[])).expect("write");

        assert!(markdown_dir(repo.path()).join(INDEX_FILE).exists());
    }

    #[test]
    fn boilerplate_alone_does_not_count_as_notes() {
        let repo = tempfile::tempdir().expect("tempdir");
        write_all(repo.path(), &graph_with(&["src/a.ts"])).expect("first");

        let dir = markdown_dir(repo.path());
        assert!(!has_notes(&read(&dir, "src-a-ts.md")));
    }
}
