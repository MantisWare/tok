//! End-to-end coverage for monorepo scopes and workspace federation.
//!
//! These build real repositories on disk and run the real binary, because the
//! behaviour under test is mostly about layout: which marker files exist, where
//! `.git` is, what the walker can see. A unit test with a hand-built graph
//! cannot catch a discovery pass that never runs.

use std::path::Path;

use assert_fs::TempDir;

use super::tok_cmd;

/// A repository laid out by the test, with `.git` present so workspace
/// detection sees a real checkout.
struct Repo {
    dir: TempDir,
}

impl Repo {
    fn new() -> Self {
        let dir = TempDir::new().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join(".git")).expect("mkdir .git");
        Self { dir }
    }

    /// A parent directory of repositories, with no checkout of its own.
    fn workspace() -> Self {
        Self {
            dir: TempDir::new().expect("create temp dir"),
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, relative: &str, contents: &str) -> &Self {
        let path = self.root().join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, contents).expect("write");
        self
    }

    /// Create a child repository under this directory.
    fn child(&self, name: &str) -> &Self {
        std::fs::create_dir_all(self.root().join(name).join(".git")).expect("mkdir child");
        self
    }

    fn run(&self, args: &[&str]) -> (String, i32) {
        let mut cmd = tok_cmd();
        cmd.arg("mem")
            .args(args)
            .current_dir(self.root())
            .env("TOK_MEMORY_DB_PATH", self.root().join("memory.db"))
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

/// Two packages, each a project in its own right.
fn monorepo() -> Repo {
    let repo = Repo::new();
    repo.write("package.json", r#"{"workspaces":["packages/*"]}"#)
        .write("packages/api/package.json", r#"{"name":"api"}"#)
        .write(
            "packages/api/server.ts",
            "export function parseConfig(raw: string) { return JSON.parse(raw); }\n\
             export function startServer() { return parseConfig('{}'); }\n\
             export function handleRequest() { return startServer(); }\n\
             export function authorize() { return handleRequest(); }\n\
             export function shutdown() { return authorize(); }\n\
             export function reload() { return shutdown(); }\n",
        )
        .write("packages/web/package.json", r#"{"name":"web"}"#)
        .write(
            "packages/web/render.ts",
            "export function renderPage() { return 1; }\n\
             export function renderHeader() { return renderPage(); }\n\
             export function renderFooter() { return renderHeader(); }\n\
             export function mount() { return renderFooter(); }\n\
             export function hydrate() { return mount(); }\n\
             export function paint() { return hydrate(); }\n",
        );
    repo
}

#[test]
fn a_monorepo_labels_results_with_the_package_they_came_from() {
    let repo = monorepo();
    repo.run(&["index"]);

    let out = repo.stdout(&["ask", "render page"]);

    assert!(out.contains("renderPage"), "{out}");
    assert!(out.contains("packages/web"), "{out}");
}

/// The whole point of scoping: a question about one package should not fill
/// half the answer with the other one.
#[test]
fn a_package_with_nothing_to_say_stays_out_of_the_answer() {
    let repo = monorepo();
    repo.run(&["index"]);

    let out = repo.stdout(&["ask", "parseConfig"]);

    assert!(out.contains("parseConfig"), "{out}");
    assert!(!out.contains("renderPage"), "{out}");
}

#[test]
fn narrowing_confines_the_answer_to_one_package() {
    let repo = monorepo();
    repo.run(&["index"]);

    let out = repo.stdout(&["ask", "render", "--in", "packages/web"]);

    assert!(out.contains("renderPage"), "{out}");
    assert!(!out.contains("packages/api"), "{out}");
}

/// A single-project repository must look exactly as it did before scopes
/// existed — no labels, no footer, no extra column.
#[test]
fn a_plain_repository_gains_no_scope_labels() {
    let repo = Repo::new();
    repo.write(
        "src/config.ts",
        "export function parseConfig(raw: string) { return JSON.parse(raw); }\n",
    );
    repo.run(&["index"]);

    let out = repo.stdout(&["ask", "parseConfig"]);

    assert!(out.contains("parseConfig"), "{out}");
    // The only bracketed column should be the symbol kind.
    assert!(out.contains("[function]"), "{out}");
    assert!(!out.contains("[src"), "no scope label expected: {out}");
    assert!(!out.contains("also matched"), "{out}");
}

#[test]
fn indexing_a_workspace_indexes_each_repository_in_it() {
    let workspace = Repo::workspace();
    workspace.child("api").child("web");
    workspace
        .write(
            "api/server.ts",
            "export function parseConfig(raw: string) { return JSON.parse(raw); }\n",
        )
        .write(
            "web/render.ts",
            "export function renderPage() { return 1; }\n",
        );

    workspace.run(&["index"]);

    assert!(workspace.root().join("api/.tok/graph/graph.json").exists());
    assert!(workspace.root().join("web/.tok/graph/graph.json").exists());
}

/// The parent has no source of its own, so it must store the child list and no
/// graph at all.
#[test]
fn a_workspace_parent_stores_only_its_member_list() {
    let workspace = Repo::workspace();
    workspace.child("api").child("web");
    workspace.write("api/server.ts", "export function parseConfig() {}\n");

    workspace.run(&["index"]);

    assert!(workspace.root().join(".tok/graph/workspace.json").exists());
    assert!(!workspace.root().join(".tok/graph/graph.json").exists());
}

#[test]
fn a_query_at_the_parent_searches_every_child() {
    let workspace = Repo::workspace();
    workspace.child("api").child("web");
    workspace
        .write(
            "api/server.ts",
            "export function parseConfig(raw: string) { return JSON.parse(raw); }\n",
        )
        .write(
            "web/config.ts",
            "export function parseConfig(raw: string) { return raw; }\n",
        );

    workspace.run(&["index"]);
    let out = workspace.stdout(&["ask", "parseConfig"]);

    assert!(out.contains("api/"), "{out}");
    assert!(out.contains("web/"), "{out}");
}

/// A pointer copied out of a federated answer has to resolve from where the
/// query was run, not from inside the child.
#[test]
fn federated_pointers_are_relative_to_the_parent() {
    let workspace = Repo::workspace();
    workspace.child("api").child("web");
    workspace.write(
        "api/server.ts",
        "export function parseConfig(raw: string) { return JSON.parse(raw); }\n",
    );
    workspace.write("web/empty.ts", "export const x = 1;\n");

    workspace.run(&["index"]);
    let out = workspace.stdout(&["ask", "parseConfig"]);

    assert!(out.contains("api/server.ts:"), "{out}");
}

#[test]
fn narrowing_at_a_workspace_selects_one_repository() {
    let workspace = Repo::workspace();
    workspace.child("api").child("web");
    workspace
        .write(
            "api/server.ts",
            "export function parseConfig(raw: string) { return JSON.parse(raw); }\n",
        )
        .write(
            "web/config.ts",
            "export function parseConfig(raw: string) { return raw; }\n",
        );

    workspace.run(&["index"]);
    let out = workspace.stdout(&["ask", "parseConfig", "--in", "web"]);

    assert!(out.contains("web/config.ts"), "{out}");
    assert!(!out.contains("api/server.ts"), "{out}");
}

/// Indexing a repository that uses submodules must keep indexing its own
/// source rather than treating the submodules as a workspace.
#[test]
fn a_repository_with_submodules_still_indexes_its_own_source() {
    let repo = Repo::new();
    repo.child("vendor-a").child("vendor-b");
    repo.write(
        "src/config.ts",
        "export function parseConfig(raw: string) { return JSON.parse(raw); }\n",
    );

    repo.run(&["index"]);

    assert!(repo.root().join(".tok/graph/graph.json").exists());
    assert!(!repo.root().join(".tok/graph/workspace.json").exists());
    assert!(repo.stdout(&["ask", "parseConfig"]).contains("parseConfig"));
}

#[test]
fn a_workspace_query_reports_repositories_it_could_not_search() {
    let workspace = Repo::workspace();
    workspace.child("api").child("empty");
    workspace.write(
        "api/server.ts",
        "export function parseConfig(raw: string) { return JSON.parse(raw); }\n",
    );

    workspace.run(&["index"]);
    let out = workspace.stdout(&["ask", "parseConfig"]);

    assert!(out.contains("not indexed"), "{out}");
    assert!(out.contains("empty"), "{out}");
}
