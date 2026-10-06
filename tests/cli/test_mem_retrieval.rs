//! End-to-end coverage for the graph-backed retrieval commands.
//!
//! These run the real binary against a copy of the multi-language fixture, so
//! they exercise the whole path: auto-refresh, tree-sitter extraction,
//! reference resolution, sidecar construction, and rendering. A unit test of
//! the ranking functions cannot catch a command that never builds its graph.
//!
//! The fixture is copied to a temp directory rather than queried in place, both
//! to keep `.tok/graph/` out of the checkout and so each test starts from a
//! genuinely cold cache.

use std::path::Path;

use assert_fs::TempDir;

use super::tok_cmd;

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("create temp dir");
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("code_graph");
        copy_tree(&src, dir.path());
        Self { dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn run(&self, args: &[&str]) -> (String, i32) {
        let mut cmd = tok_cmd();
        cmd.arg("mem")
            .args(args)
            .current_dir(self.root())
            .env("TOK_MEMORY_DB_PATH", self.dir.path().join("memory.db"))
            .env("TOK_TELEMETRY_DISABLED", "1")
            .env("NO_COLOR", "1");

        let out = cmd.output().expect("run tok mem");
        (
            String::from_utf8_lossy(&out.stdout).to_string(),
            out.status.code().unwrap_or(-1),
        )
    }

    fn stdout(&self, args: &[&str]) -> String {
        self.run(args).0
    }
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).expect("read fixture") {
        let entry = entry.expect("dir entry");
        let target = to.join(entry.file_name());

        if entry.file_type().expect("file type").is_dir() {
            std::fs::create_dir_all(&target).expect("mkdir");
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

fn path_args(fx: &Fixture) -> [String; 2] {
    [
        "--path".to_string(),
        fx.root().to_string_lossy().to_string(),
    ]
}

// ---------------------------------------------------------------- ask

/// The headline promise: ask a question, get symbols, with no prior index step.
#[test]
fn ask_indexes_on_first_use_and_finds_a_symbol() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["ask", "buildCache", &args[0], &args[1]]);

    assert!(out.contains("buildCache"), "unexpected output:\n{out}");
}

#[test]
fn ask_reports_cleanly_when_nothing_matches() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let (out, code) = fx.run(&["ask", "zzzznotarealsymbol", &args[0], &args[1]]);

    assert_eq!(code, 0, "a query with no hits is not an error");
    assert!(out.contains("No symbols matched"), "output:\n{out}");
}

#[test]
fn ask_respects_its_limit() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["ask", "cache", "--limit", "3", &args[0], &args[1]]);
    let listed = out.lines().filter(|l| l.contains("] ")).count();

    assert!(listed <= 3, "expected at most 3 results, got:\n{out}");
}

#[test]
fn ask_can_narrow_to_a_path() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["ask", "cache", "--in-path", "python", &args[0], &args[1]]);

    assert!(out.contains("python/"), "output:\n{out}");
    assert!(!out.contains("go/cache.go"), "output:\n{out}");
}

/// Structural mode should surface symbols that lexical mode cannot, because
/// they share no word with the query.
///
/// `slugify` calls `normalize`, and nothing about the word "slugify" matches
/// `normalize` lexically — so `normalize` can only arrive through the graph.
#[test]
fn structural_mode_returns_more_than_lexical_mode() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let structural = fx.stdout(&["ask", "slugify", "--limit", "25", &args[0], &args[1]]);
    let lexical = fx.stdout(&[
        "ask",
        "slugify",
        "--lexical",
        "--limit",
        "25",
        &args[0],
        &args[1],
    ]);

    let count = |text: &str| text.lines().filter(|l| l.contains("] ")).count();

    assert!(
        count(&structural) > count(&lexical),
        "structural should expand the result set\nstructural:\n{structural}\nlexical:\n{lexical}"
    );
    assert!(
        structural.contains("normalize") && !lexical.contains("normalize"),
        "the callee should arrive only through the graph\nstructural:\n{structural}\nlexical:\n{lexical}"
    );
}

