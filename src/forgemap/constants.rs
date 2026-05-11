//! ForgeMap configuration constants (FORGEMAP.md §5).

/// File extensions supported by ForgeMap scanning.
///
/// Extends the original TS-only spec to cover all languages supported
/// by `mem::parser_regex`.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    // TypeScript / JavaScript
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", // Rust
    "rs",  // Python
    "py", "pyi", // Go
    "go",  // Ruby
    "rb",  // C# / Java
    "cs", "java",
];

/// Directories never to descend into.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    "build",
    ".next",
    ".turbo",
    ".cache",
    "coverage",
    ".nyc_output",
    ".vscode",
    ".idea",
    "out",
    "tmp",
    ".tmp",
    ".vite",
    ".parcel-cache",
    "target",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    "vendor",
    ".bundle",
    "bin",
];

/// Test file suffixes excluded from scanning unless the directory is
/// the explicit target.
pub const TEST_SUFFIXES: &[&str] = &[
    ".test.ts",
    ".spec.ts",
    ".test.tsx",
    ".spec.tsx",
    ".test.js",
    ".spec.js",
    ".test.jsx",
    ".spec.jsx",
    ".test.mts",
    ".spec.mts",
    ".test.cts",
    ".spec.cts",
    ".test.mjs",
    ".spec.mjs",
    ".test.cjs",
    ".spec.cjs",
    "_test.go",
    "_test.rs",
    "_spec.rb",
    "_test.py",
    "test_",
];

/// Cap on number of `exports:` entries before truncation.
pub const EXPORTS_CAP: usize = 20;

/// Rolling window for `agent:` lines inside a single header.
pub const AGENT_WINDOW: usize = 5;

/// Rolling window for `agent_sessions:` inside `.forgemap`.
pub const SESSIONS_WINDOW: usize = 3;

/// Cap on package key depth — a "package" is identified up to this many path segments.
pub const PACKAGE_DEPTH: usize = 3;

/// Header detection: scan the first N lines for field markers.
pub const HEADER_SCAN_LINES: usize = 30;

/// Default model ID when no LLM is involved.
pub const DEFAULT_MODEL_ID: &str = "forgemap-cli (no-llm)";

/// Manifest filename at repo root.
pub const MANIFEST_FILENAME: &str = ".forgemap";

/// Fields that signal a ForgeMap/CodeDNA header in a comment block.
pub const HEADER_FIELDS: &[&str] = &[
    "exports:", "used_by:", "related:", "wiki:", "rules:", "agent:", "message:",
];

/// Return the comment prefix for a given file extension.
pub fn comment_prefix_for_ext(ext: &str) -> &'static str {
    match ext {
        "py" | "pyi" | "rb" => "#",
        _ => "//",
    }
}

/// Return the comment prefix for a given file path (by extension).
#[allow(dead_code)]
pub fn comment_prefix_for_path(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    comment_prefix_for_ext(ext)
}
