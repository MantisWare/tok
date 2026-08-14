//! Language detection and tree-sitter grammar handles.
//!
//! Grammars are resolved per call rather than cached in a global. Constructing a
//! `Language` from a `LanguageFn` is a pointer copy, so there is nothing to
//! amortize, and keeping no global state means the `<10ms` startup budget is
//! unaffected by how many grammars are compiled in.
//!
//! Each grammar sits behind its own `lang-*` feature. When a feature is off the
//! language is still *indexed* — [`Language::detect`] still recognizes it — but
//! [`Language::grammar`] returns `None` and the caller falls back to the regex
//! extractor, which finds symbols but no call edges.

/// A source language TOK can extract from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Language {
    TypeScript,
    /// TSX needs a separate grammar from TypeScript; the two are not interchangeable.
    Tsx,
    JavaScript,
    Python,
    Go,
    Rust,
}

impl Language {
    /// Stable identifier stored in the graph and used in `--in` filters.
    pub fn as_str(self) -> &'static str {
        match self {
            Language::TypeScript => "typescript",
            Language::Tsx => "tsx",
            Language::JavaScript => "javascript",
            Language::Python => "python",
            Language::Go => "go",
            Language::Rust => "rust",
        }
    }

    /// Detect from a file extension, without the leading dot.
    pub fn detect(extension: &str) -> Option<Self> {
        Some(match extension {
            "ts" | "mts" | "cts" => Language::TypeScript,
            "tsx" => Language::Tsx,
            // JSX is parsed by the TSX grammar, which is a superset of JS+JSX.
            "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
            "py" | "pyi" => Language::Python,
            "go" => Language::Go,
            "rs" => Language::Rust,
            _ => return None,
        })
    }

    /// Detect from a path, honouring the extension only.
    pub fn from_path(path: &str) -> Option<Self> {
        let ext = path.rsplit('.').next()?;
        // A path with no dot yields the whole path from rsplit; reject that.
        if ext == path {
            return None;
        }
        Language::detect(ext)
    }

    /// Every language the extractor knows about, feature-gated or not.
    pub fn all() -> &'static [Language] {
        &[
            Language::TypeScript,
            Language::Tsx,
            Language::JavaScript,
            Language::Python,
            Language::Go,
            Language::Rust,
        ]
    }

    /// The tree-sitter grammar, or `None` when its feature is disabled.
    #[cfg(feature = "graph")]
    pub fn grammar(self) -> Option<tree_sitter::Language> {
        match self {
            #[cfg(feature = "lang-typescript")]
            Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            #[cfg(feature = "lang-typescript")]
            Language::Tsx | Language::JavaScript => {
                Some(tree_sitter_typescript::LANGUAGE_TSX.into())
            }
            #[cfg(feature = "lang-python")]
            Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
            #[cfg(feature = "lang-go")]
            Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
            #[cfg(feature = "lang-rust")]
            Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    /// Whether a tree-sitter grammar is compiled in for this language.
    pub fn has_grammar(self) -> bool {
        #[cfg(feature = "graph")]
        {
            self.grammar().is_some()
        }
        #[cfg(not(feature = "graph"))]
        {
            false
        }
    }

    /// Heuristic for "this file is a test", used to down-rank results.
    ///
    /// Path-based rather than content-based so it stays cheap and works
    /// identically across languages.
    pub fn is_test_path(path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        let name = lower.rsplit('/').next().unwrap_or(&lower);

        name.ends_with("_test.go")
            || name.ends_with("_test.py")
            || name.starts_with("test_")
            || name.contains(".test.")
            || name.contains(".spec.")
            || lower.split('/').any(|seg| {
                matches!(
                    seg,
                    "test" | "tests" | "__tests__" | "spec" | "specs" | "testdata" | "fixtures"
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension() {
        assert_eq!(Language::detect("ts"), Some(Language::TypeScript));
        assert_eq!(Language::detect("tsx"), Some(Language::Tsx));
        assert_eq!(Language::detect("jsx"), Some(Language::JavaScript));
        assert_eq!(Language::detect("py"), Some(Language::Python));
        assert_eq!(Language::detect("go"), Some(Language::Go));
        assert_eq!(Language::detect("rs"), Some(Language::Rust));
        assert_eq!(Language::detect("md"), None);
    }

    #[test]
    fn from_path_requires_an_extension() {
        assert_eq!(Language::from_path("src/a.rs"), Some(Language::Rust));
        assert_eq!(Language::from_path("Makefile"), None);
        assert_eq!(Language::from_path(""), None);
    }

    #[test]
    fn test_paths_are_recognized() {
        assert!(Language::is_test_path("src/cache_test.go"));
        assert!(Language::is_test_path("src/cache.test.ts"));
        assert!(Language::is_test_path("src/cache.spec.ts"));
        assert!(Language::is_test_path("tests/cli/test_x.rs"));
        assert!(Language::is_test_path("src/__tests__/cache.ts"));
        assert!(Language::is_test_path("app/test_helpers.py"));

        assert!(!Language::is_test_path("src/cache.ts"));
        assert!(!Language::is_test_path("src/latest/cache.ts"));
        assert!(!Language::is_test_path("src/contest.py"));
    }

    /// The real ABI check. `tree-sitter` and the grammar crates version
    /// independently, and a mismatch shows up here as a panic or a parse
    /// failure rather than at compile time.
    #[cfg(feature = "graph")]
    #[test]
    fn every_enabled_grammar_parses_a_sample() {
        let samples: &[(Language, &str)] = &[
            (
                Language::TypeScript,
                "export function f(a: number): void {}",
            ),
            (Language::Tsx, "export const C = () => <div>hi</div>;"),
            (Language::JavaScript, "export function f(a) { return a; }"),
            (Language::Python, "def f(a):\n    return a\n"),
            (Language::Go, "package m\nfunc F(a int) int { return a }\n"),
            (Language::Rust, "pub fn f(a: u32) -> u32 { a }"),
        ];

        for (lang, src) in samples {
            let Some(grammar) = lang.grammar() else {
                continue;
            };
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&grammar)
                .unwrap_or_else(|e| panic!("ABI mismatch for {}: {e}", lang.as_str()));

            let tree = parser
                .parse(src, None)
                .unwrap_or_else(|| panic!("{} produced no tree", lang.as_str()));

            assert!(
                !tree.root_node().has_error(),
                "{} failed to parse its own sample: {}",
                lang.as_str(),
                tree.root_node().to_sexp()
            );
        }
    }

    /// Guards the default build: shipping with a language silently degraded to
    /// the regex fallback would be an invisible quality regression.
    #[cfg(all(
        feature = "lang-typescript",
        feature = "lang-python",
        feature = "lang-go",
        feature = "lang-rust"
    ))]
    #[test]
    fn default_build_has_every_grammar() {
        for lang in Language::all() {
            assert!(lang.has_grammar(), "{} has no grammar", lang.as_str());
        }
    }
}