#[test]
fn repeated_queries_return_identical_output() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let first = fx.stdout(&["ask", "cache", &args[0], &args[1]]);
    for _ in 0..3 {
        assert_eq!(fx.stdout(&["ask", "cache", &args[0], &args[1]]), first);
    }
}

/// Auto-refresh is what makes the command usable mid-session: an agent edits a
/// file and asks about it without re-indexing.
#[test]
fn ask_sees_an_edit_without_an_explicit_reindex() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    fx.stdout(&["ask", "cache", &args[0], &args[1]]);

    std::fs::write(
        fx.root().join("ts/fresh.ts"),
        "export function brandNewSymbol(): void {}\n",
    )
    .expect("write new file");

    let out = fx.stdout(&["ask", "brandNewSymbol", &args[0], &args[1]]);

    assert!(out.contains("brandNewSymbol"), "output:\n{out}");
}

#[test]
fn pinning_the_graph_hides_later_edits() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    fx.stdout(&["ask", "cache", &args[0], &args[1]]);

    std::fs::write(
        fx.root().join("ts/pinned.ts"),
        "export function pinnedAwaySymbol(): void {}\n",
    )
    .expect("write new file");

    let mut cmd = tok_cmd();
    cmd.arg("mem")
        .args(["ask", "pinnedAwaySymbol", &args[0], &args[1]])
        .current_dir(fx.root())
        .env("TOK_GRAPH_NO_REFRESH", "1")
        .env("TOK_TELEMETRY_DISABLED", "1")
        .env("NO_COLOR", "1");
    let out = String::from_utf8_lossy(&cmd.output().expect("run").stdout).to_string();

    assert!(out.contains("No symbols matched"), "output:\n{out}");
}

// ----------------------------------------------------------- skeleton

#[test]
fn skeleton_outlines_a_file_without_its_bodies() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["skeleton", "ts/cache.ts", &args[0], &args[1]]);

    assert!(out.contains("class Cache"), "output:\n{out}");
    assert!(out.contains("get"), "output:\n{out}");
    // A body line from the fixture must not leak into the outline.
    assert!(!out.contains("return this.entries"), "output:\n{out}");
}

#[test]
fn skeleton_with_no_file_lists_what_is_indexed() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["skeleton", &args[0], &args[1]]);

    assert!(out.contains("ts/cache.ts"), "output:\n{out}");
    assert!(out.contains("go/cache.go"), "output:\n{out}");
}

#[test]
fn skeleton_of_an_unknown_file_suggests_the_listing() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let (out, code) = fx.run(&["skeleton", "does/not/exist.ts", &args[0], &args[1]]);

    assert_eq!(code, 0);
    assert!(out.contains("No symbols"), "output:\n{out}");
}

/// The point of the command is token economy, so the outline has to be
/// dramatically smaller than the file it describes.
#[test]
fn an_outline_is_much_smaller_than_its_source() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let source = std::fs::read_to_string(fx.root().join("ts/cache.ts")).expect("read fixture");
    let outline = fx.stdout(&["skeleton", "ts/cache.ts", &args[0], &args[1]]);

    assert!(
        outline.len() < source.len(),
        "outline {} bytes vs source {} bytes",
        outline.len(),
        source.len()
    );
}

// --------------------------------------------------------------- grep

#[test]
fn grep_attributes_matches_to_their_symbol() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["grep", "normalize", &args[0], &args[1]]);

    assert!(out.contains("matches in"), "output:\n{out}");
    assert!(out.contains("normalize"), "output:\n{out}");
}

#[test]
fn grep_treats_the_pattern_literally_by_default() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let (_, code) = fx.run(&["grep", "buildCache(", &args[0], &args[1]]);

    // Would be a regex parse error if the pattern were compiled as a regex.
    assert_eq!(code, 0, "an unbalanced paren must not be a regex");
}

