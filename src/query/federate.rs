//! Querying a workspace: one question, every child repository.
//!
//! Each child is ranked independently against its own graph and its own corpus
//! statistics, exactly as if it had been queried alone, and the per-child
//! results are then fused by the same reciprocal rank fusion that merges scopes
//! within a repository. A child repo is a scope that happens to live in its own
//! git checkout, so it earns no special ranking rules.
//!
//! Loading is eager and sequential. A workspace holds a handful of checkouts,
//! not hundreds, and reading their graphs is a JSON parse rather than a build.
//! A child whose graph is missing or unreadable is skipped with a note rather
//! than failing the query, since one un-indexed repository should not make the
//! other four unreachable.

use std::path::Path;

use crate::graph::session;
use crate::graph::types::GraphV1;
use crate::graph::workspace;
use crate::query::ask::AskOptions;
use crate::query::index_file::{self, AskIndex};
use crate::query::scoped::{self, ScopedResults};

/// One child repository, loaded and ready to query.
pub struct Member {
    pub name: String,
    pub graph: GraphV1,
    pub index: AskIndex,
}

/// What loading a workspace produced, including the children it could not read.
pub struct Loaded {
    pub members: Vec<Member>,
    /// Children on disk that contribute nothing — unreadable, or holding no
    /// indexed file — so the caller can say which repositories an answer does
    /// not cover rather than implying it searched them.
    pub unindexed: Vec<String>,
}

impl Loaded {
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

/// Whether a query at this directory should federate.
pub fn applies(root: &Path) -> bool {
    workspace::is_workspace_root(root)
}

/// Load every child repository's graph.
///
/// `only` restricts to a single child, which is how `--in <child>` avoids
/// paying to read graphs it will not query.
pub fn load(root: &Path, only: Option<&str>) -> Loaded {
    let mut members = Vec::new();
    let mut unindexed = Vec::new();

    for name in workspace::members(root) {
        if only.is_some_and(|wanted| wanted != name) {
            continue;
        }

        let child = root.join(&name);
        // Each child refreshes on its own terms, so a stale sibling cannot
        // block the query.
        let Ok(opened) = session::open(&child) else {
            unindexed.push(name);
            continue;
        };

        // An empty graph is indistinguishable from a missing one at query
        // time, and saying so is more useful than ranking it.
        if opened.graph.files.is_empty() {
            unindexed.push(name);
            continue;
        }

        let index =
            index_file::load_or_build(&crate::graph::store::GraphPaths::new(&child), &opened.graph);

        members.push(Member {
            name,
            graph: opened.graph,
            index,
        });
    }

    Loaded { members, unindexed }
}

/// Run a query across loaded children and fuse the results.
///
/// `options.scope_prefix` should already be the *inner* prefix, with the child
/// name stripped by [`crate::graph::workspace::split_in`]; the child itself is
/// selected at load time.
pub fn ask<'a>(loaded: &'a Loaded, query: &str, options: &AskOptions) -> ScopedResults<'a> {
    let mut rankings = Vec::new();
    let mut weak = Vec::new();

    for member in &loaded.members {
        let scopes = eligible(&member.graph, options);
        let (ranked, weak_scopes) = scoped::rank(
            &member.graph,
            &member.index,
            query,
            options,
            &scopes,
            Some(&member.name),
        );

        rankings.extend(ranked);
        weak.extend(weak_scopes);
    }

    scoped::finish(rankings, weak, options.limit)
}