#[test]
fn grep_regex_mode_interprets_the_pattern() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["grep", "--regex", r"func\s+\w+", &args[0], &args[1]]);

    assert!(out.contains("matches in"), "output:\n{out}");
}

#[test]
fn grep_exits_nonzero_when_nothing_matches() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let (out, code) = fx.run(&["grep", "zzzznotpresentanywhere", &args[0], &args[1]]);

    assert_eq!(code, 1, "no matches should be a nonzero exit, like grep");
    assert!(out.contains("No matches"), "output:\n{out}");
}

#[test]
fn grep_can_narrow_to_a_path() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["grep", "cache", "--in-path", "go/", &args[0], &args[1]]);

    assert!(!out.contains("python/"), "output:\n{out}");
}

// ---------------------------------------------------------------- map

#[test]
fn map_summarizes_the_repository() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["map", &args[0], &args[1]]);

    assert!(out.contains("files"), "output:\n{out}");
    assert!(out.contains("symbols"), "output:\n{out}");
    assert!(out.contains("Layout"), "output:\n{out}");
}

#[test]
fn map_reports_every_fixture_language() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["map", &args[0], &args[1]]);

    for language in ["typescript", "python", "go", "rust"] {
        assert!(out.contains(language), "missing {language} in:\n{out}");
    }
}

#[test]
fn map_limits_are_honoured() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&[
        "map",
        "--max-dirs",
        "2",
        "--max-hubs",
        "2",
        &args[0],
        &args[1],
    ]);

    let layout_lines = out
        .lines()
        .skip_while(|l| !l.contains("Layout"))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .count();

    assert!(layout_lines <= 2, "output:\n{out}");
}

#[test]
fn map_output_is_stable_across_runs() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let first = fx.stdout(&["map", &args[0], &args[1]]);
    for _ in 0..3 {
        assert_eq!(fx.stdout(&["map", &args[0], &args[1]]), first);
    }
}

// ------------------------------------------------------- cards / check

fn card_path(fx: &Fixture, name: &str) -> std::path::PathBuf {
    fx.root().join(".tok").join("map").join(name)
}

#[test]
fn cards_writes_an_index_and_one_card_per_file() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["cards", &args[0], &args[1]]);

    assert!(out.contains("written"), "output:\n{out}");
    assert!(card_path(&fx, "INDEX.md").exists());
    assert!(card_path(&fx, "ts-cache-ts.md").exists());
    assert!(card_path(&fx, "manifest.json").exists());
}

/// Frontmatter only parses as frontmatter when it is the first thing in the
/// file, so this is a hard requirement rather than a formatting preference.
#[test]
fn a_card_opens_with_its_frontmatter() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    fx.stdout(&["cards", &args[0], &args[1]]);

    let card = std::fs::read_to_string(card_path(&fx, "ts-cache-ts.md")).expect("read card");

    assert!(card.starts_with("---\n"), "card:\n{card}");
    assert!(card.contains("path: ts/cache.ts"));
    // The generated block must come after the frontmatter, not wrap it.
    let fence_end = card.find("\n---\n").expect("closing fence");
    let marker = card.find("<!-- tok:generated:start -->").expect("marker");
    assert!(marker > fence_end);
}

/// The single most important property of the markdown layer.
#[test]
fn regenerating_cards_preserves_hand_written_notes() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    fx.stdout(&["cards", &args[0], &args[1]]);

    let path = card_path(&fx, "ts-cache-ts.md");
    let annotated = std::fs::read_to_string(&path).expect("read").replace(
        "## Notes",
        "## Notes\n\nCache eviction is deliberately naive.",
    );
    std::fs::write(&path, annotated).expect("annotate");

    fx.stdout(&["cards", &args[0], &args[1]]);

    let after = std::fs::read_to_string(&path).expect("read again");
    assert!(
        after.contains("Cache eviction is deliberately naive."),
        "card:\n{after}"
    );
}

#[test]
fn check_is_clean_immediately_after_generating() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    fx.stdout(&["cards", &args[0], &args[1]]);

    let (out, code) = fx.run(&["check", &args[0], &args[1]]);

    assert_eq!(code, 0);
    assert!(out.contains("up to date"), "output:\n{out}");
}

#[test]
fn check_reports_missing_markdown_with_the_command_that_fixes_it() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["check", &args[0], &args[1]]);

    assert!(out.contains("tok mem cards"), "output:\n{out}");
}

#[test]
fn check_detects_an_edited_source_file() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    fx.stdout(&["cards", &args[0], &args[1]]);

    std::fs::write(
        fx.root().join("ts/util.ts"),
        "export function normalize(x: string): string { return x.trim(); }\n// changed\n",
    )
    .expect("edit");

    let out = fx.stdout(&["check", &args[0], &args[1]]);

    assert!(out.contains("changed since generation"), "output:\n{out}");
    assert!(out.contains("ts/util.ts"), "output:\n{out}");
}

#[test]
fn check_detects_a_new_undocumented_file() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    fx.stdout(&["cards", &args[0], &args[1]]);

    std::fs::write(fx.root().join("ts/added.ts"), "export const added = 1;\n").expect("add");

    let out = fx.stdout(&["check", &args[0], &args[1]]);

    assert!(out.contains("not yet documented"), "output:\n{out}");
}

#[test]
fn strict_check_exits_nonzero_on_drift() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    fx.stdout(&["cards", &args[0], &args[1]]);

    std::fs::write(fx.root().join("ts/added.ts"), "export const added = 1;\n").expect("add");

    let (_, lenient) = fx.run(&["check", &args[0], &args[1]]);
    let (_, strict) = fx.run(&["check", "--strict", &args[0], &args[1]]);

    assert_eq!(lenient, 0, "the default must not fail a developer's shell");
    assert_eq!(strict, 1, "--strict is what CI runs");
}

#[test]
fn regenerating_clears_reported_drift() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    fx.stdout(&["cards", &args[0], &args[1]]);

    std::fs::write(fx.root().join("ts/added.ts"), "export const added = 1;\n").expect("add");
    assert_eq!(fx.run(&["check", "--strict", &args[0], &args[1]]).1, 1);

    fx.stdout(&["cards", &args[0], &args[1]]);

    assert_eq!(fx.run(&["check", "--strict", &args[0], &args[1]]).1, 0);
}

#[test]
fn a_second_generation_rewrites_nothing() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    fx.stdout(&["cards", &args[0], &args[1]]);

    let out = fx.stdout(&["cards", &args[0], &args[1]]);

    assert!(out.contains("0 written"), "output:\n{out}");
}

// ------------------------------------------------------ ranking pins

/// Pins the retrieval surface, including the order hits come back in.
///
/// The individual behaviour tests above each assert one property; ranking is
/// the emergent result of BM25, the personalized walk, and the fusion step, and
/// no single assertion describes it. A snapshot does, and it turns "the blend
/// weights moved" from something noticed in review into a failing test.
#[test]
fn retrieval_surface_snapshot() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    let root = fx.root().to_string_lossy().to_string();
    let canonical = std::fs::canonicalize(fx.root())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| root.clone());

    let mut report = String::new();
    let mut record = |label: &str, out: String| {
        let clean = out
            .replace(&canonical, "<FIXTURE>")
            .replace(&root, "<FIXTURE>");
        report.push_str(&format!("=== {label} ===\n{}\n\n", clean.trim_end()));
    };

    record(
        "ask-cache",
        fx.stdout(&["ask", "cache", &args[0], &args[1]]),
    );
    record(
        "ask-cache-lexical",
        fx.stdout(&["ask", "cache", "--lexical", &args[0], &args[1]]),
    );
    record(
        "ask-normalize",
        fx.stdout(&["ask", "normalize", &args[0], &args[1]]),
    );
    record(
        "ask-limit-2",
        fx.stdout(&["ask", "cache", "--limit", "2", &args[0], &args[1]]),
    );
    record(
        "skeleton",
        fx.stdout(&["skeleton", "ts/cache.ts", &args[0], &args[1]]),
    );
    record("grep", fx.stdout(&["grep", "cache", &args[0], &args[1]]));
    record("map", fx.stdout(&["map", &args[0], &args[1]]));

    insta::assert_snapshot!("retrieval_surface", report);
}