/// The scopes within one child that a query may draw on.
///
/// A child with no sub-projects contributes its single root scope, which is the
/// common case and keeps federation cheap for ordinary repositories.
fn eligible(graph: &GraphV1, options: &AskOptions) -> Vec<crate::graph::scopes::ScopeV1> {
    let scopes = graph.scopes();

    let Some(prefix) = &options.scope_prefix else {
        return scopes;
    };

    let narrowed: Vec<crate::graph::scopes::ScopeV1> = scopes
        .iter()
        .filter(|scope| crate::graph::scopes::path_under_prefix(&scope.prefix, prefix))
        .cloned()
        .collect();

    if narrowed.is_empty() {
        return vec![crate::graph::scopes::ScopeV1 {
            prefix: prefix.clone(),
            label: prefix.clone(),
            markers: Vec::new(),
        }];
    }

    narrowed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{NodeKind, NodeV1, Span};

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn member(name: &str, symbols: &[(&str, &str)]) -> Member {
        let mut graph = GraphV1::new(name, "test");
        graph.nodes = symbols
            .iter()
            .map(|(id, file)| {
                NodeV1::new(
                    (*id).to_string(),
                    NodeKind::Function,
                    (*id).to_string(),
                    (*file).to_string(),
                    Span::new(1, 5),
                )
            })
            .collect();
        graph.normalize();
        let index = index_file::build(&graph);

        Member {
            name: name.to_string(),
            graph,
            index,
        }
    }

    fn loaded(members: Vec<Member>) -> Loaded {
        Loaded {
            members,
            unindexed: Vec::new(),
        }
    }

    fn labels(results: &ScopedResults<'_>) -> Vec<String> {
        results
            .hits
            .iter()
            .map(|hit| hit.label().unwrap_or_default())
            .collect()
    }

    #[test]
    fn a_parent_of_several_repositories_federates() {
        let dir = temp();
        for name in ["api", "web"] {
            std::fs::create_dir_all(dir.path().join(name).join(".git")).expect("mkdir");
        }

        assert!(applies(dir.path()));
    }

    #[test]
    fn an_ordinary_repository_does_not_federate() {
        let dir = temp();
        std::fs::create_dir_all(dir.path().join(".git")).expect("mkdir");

        assert!(!applies(dir.path()));
    }

    #[test]
    fn a_match_in_each_child_comes_back_from_both() {
        let workspace = loaded(vec![
            member("api", &[("parseConfig", "src/config.ts")]),
            member("web", &[("parseConfig", "src/config.ts")]),
        ]);

        let results = ask(&workspace, "parseConfig", &AskOptions::default());

        assert_eq!(results.hits.len(), 2);
        assert_eq!(labels(&results), vec!["api", "web"]);
    }

    /// Two children can mint the same symbol id, and fusing them into one
    /// result would silently drop a real answer.
    #[test]
    fn identical_ids_in_different_children_stay_distinct() {
        let workspace = loaded(vec![
            member("api", &[("handler", "src/a.ts")]),
            member("web", &[("handler", "src/a.ts")]),
        ]);

        let results = ask(&workspace, "handler", &AskOptions::default());

        assert_eq!(results.hits.len(), 2);
    }

    #[test]
    fn a_child_pointer_resolves_from_the_parent() {
        let workspace = loaded(vec![member("api", &[("parseConfig", "src/config.ts")])]);

        let results = ask(&workspace, "parseConfig", &AskOptions::default());

        assert_eq!(results.hits[0].path(), "api/src/config.ts");
        assert_eq!(results.hits[0].location(), "api/src/config.ts:1");
    }

    #[test]
    fn a_child_that_matches_only_weakly_is_reported_not_included() {
        let workspace = loaded(vec![
            member("api", &[("parseConfig", "src/config.ts")]),
            member("docs", &[("config", "src/other.ts")]),
        ]);

        let results = ask(&workspace, "parseConfig zzz", &AskOptions::default());

        assert_eq!(labels(&results), vec!["api"]);
        assert!(results.also_matched.contains(&"docs".to_string()));
    }

    /// A child sharing no term with the question is simply not an answer, and
    /// listing it as "also matched" would be untrue.
    #[test]
    fn a_child_matching_nothing_is_not_reported_at_all() {
        let workspace = loaded(vec![
            member("api", &[("parseConfig", "src/config.ts")]),
            member("docs", &[("unrelatedThing", "src/other.ts")]),
        ]);

        let results = ask(&workspace, "parseConfig", &AskOptions::default());

        assert_eq!(labels(&results), vec!["api"]);
        assert!(results.also_matched.is_empty());
    }

    #[test]
    fn narrowing_at_load_time_queries_only_one_child() {
        let dir = temp();
        for name in ["api", "web"] {
            std::fs::create_dir_all(dir.path().join(name).join(".git")).expect("mkdir");
        }

        let only = load(dir.path(), Some("api"));

        // Neither child holds source, so both would be reported — but only the
        // one that was asked for was even looked at.
        assert_eq!(only.unindexed, vec!["api"]);
    }

    /// One un-indexed repository must not make the other four unreachable.
    #[test]
    fn a_child_with_no_indexed_source_is_recorded_rather_than_ranked() {
        let dir = temp();
        for name in ["api", "web"] {
            std::fs::create_dir_all(dir.path().join(name).join(".git")).expect("mkdir");
        }

        let all = load(dir.path(), None);

        assert!(all.is_empty());
        assert_eq!(all.unindexed, vec!["api", "web"]);
    }

    #[test]
    fn an_empty_workspace_answers_nothing() {
        let workspace = loaded(Vec::new());

        assert!(ask(&workspace, "anything", &AskOptions::default()).is_empty());
    }

    #[test]
    fn the_limit_applies_across_all_children() {
        let workspace = loaded(vec![
            member(
                "api",
                &[("parseConfig", "a.ts"), ("parseConfigFile", "b.ts")],
            ),
            member(
                "web",
                &[("parseConfig", "a.ts"), ("parseConfigFile", "b.ts")],
            ),
        ]);

        let options = AskOptions {
            limit: 3,
            ..AskOptions::default()
        };

        assert_eq!(ask(&workspace, "parseConfig", &options).hits.len(), 3);
    }

    #[test]
    fn federated_results_are_stable_across_runs() {
        let workspace = loaded(vec![
            member("api", &[("cacheGet", "src/cache.ts")]),
            member("web", &[("cacheGet", "src/cache.ts")]),
        ]);

        let first = labels(&ask(&workspace, "cache", &AskOptions::default()));
        for _ in 0..5 {
            assert_eq!(
                labels(&ask(&workspace, "cache", &AskOptions::default())),
                first
            );
        }
    }
}