// --------------------------------------------------------- determinism

/// The property the whole cache layer rests on: reusing fingerprints and the
/// extract cache must produce the same graph as parsing everything again. If it
/// does not, incremental indexing silently serves a different answer from a
/// cold one, and no test of query behaviour would catch it.
#[test]
fn an_incremental_build_produces_the_same_graph_as_a_cold_one() {
    let fx = Fixture::new();
    let root = fx.root().to_string_lossy().to_string();
    let graph = fx.root().join(".tok/graph/graph.json");

    fx.stdout(&["index", &root]);
    let cold = std::fs::read_to_string(&graph).expect("cold graph");

    fx.stdout(&["index", "--incremental", &root]);
    let incremental = std::fs::read_to_string(&graph).expect("incremental graph");

    assert_eq!(cold, incremental, "incremental build diverged from cold");
}

/// Extraction order follows the filesystem walk, so an unstable sort would
/// show up as a graph that differs between machines rather than between runs.
#[test]
fn rebuilding_from_scratch_is_byte_identical() {
    let fx = Fixture::new();
    let root = fx.root().to_string_lossy().to_string();
    let graph = fx.root().join(".tok/graph/graph.json");

    fx.stdout(&["index", &root]);
    let first = std::fs::read_to_string(&graph).expect("first graph");

    std::fs::remove_dir_all(fx.root().join(".tok/graph")).expect("clear graph");
    fx.stdout(&["index", &root]);
    let second = std::fs::read_to_string(&graph).expect("second graph");

    assert_eq!(first, second);
}

/// An edit must reach the graph through the incremental path, or the cache is
/// serving stale answers rather than fast ones.
#[test]
fn an_incremental_build_picks_up_an_edited_file() {
    let fx = Fixture::new();
    let args = path_args(&fx);
    let root = fx.root().to_string_lossy().to_string();
    fx.stdout(&["index", &root]);

    std::fs::write(
        fx.root().join("ts/util.ts"),
        "export function freshlyAddedSymbol(): void {}\n",
    )
    .expect("edit");
    fx.stdout(&["index", "--incremental", &root]);

    let out = fx.stdout(&["ask", "freshlyAddedSymbol", &args[0], &args[1]]);

    assert!(out.contains("freshlyAddedSymbol"), "output:\n{out}");
}

// ------------------------------------------------------------- savings

/// Retrieval exists to cost fewer tokens than reading the files, so it should
/// say by how much.
#[test]
fn ask_reports_what_it_saved_against_reading_the_files() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["ask", "cache", &args[0], &args[1]]);

    assert!(out.contains("% saved"), "output:\n{out}");
    assert!(out.contains("tokens vs"), "output:\n{out}");
}

#[test]
fn skeleton_reports_what_it_saved_against_reading_the_file() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["skeleton", "ts/cache.ts", &args[0], &args[1]]);

    assert!(out.contains("% saved"), "output:\n{out}");
}

/// A query that returns nothing has saved nothing, and claiming otherwise
/// would make every other savings number less believable.
#[test]
fn a_query_with_no_hits_claims_no_saving() {
    let fx = Fixture::new();
    let args = path_args(&fx);

    let out = fx.stdout(&["ask", "zzzznotarealsymbol", &args[0], &args[1]]);

    assert!(!out.contains("% saved"), "output:\n{out}");
}

// -------------------------------------------------------- shared paths

#[test]
fn a_missing_repository_path_is_reported_not_panicked() {
    let fx = Fixture::new();

    let (_, code) = fx.run(&["map", "--path", "/definitely/not/a/repo"]);

    assert_ne!(code, 0);
    assert_ne!(code, 101, "must not be a panic");
}
