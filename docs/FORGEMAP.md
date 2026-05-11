# ForgeMap — Code-Indexing and Annotation Engine

**Purpose:** This document is a complete, self-contained specification for the ForgeMap system — a TypeScript-native code-indexing and annotation engine (CodeDNA-compatible). It covers every module, data structure, algorithm, CLI command, IPC integration, and UI surface. Give this document to another application instance to replicate ForgeMap functionality in full.

**Origin:** ForgeMap is a TypeScript port of the [CodeDNA](https://github.com/Larens94/codedna) Python protocol. The implementation name is "ForgeMap"; the protocol format is CodeDNA-compatible.

---

## Table of Contents

1. [Overview](#1-overview)
2. [The ForgeMap Protocol](#2-the-forgemap-protocol)
3. [Core Data Model (TypeScript)](#3-core-data-model-typescript)
4. [Module Layout](#4-module-layout)
5. [Constants](#5-constants)
6. [File Collection (`collect.ts`)](#6-file-collection-collectts)
7. [Per-File Extraction (`scan.ts`)](#7-per-file-extraction-scants)
8. [Graph Construction (`graph.ts`)](#8-graph-construction-graphts)
9. [Header Parsing and Formatting (`header.ts`)](#9-header-parsing-and-formatting-headerts)
10. [Idempotent Injection (`inject.ts`)](#10-idempotent-injection-injectts)
11. [Utility Modules](#11-utility-modules)
12. [Commands](#12-commands)
13. [Manifest System (`.forgemap`)](#13-manifest-system-forgemap)
14. [Obsidian Wiki Generation](#14-obsidian-wiki-generation)
15. [Pre-Commit Hook & Tool Prompts](#15-pre-commit-hook--tool-prompts)
16. [CLI Surface](#16-cli-surface)
17. [Electron IPC Integration](#17-electron-ipc-integration)
18. [UI Integration (StatusBar)](#18-ui-integration-statusbar)
19. [Critical Algorithms & Invariants](#19-critical-algorithms--invariants)
20. [Acceptance Tests](#20-acceptance-tests)
21. [Implementation Order](#21-implementation-order)

---

## 1. Overview

ForgeMap is a **code-indexing and in-file annotation engine**. It:

- **Scans** TypeScript/JavaScript files to extract exports and internal import dependencies.
- **Builds** a reverse dependency graph (`used_by`) — answering "who imports this file?"
- **Injects** machine-readable comment headers (Level 1) at the top of source files, idempotently.
- **Refreshes** only structural fields (`exports:`, `used_by:`) without touching human/agent-authored fields.
- **Generates** a `.forgemap` project manifest (Level 0) with package detection and session logging.
- **Emits** an Obsidian-style markdown wiki vault with `[[wikilinks]]` derived from the dependency graph.
- **Provides** a CLI (`forge forgemap <command>`) and Electron IPC for integration with desktop apps.
- **Installs** a pre-commit hook and tool prompt files (`CLAUDE.md`, `AGENTS.md`).

**What ForgeMap is NOT:** It is not a visual graph (no D3/SVG/canvas). The "map" refers to dependency/manifest mapping + documentation, not an on-screen node diagram.

**Constraints:**
- No LLM calls in v1 — all analysis is AST-based.
- No network calls during normal operation.
- TypeScript/JavaScript files only (`.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs`, `.mts`, `.cts`).
- Uses TypeScript's own compiler API (`ts.createSourceFile`) — not tree-sitter.
- No YAML library — the `.forgemap` manifest uses line-based regex parsing.

---

## 2. The ForgeMap Protocol

ForgeMap implements an **inter-agent communication protocol** encoded as comment blocks at the top of source files. The annotations carry information that **cannot be inferred from reading a single file alone** — most importantly the reverse dependency graph (`used_by:`) and domain rules (`rules:`).

### Four Levels (only Levels 0 and 1 are generated)

| Level | Where | Purpose | Generated? |
|-------|-------|---------|------------|
| **0** Project manifest | `.forgemap` at repo root | Project structure, packages, `depends_on`, session log | **Yes** |
| **1** Module header | Comment block at top of every source file | `exports:`, `used_by:`, `rules:`, `agent:`, `message:` | **Yes** (structural fields only) |
| **2** Function `Rules:` | JSDoc comment on individual functions | Per-function constraints | **No** (organic growth by agents) |
| **3** Semantic naming | Variable identifiers | Type/origin in the name | **No** (convention only) |

### Level 1 Module Header — The Exact Format

The header is a `//` comment block. It is the **first content in the file** (except for an optional shebang on line 1). A blank line follows the closing line of the header before the first import or code statement.

```ts
// services/revenue.ts — Monthly/annual revenue aggregation from paid invoices.
//
// exports: monthlyRevenue(year, month, users): Promise<object> | annualSummary(year, users): Promise<object[]>
// used_by: src/routes.ts → revenueRoute
//          src/api/serializers.ts → RevenueSchema [cascade]
// related: src/billing/invoices.ts — shares the suspended-tenant filter pattern
// wiki:    docs/wiki/services/revenue.md
// rules:   getInvoicesForPeriod returns ALL invoices including suspended users —
//          callers MUST filter via activeIds set before aggregating.
// agent:   claude-opus-4-7 | anthropic | 2026-04-29 | s_20260429_001 | initial ForgeMap annotation pass
//          message: "rounding edge case in multi-currency — investigate before next release"
```

### Field Reference

| Field | Required | Multi-line? | Content |
|-------|----------|-------------|---------|
| First line | Yes | No | `<rel-path> — <purpose ≤15 words>` |
| `exports:` | Yes | No (pipe-separated, capped at 20) | Public API of the file |
| `used_by:` | Yes | Yes (one importer per line, indented) | Inverse import graph |
| `related:` | No | Yes | Files sharing logic/pattern without importing each other |
| `wiki:` | No | No (single path) | Pointer to curated `docs/wiki/<path>.md` |
| `rules:` | Yes | Yes | Hard constraints; literal `none` if no constraints |
| `agent:` | Yes | Yes (rolling window of last 5 entries) | `model-id \| provider \| YYYY-MM-DD \| session_id \| narrative` |
| `message:` | No | No | Inter-agent observations, soft warnings (append-only via `agent:` sub-field) |

### Level 0 Manifest (`.forgemap`)

YAML-shaped but **written and read with simple line-based logic** — no YAML library.

```yaml
# .forgemap — ForgeMap project manifest (auto-generated by `forge forgemap manifest`)
project: myproject
description: "Project description"
mode: semi   # human | semi | agent

packages:
  src/services/:
    purpose: "Domain services — revenue aggregation, billing, user lifecycle"
    key_files: [revenue.ts, billing.ts, user.ts]
    depends_on: [src/models/, src/utils/]

  src/api/:
    purpose: "HTTP routing layer"
    key_files: [routes.ts, serializers.ts]
    depends_on: [src/services/]

cross_cutting_patterns: {}

agent_sessions:
  - agent: claude-opus-4-7
    provider: anthropic
    date: 2026-04-29
    session_id: s_20260429_001
    task: "implement monthly revenue aggregation"
    changed: [src/services/revenue.ts]
    visited: [src/services/revenue.ts, src/models/user.ts, src/api/routes.ts]
    message: >
      Implemented monthlyRevenue. Discovered getInvoicesForPeriod returns all
      invoices — added rule.
```

**Manifest Invariants:**
- `agent_sessions:` is **append-only** with a rolling window of the last 3 entries (older sessions pruned — full history in `git log`).
- `packages:` is **regenerated on every `manifest` run** — authoritative from code.
- `cross_cutting_patterns:` is **preserved verbatim** across runs.
- `project:` and `description:` are preserved unless upgrading from a directory-name fallback to `package.json#name`.

---

## 3. Core Data Model (TypeScript)

These are the canonical types. Use them across all modules.

```typescript
// types.ts

/** Public symbol exported from a file. Stored as the human-readable signature string. */
export type ExportSig = string;

/** Repo-relative POSIX path (forward slashes, even on Windows). */
export type RelPath = string;

/** Internal map: importer file -> [imported symbols]. Empty array means "imports the file but no specific named symbol". */
export type DepMap = Record<RelPath, string[]>;

/** Reverse map: imported file -> { importer file -> [symbols] }. */
export type UsedByMap = Record<RelPath, Record<RelPath, string[]>>;

export interface FileInfo {
  /** Absolute path on disk. */
  absPath: string;
  /** Repo-relative POSIX path (the canonical key everywhere). */
  rel: RelPath;
  /** Public exports as printable signatures. */
  exports: ExportSig[];
  /** Map of dep file (rel path) -> imported symbols. */
  deps: DepMap;
  /** Parsed ForgeMap fields (if a header was found), else null. */
  header: ParsedHeader | null;
  /** True iff a ForgeMap header was detected. */
  hasForgeMap: boolean;
  /** False if the file failed to parse. */
  parseable: boolean;
}

export interface ParsedHeader {
  /** First line minus the leading `// ` — the purpose blurb. */
  firstLine: string;
  /** Raw value of `exports:` (joined into a single string for round-tripping). */
  exports: string;
  /** Raw value of `used_by:` — may be multi-line, lines joined with `\n`. */
  usedBy: string;
  related?: string;
  wiki?: string;
  rules: string;
  agent: string;
  message?: string;
  /** 0-based line index in the file where the header block starts. */
  startLine: number;
  /** 0-based line index where it ends (the last comment line of the block). */
  endLine: number;
}

export interface PackageInfo {
  /** Path-like key, e.g. "src/services/" or "" for repo root. */
  key: string;
  files: RelPath[];
  purpose: string;
  keyFiles: string[];     // bare basenames, ranked by importance
  dependsOn: string[];    // other package keys, sorted
}

export interface AgentSession {
  agent: string;
  provider: string;
  date: string;            // YYYY-MM-DD
  sessionId: string;       // e.g. "s_20260429_001"
  task: string;            // ≤15 words
  changed: RelPath[];
  visited: RelPath[];
  message: string;         // free-form narrative
}

export interface Manifest {
  project: string;
  description: string;
  mode: 'human' | 'semi' | 'agent';
  packages: Record<string, Omit<PackageInfo, 'key' | 'files'>>;
  /** Preserved verbatim across runs. */
  crossCuttingPatternsBlock: string;
  /** Append-only, rolling window of last 3. */
  agentSessions: AgentSession[];
}

export interface ExistingManifest {
  project: string;
  description: string;
  mode: 'human' | 'semi' | 'agent';
  agentSessionsBlock: string;
  crossCuttingBlock: string;
}

export interface InitOptions {
  target: string;          // absolute path
  repoRoot: string;
  extensions?: readonly string[];
  exclude?: string[];
  dryRun?: boolean;
  force?: boolean;         // re-annotate already-annotated files (init only)
  verbose?: boolean;
  modelId?: string;        // default "forgemap-cli (no-llm)"
  sessionId?: string;      // default: auto-generated
}
```

The `header` object stores **raw parsed strings**, not pre-split arrays. Round-tripping `refresh` requires preserving every byte of `rules:`, `agent:`, `related:`, `wiki:`, `message:` exactly as written. Only `exports:` and `used_by:` are ever rewritten.

### IPC Types (for Electron integration)

```typescript
export interface ForgeMapStats {
  totalFiles: number;
  annotatedFiles: number;
  missingFiles: number;
  coveragePercent: number;
  packageCount: number;
  topPackages: Array<{
    name: string;
    fileCount: number;
    keyFiles: string[];
  }>;
  exportCount: number;
  depEdgeCount: number;
  hasManifest: boolean;
  lastScanMs: number;
}

export interface ForgeMapInitResult {
  success: boolean;
  totalFiles: number;
  annotated: number;
  skipped: number;
  errors: number;
  durationMs: number;
}
```

---

## 4. Module Layout

```
src/forgemap/
  index.ts              # Public API barrel — re-exports CLI registration, core pipeline, types
  cli.ts                # Commander subcommand registration (forge forgemap <cmd>)
  types.ts              # Shared types (§3)
  constants.ts          # Scan/manifest constants (§5)
  collect.ts            # File walking + filtering (§6)
  scan.ts               # Per-file extraction via ts.createSourceFile (§7)
  graph.ts              # buildUsedBy, detectPackages, dependsOn (§8)
  header.ts             # Parse + format ForgeMap comment headers (§9)
  inject.ts             # Idempotent header injection + refresh (§10)
  manifest-io.ts        # .forgemap read/write, line-based parser (§13)
  wiki-emitter.ts       # Obsidian markdown emitter (§14)
  hook.ts               # Pre-commit hook template + tool prompt content (§15)
  commands/
    init.ts             # Run pipeline, write headers
    update.ts           # Init but skip already-annotated
    check.ts            # Coverage report, exit code
    refresh.ts          # Structural-only update
    manifest.ts         # .forgemap writer
    wiki.ts             # Bootstrap + sync wrappers
    install.ts          # Pre-commit hook + tool prompt files
  util/
    fs.ts               # Safe read/write, POSIX paths, CRLF normalization
    fmt.ts              # Formatting helpers (fmtExports, fmtUsedBy, detectProvider, etc.)

# Tests
src/__tests__/forgemap/
  forgemap.test.ts      # Unit tests
  fixtures/             # Sample TS files with/without headers

# Electron integration
electron/handlers/forgemap-handlers.ts   # IPC handlers (GET_STATS, INIT)
electron/handlers/index.ts               # Calls registerForgeMapHandlers()
electron/preload.ts                      # Exposes getForgeMapStats, initForgeMap on window.vibeforge

# IPC channels
common/constants/ipc-channels.ts         # FORGEMAP.GET_STATS, FORGEMAP.INIT

# IPC types
common/types.ts                          # ForgeMapStats, ForgeMapInitResult, API typings

# CLI entry
bin/forge.js                             # Registers `forge forgemap` subcommand

# UI
src/components/StatusBar.tsx             # Footer chip with coverage %, popup with stats
```

### Barrel Export (`index.ts`)

```typescript
export { registerForgemapCommand } from './cli';
export { collectFiles } from './collect';
export { scanFile } from './scan';
export { buildHeader, parseHeader, rebuildHeader } from './header';
export { injectHeader, replaceHeader, refreshHeader } from './inject';
export { buildUsedBy, detectPackages } from './graph';
export { readExistingManifest, writeManifest, detectProjectMeta } from './manifest-io';
export { bootstrapWiki, syncWiki, renderFilePage } from './wiki-emitter';
export { runInit } from './commands/init';
export { runUpdate } from './commands/update';
export { runCheck, formatCheckResult } from './commands/check';
export { runRefresh } from './commands/refresh';
export { runManifest } from './commands/manifest';
export { runWikiBootstrap, runWikiSync } from './commands/wiki';
export { runInstall } from './commands/install';
export type {
  FileInfo, ParsedHeader, ExportSig, RelPath,
  DepMap, UsedByMap, PackageInfo, AgentSession,
  Manifest, ExistingManifest, InitOptions,
} from './types';
```

---

## 5. Constants

```typescript
// constants.ts

export const SUPPORTED_EXTENSIONS = [
  '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.mts', '.cts',
] as const;

/** Directories never to descend into. */
export const SKIP_DIRS = new Set([
  'node_modules', '.git', 'dist', 'build', '.next', '.turbo', '.cache',
  'coverage', '.nyc_output', '.vscode', '.idea',
  'out', 'tmp', '.tmp', '.vite', '.parcel-cache',
]);

/** Test file suffixes excluded from scanning unless their directory is the explicit target. */
export const TEST_SUFFIXES = [
  '.test.ts', '.spec.ts', '.test.tsx', '.spec.tsx',
  '.test.js', '.spec.js', '.test.jsx', '.spec.jsx',
  '.test.mts', '.spec.mts', '.test.cts', '.spec.cts',
  '.test.mjs', '.spec.mjs', '.test.cjs', '.spec.cjs',
] as const;

export const COMMENT_PREFIX = '//';

/** Cap on number of `exports:` entries before truncation. */
export const EXPORTS_CAP = 20;

/** Rolling window for `agent:` lines inside a single header. */
export const AGENT_WINDOW = 5;

/** Rolling window for `agent_sessions:` inside .forgemap. */
export const SESSIONS_WINDOW = 3;

/** Cap on package key depth — a "package" is identified up to this many path segments. */
export const PACKAGE_DEPTH = 3;

/** Header detection: scan the first N lines for field markers. */
export const HEADER_SCAN_LINES = 30;

/** Default model ID when no LLM is involved. */
export const DEFAULT_MODEL_ID = 'forgemap-cli (no-llm)';

/** Manifest filename at repo root. */
export const MANIFEST_FILENAME = '.forgemap';

/** Fields that signal a ForgeMap/CodeDNA header in a comment block. */
export const HEADER_FIELDS = [
  'exports:', 'used_by:', 'related:', 'wiki:', 'rules:', 'agent:', 'message:',
] as const;
```

---

## 6. File Collection (`collect.ts`)

Walks a project tree, filters by extension, skips vendor/build dirs.

### Interface

```typescript
export interface CollectOptions {
  /** Absolute path — file or directory. */
  target: string;
  /** Absolute path to the repo root. */
  repoRoot: string;
  /** File extensions to include (default: SUPPORTED_EXTENSIONS). */
  extensions?: readonly string[];
  /** Glob patterns to exclude (matched against basename and relative-to-target path). */
  exclude?: string[];
}

export function collectFiles(opts: CollectOptions): string[];
// Returns absolute paths, sorted alphabetically.
```

### Rules

1. If `target` is a file: return `[target]` iff its extension is supported and not excluded; else `[]`.
2. If `target` is a directory: recursively walk.
3. Skip any path whose components include a name in `SKIP_DIRS` (at any depth).
4. Skip files whose extension is not in the requested set.
5. Apply `exclude` patterns via `minimatch` (basename **or** relative-to-target). 
6. Sort output deterministically (alphabetical by absolute path).
7. **Test file exclusion:** files ending in `.test.ts` / `.spec.ts` / etc. are collected only when their directory is the explicit `target`. Otherwise they are skipped. (Test files distort the dependency graph.)

### Dependencies

- `minimatch` — for glob pattern matching on exclude patterns.
- `node:fs`, `node:path` — standard Node.js modules.

---

## 7. Per-File Extraction (`scan.ts`)

The heart of ForgeMap. Uses `ts.createSourceFile` from the `typescript` package.

### Interface

```typescript
export function scanFile(absPath: string, repoRoot: string): FileInfo;
export function isTypeScriptAvailable(): boolean;
```

### TypeScript Compiler Availability

The TypeScript compiler is loaded via dynamic `require('typescript')`. In packaged/bundled builds it may not be available. When unavailable:
- Header detection still works (regex-based).
- Exports and deps are empty; `parseable = false`.
- The `isTypeScriptAvailable()` function lets callers check before invoking.

### ScriptKind Mapping

| Extension | ScriptKind |
|-----------|-----------|
| `.ts`, `.cts`, `.mts` | `ts.ScriptKind.TS` |
| `.tsx` | `ts.ScriptKind.TSX` |
| `.js`, `.cjs`, `.mjs` | `ts.ScriptKind.JS` |
| `.jsx` | `ts.ScriptKind.JSX` |

### Export Extraction

Walk top-level statements only (do not recurse into function bodies).

| AST Node | Emit |
|----------|------|
| `ExportDeclaration` with named `exportClause` | One entry per `ExportSpecifier.name.text` |
| `ExportDeclaration` with `*` re-export | Skip (barrel re-exports) |
| `ExportAssignment` (`export = X` or `export default X`) | `default` |
| `FunctionDeclaration` with `export` modifier | Full signature: `name(p1, p2, ...): ReturnType` |
| `ClassDeclaration` with `export` modifier | Class name + public method signatures as `ClassName::methodName(...)` |
| `InterfaceDeclaration` with `export` modifier | Interface name |
| `TypeAliasDeclaration` with `export` modifier | Type name |
| `EnumDeclaration` with `export` modifier | Enum name |
| `VariableStatement` with `export` modifier | One entry per variable name |

**Signature formatting:** Uses `printer.printNode()` then strips the function body via regex (`{[\s\S]*$`), removes `export`/`default`/`declare` keywords, collapses whitespace. If signature exceeds 120 chars, truncate to `name(...)`.

**Export cap:** At `EXPORTS_CAP` (20). If more, appends `(+N more)`.

**Class methods:** Only public methods emitted (no `private`, `protected`, constructor, or `#`-prefixed). Getters: `ClassName::get propName()`. Setters: `ClassName::set propName(value)`.

### Dependency Extraction

Walk top-level `ImportDeclaration` and dynamic `import()` expressions.

**Rules:**
1. **Skip bare specifiers** — anything not starting with `.` or `/`. So `import x from "react"` is ignored.
2. **Resolve relative path** to absolute using the candidate resolution algorithm.
3. If resolved file is **outside `repoRoot`**, skip.
4. Convert to repo-relative POSIX path — that's the key in `deps`.
5. Collect imported symbol names (`default`, `*`, named specifiers). De-duplicate and sort.

### Import Resolution Algorithm

Run candidates in this exact order, return first hit:

```typescript
const base = path.resolve(path.dirname(currentFile), importPath);
const candidates = [
  base,                          // exact
  base + '.ts',
  base + '.tsx',
  base + '.d.ts',
  base + '.js',
  base + '.jsx',
  base + '.mts',
  base + '.cts',
  base + '.mjs',
  base + '.cjs',
  path.join(base, 'index.ts'),
  path.join(base, 'index.tsx'),
  path.join(base, 'index.js'),
  path.join(base, 'index.jsx'),
];
for (const c of candidates) if (existsAndIsFile(c)) return c;
return null;
```

**Note:** `tsconfig.json` `paths` aliases are NOT consulted in v1. Aliased imports (e.g., `@/services/...`) are treated as bare specifiers and skipped.

### Header Detection

A file has a ForgeMap header iff the **first 30 lines** contain a `//` line whose stripped content starts with one of: `exports:`, `used_by:`, `related:`, `rules:`, `agent:`, `message:`. One field marker is the signal.

The parser:
1. Splits source by `\n`. Normalizes CRLF.
2. Skips optional shebang on line 0.
3. Scans downward; recognizes comment lines (`//` prefix).
4. First non-comment non-blank line ends the header block.
5. Recognizes field starts and continuation lines.
6. Returns `ParsedHeader` or `null`.

---

## 8. Graph Construction (`graph.ts`)

### `buildUsedBy` — Reverse Dependency Map

```typescript
export function buildUsedBy(infos: Record<RelPath, FileInfo>): UsedByMap {
  const ub: UsedByMap = {};
  for (const [importer, info] of Object.entries(infos)) {
    for (const [dep, syms] of Object.entries(info.deps)) {
      (ub[dep] ??= {})[importer] = syms;
    }
  }
  return ub;
}
```

Only files in `infos` appear as keys. If a dep points to a file outside the scanned set (e.g., a `.json` file), it is dropped.

### Package Detection

A "package" is any directory subtree containing source files, capped at `PACKAGE_DEPTH` (3) path segments.

**Algorithm:**
1. Take every directory that is the parent of at least one source file.
2. Cap each candidate to 3 path segments (`a/b/c/d/file.ts` → `a/b/c`).
3. Each file belongs to its deepest such candidate. Files at repo root go under the empty-string key `""`.
4. Drop any candidate whose path includes a `SKIP_DIRS` member.

**`depends_on`:** Package A depends on package B iff any file in A imports a file whose package is B. Self-deps excluded. Output sorted, each suffixed with `/`.

**`key_files`:** Rank files by `importerCount * 10 + exportCount`. Take top 5 bare basenames, deduplicated.

**Purpose heuristic (no LLM):**
```typescript
function packagePurposeHeuristic(pkgKey: string, files: RelPath[]): string {
  const stems = files
    .map(f => path.basename(f, path.extname(f)))
    .filter(s => s !== 'index' && !s.startsWith('_'))
    .slice(0, 3)
    .map(s => s.replace(/[-_]/g, ' '));
  if (stems.length === 0) return `${pkgKey || 'root'} package`;
  return `${stems.join(', ')} module`;
}
```

---

## 9. Header Parsing and Formatting (`header.ts`)

### `buildHeader` — Create a Fresh Header

```typescript
export interface BuildHeaderOpts {
  rel: RelPath;
  purpose: string;
  exports: ExportSig[];
  usedBy: Record<RelPath, string[]>;
  related?: string;
  wiki?: string;
  rules: string;
  modelId: string;
  today: string;         // YYYY-MM-DD
  sessionId: string;
}

export function buildHeader(opts: BuildHeaderOpts): string;
```

Output format (note column alignment — agents pattern-match on it):

```
// <rel> — <purpose>.
//
// exports: <fmt-exports>
// used_by: <fmt-used_by>
// related: <related>           (omit line if empty)
// wiki:    <wiki>               (omit line if empty)
// rules:   <rules-line-1>
//          <rules-line-2>       (continuation per newline)
// agent:   <model> | <provider> | <date> | <session> | initial ForgeMap annotation pass
```

### `parseHeader` — Parse Header from Source

```typescript
export function parseHeader(source: string): ParsedHeader | null;
```

Returns `null` if no valid header found in the first `HEADER_SCAN_LINES` lines.

### `rebuildHeader` — Rebuild with New Structural Fields

```typescript
export function rebuildHeader(
  parsed: ParsedHeader,
  newExports: string,
  newUsedBy: string,
): string;
```

Replaces `exports:` and `used_by:` while preserving all other fields (`related`, `wiki`, `rules`, `agent`, `message`) verbatim.

---

## 10. Idempotent Injection (`inject.ts`)

### `injectHeader` — First-Time Injection

```typescript
export function injectHeader(source: string, newHeader: string): string;
```

- Normalizes CRLF → LF.
- If header already exists, returns source unchanged (no duplication).
- Preserves shebang on line 0.
- Ensures exactly one blank line between header and first code.

### `replaceHeader` — Force Replacement

```typescript
export function replaceHeader(source: string, newHeader: string): string;
```

Used by `--force` mode. Replaces existing header with new one.

### `refreshHeader` — Structural-Only Update

```typescript
export interface RefreshResult {
  source: string;
  changed: boolean;
  changedFields: string[];
}

export function refreshHeader(
  source: string,
  newExports: ExportSig[],
  newUsedBy: Record<RelPath, string[]>,
): RefreshResult;
```

**Critical behavior — "never degrade" rule:**
- If new `exports` resolves to `"none"` but existing value has real content, **keep existing**.
- Same for `used_by`.
- This protects against scanner regressions on edge-case files.

Returns `changed: false` and original source if nothing needs updating.

---

## 11. Utility Modules

### `util/fs.ts` — File System Helpers

```typescript
/** Convert absolute path to repo-relative POSIX path. */
export function toPosixRel(absPath: string, repoRoot: string): string;

/** Normalize CRLF and bare CR to LF. Critical before any line splitting. */
export function normalizeLF(source: string): string;

/** Read a file as UTF-8 with LF normalization. Returns null on read error. */
export function safeReadFile(absPath: string): string | null;

/** Write a file atomically (write to tmp, then rename). */
export function safeWriteFile(absPath: string, content: string): void;

/** Check if a path exists and is a regular file. */
export function existsAndIsFile(p: string): boolean;

/** Check if a path exists and is a directory. */
export function existsAndIsDir(p: string): boolean;
```

### `util/fmt.ts` — Formatting Helpers

```typescript
/** Format exports list for header. Pipe-separated, capped at EXPORTS_CAP. */
export function fmtExports(exports: string[]): string;

/**
 * Format used_by map for header.
 * Multi-line, continuation indented to 9 spaces (aligns with `used_by:` value column).
 */
export function fmtUsedBy(ub: Record<string, string[]>): string;

/** Derive provider from model ID string. */
export function detectProvider(modelId: string): string;
// "forgemap-cli (no-llm)" → "forgemap-cli"
// "codedna-cli (no-llm)"  → "forgemap-cli"
// "ollama/..."             → "ollama"
// "gpt..."                 → "openai"
// "claude..."              → "anthropic"
// "gemini/..."             → "gemini"
// "deepseek/..."           → "deepseek"

/** Generate a unique session ID: s_YYYYMMDD_<hex6> */
export function genSessionId(): string;

/** Truncate purpose to at most 15 words. */
export function truncatePurpose(purpose: string): string;

/** Generate heuristic purpose from file basename. */
export function filePurposeHeuristic(rel: string): string;
// "services/revenue.ts" → "revenue module"
```

---

## 12. Commands

### 12.1 `init` — First-Time Annotation Pass

```typescript
export interface InitResult {
  totalFiles: number;
  annotated: number;
  skipped: number;
  errors: number;
}

export function runInit(opts: InitOptions): InitResult;
```

**Pipeline:**
1. `collectFiles(opts)` — gather all source files.
2. For each file: `scanFile(absPath, repoRoot)` → `infos[rel] = info`.
3. `buildUsedBy(infos)` — compute reverse dependency graph.
4. For each `rel` in sorted order:
   - If `info.hasForgeMap && !force`: skip.
   - Build header with `buildHeader(...)`.
   - Inject or replace header in source.
   - Write to disk (unless `dryRun`).

### 12.2 `update` — Incremental Annotation

```typescript
export function runUpdate(opts: InitOptions): InitResult;
// Simply calls runInit({ ...opts, force: false })
```

### 12.3 `check` — Coverage Report

```typescript
export interface CheckResult {
  totalFiles: number;
  annotated: number;
  missing: RelPath[];
  unparseable: RelPath[];
  allAnnotated: boolean;
}

export function runCheck(opts: CheckOptions): CheckResult;
export function formatCheckResult(result: CheckResult, target: string, verbose: boolean): string;
```

Exit code 0 iff all files annotated. Output:
```
ForgeMap Check
Target      <path>
Files       <N>

L1 (module headers)    <annotated>/<total>  (<pct>%)
Unparseable            <n>

Missing L1:           (verbose only)
  src/foo.ts
  src/bar.tsx

OK — fully annotated
```

### 12.4 `refresh` — Structural-Only Update

```typescript
export interface RefreshResult {
  totalFiles: number;
  updated: number;
  unchanged: number;
  skippedNoHeader: number;
  errors: number;
}

export function runRefresh(opts: RefreshOptions): RefreshResult;
```

Re-scans, recomputes `used_by`, rewrites only structural fields (`exports:`, `used_by:`). Never touches `rules:`, `agent:`, `message:`. Files without an existing header are skipped — refresh is not a generator.

### 12.5 `manifest` — Generate `.forgemap`

```typescript
export function runManifest(opts: ManifestOptions): string;
```

Pipeline:
1. Read existing `.forgemap` (or defaults).
2. `collectFiles` → `scanFile` each → `infos`.
3. `buildUsedBy` → `usedBy`.
4. `detectPackages(infos, usedBy)` → packages.
5. For each package: compute keyFiles, dependsOn, purpose (heuristic).
6. `detectProjectMeta(repoRoot)` → fill blanks in project/description.
7. `writeManifest(...)`.

### 12.6 `wiki bootstrap` — Per-File Obsidian Vault

```typescript
export function runWikiBootstrap(opts: WikiBootstrapOptions): { written: number; skipped: number };
```

Emits one markdown page per annotated file under `<outDir>/<rel-path-without-ext>.md`. Also emits `README.md` (index) and `log.md` (manifest contents).

### 12.7 `wiki sync` — Narrative Project Wiki

```typescript
export function runWikiSync(opts: WikiSyncOptions): void;
```

Generates a single markdown file (default `docs/forgemap-wiki.md`) with a 7-section stub template (Identity, L0 Mapping, Semantic Topology, Operational Workflows, Testing, Hotspots, Refresh Protocol).

### 12.8 `install` — Pre-Commit Hook + Tool Prompts

```typescript
export function runInstall(opts: InstallOptions): void;
```

Installs:
- `.git/hooks/pre-commit` — validates staged files have ForgeMap headers. Refuses to overwrite existing hook.
- Tool prompt files (`CLAUDE.md`, `AGENTS.md`, `.github/copilot-instructions.md`) — selectable via `--tools`. Default: `['claude', 'cursor']`.

---

## 13. Manifest System (`.forgemap`)

### Reading

```typescript
export function readExistingManifest(absPath: string, repoRoot: string): ExistingManifest;
```

Uses regex extraction — no YAML library. Extracts:
- `project:` — `^project:\s*(.+)$`
- `description:` — `^description:\s*"?(.+?)"?\s*$`
- `mode:` — `^mode:\s*(\w+)`
- `agent_sessions:` block (from marker to EOF) as raw string.
- `cross_cutting_patterns:` block as raw string.

### Writing

```typescript
export interface WriteManifestOpts {
  absPath: string;
  project: string;
  description: string;
  mode: 'human' | 'semi' | 'agent';
  packages: Record<string, { purpose: string; key_files: string[]; depends_on: string[] }>;
  crossCuttingBlock: string;
  agentSessionsBlock: string;
  dryRun?: boolean;
}

export function writeManifest(opts: WriteManifestOpts): string;
```

Render order is fixed (contract):
```
# .forgemap — ForgeMap project manifest (auto-generated by `forge forgemap manifest`)
project: <project>
description: "<description>"
mode: <mode>

packages:
  <pkg-key>/:
    purpose: "<purpose>"
    key_files: [a.ts, b.ts]
    depends_on: [other/, third/]

<crossCuttingBlock>

agent_sessions:
  ...
```

**Rolling window:** When writing, splits `agentSessionsBlock` on the `  - agent:` marker. If more than `SESSIONS_WINDOW` (3) entries, keeps the trailing 3.

### Project Meta Detection

```typescript
export function detectProjectMeta(repoRoot: string): {
  name: string;
  description: string;
  stack: string[];
};
```

Reads `package.json` at repo root. Strips org scope from name (`@org/foo` → `foo`). Only upgrades project name from directory fallback if `package.json#name` is available and more specific.

---

## 14. Obsidian Wiki Generation

### Per-File Page (`renderFilePage`)

```typescript
export function renderFilePage(rel: RelPath, header: ParsedHeader): string;
```

Page template:
```markdown
<!-- AUTO-GENERATED by `forge forgemap wiki` · edits above AGENT NOTES will be overwritten -->

# <rel>

> <firstLine>

## Exports

- `exportName1`
- `exportName2`

## Used by

- [[src/api/routes|src/api/routes.ts]] → revenueRoute

## Related           (omitted if no related:)
## Extended documentation    (only if wiki: present)
## Rules
## Agent history
## Open messages     (omitted if no message:)

<!-- AGENT NOTES · edits below survive `forge forgemap wiki bootstrap` -->
```

**Key behaviors:**
- **Wikilinks:** Obsidian format `[[dir/basename-no-ext|display]]`. Extension stripped from target.
- **Hashtag escaping:** `#1234` → `` `#1234` `` (prevents Obsidian tag pollution).
- **Inline wikilink escaping:** `[[...]]` in rules text wrapped in backticks.
- **Placeholder detection:** Values starting with `none`, `n/a`, `tbd`, `todo` rendered as inline code, not wikilinks.
- **Preservation:** On re-runs, content below `<!-- AGENT NOTES -->` marker is preserved from the existing page.

### Bootstrap (`bootstrapWiki`)

```typescript
export function bootstrapWiki(
  infos: Record<RelPath, FileInfo>,
  outDir: string,
  repoRoot: string,
): { written: number; skipped: number };
```

Also emits:
- `<outDir>/README.md` — index grouped by top-level directory.
- `<outDir>/log.md` — `.forgemap` manifest contents in a fenced YAML block.

### Sync (`syncWiki`)

```typescript
export function syncWiki(repoRoot: string, outPath: string): void;
```

Generates a single narrative project wiki with a 7-section template. Enumerates up to 8 top-level directories (filtering `SKIP_DIRS` and dotfiles). Preserves AGENT NOTES on re-runs.

---

## 15. Pre-Commit Hook & Tool Prompts

### Pre-Commit Hook

Bash script that:
1. Gets list of staged `.ts`/`.tsx`/`.js`/`.jsx`/`.mjs`/`.cjs`/`.mts`/`.cts` files.
2. For each, checks first 30 lines for ForgeMap field markers (`exports:`, `used_by:`, `rules:`, `agent:`).
3. Exits 1 if any file is missing a header.
4. Provides escape hatch: `git commit --no-verify`.
5. Tells developer how to fix: `forge forgemap init <path>`.

### Tool Prompt (ForgeMap Reading Protocol)

Content installed to `CLAUDE.md` / `AGENTS.md`:

```markdown
# ForgeMap Reading Protocol

## Before editing a file
1. Read the module header — the `//` comment block at the top.
2. Check `used_by:` to find all callers affected by your changes.
3. Check `rules:` for hard constraints before writing logic.
4. Check `related:` for files sharing patterns without importing each other.

## After editing a file
1. Append an `agent:` line (rolling window of last 5 entries).
2. Update `message:` if something is worth flagging for the next agent.
3. Do NOT modify `exports:` or `used_by:` manually — run `forge forgemap refresh`.
```

---

## 16. CLI Surface

Mounted as a subcommand group under `forge forgemap`:

```
forge forgemap init <path>                  First-time annotation pass.
forge forgemap update <path>                Annotate only files missing a header.
forge forgemap check <path>                 Coverage report. Exit 1 if incomplete.
forge forgemap refresh <path>               Update exports:/used_by: only. No LLM.
forge forgemap manifest <path>              (Re)generate .forgemap at the project root.
forge forgemap wiki bootstrap [path] --out <dir>    Emit per-file Obsidian vault.
forge forgemap wiki sync [path] --out <file>        Regenerate narrative project wiki.
forge forgemap install                      Install pre-commit hook + tool prompt files.
```

### Common Flags

| Flag | Commands | Description |
|------|----------|-------------|
| `--repo-root <path>` | All | Repository root directory |
| `--exclude <patterns...>` | All except install | Glob patterns to exclude |
| `--extensions <exts...>` | All except install | File extensions to include |
| `--dry-run` | init, update, refresh, manifest | Preview changes without writing |
| `--force` | init only | Re-annotate already-annotated files |
| `-v, --verbose` | All except install | Verbose output |
| `--model <id>` | init, update | Model ID for agent: line (default: `forgemap-cli (no-llm)`) |
| `--session-id <id>` | init, update | Session ID (default: auto-generated) |
| `--tools <tools...>` | install only | Tool prompt files to install (`claude`, `cursor`, `copilot`) |

### Registration

The CLI is registered via `commander`:

```typescript
import type { Command } from 'commander';
export function registerForgemapCommand(program: Command): void;
```

Called from `bin/forge.js`:
```javascript
import { registerForgemapCommand } from '../src/forgemap/cli.js';
registerForgemapCommand(program);
```

---

## 17. Electron IPC Integration

ForgeMap exposes two IPC channels for use from the Electron renderer process.

### IPC Channels

```typescript
// common/constants/ipc-channels.ts
FORGEMAP: {
  GET_STATS: 'forgemap:get-stats',
  INIT: 'forgemap:init',
}
```

### Handler Registration

```typescript
// electron/handlers/forgemap-handlers.ts
export function registerForgeMapHandlers(): void;
```

Called from `electron/handlers/index.ts` during app startup.

### `GET_STATS` Handler

Collects, scans all files, builds `usedBy`, detects packages, and aggregates into `ForgeMapStats`:

```typescript
ipcMain.handle(IPC.FORGEMAP.GET_STATS,
  async (_event, workspacePath: string): Promise<ForgeMapStats | null> => {
    // 1. collectFiles({ target: workspacePath, repoRoot: workspacePath })
    // 2. scanFile each → infos
    // 3. buildUsedBy(infos)
    // 4. detectPackages(infos, usedBy)
    // 5. Count dep edges, coverage %, top packages
    // 6. Check for .forgemap manifest
    // 7. Return ForgeMapStats
  }
);
```

Handles missing TypeScript compiler gracefully (logs warning once, continues with header-only detection).

### `INIT` Handler

Runs the init pipeline on the workspace:

```typescript
ipcMain.handle(IPC.FORGEMAP.INIT,
  async (_event, workspacePath: string): Promise<ForgeMapInitResult> => {
    // Calls runInit({ target: workspacePath, repoRoot: workspacePath, force: false, verbose: true })
    // Returns success, counts, and duration
  }
);
```

### Preload Bridge

```typescript
// electron/preload.ts
const api: VibeForgeAPI = {
  // ...
  getForgeMapStats: (workspacePath: string) =>
    ipcRenderer.invoke(IPC.FORGEMAP.GET_STATS, workspacePath),
  initForgeMap: (workspacePath: string) =>
    ipcRenderer.invoke(IPC.FORGEMAP.INIT, workspacePath),
};
```

### Renderer API Type

```typescript
// common/types.ts
export interface VibeForgeAPI {
  // ...
  getForgeMapStats: (workspacePath: string) => Promise<ForgeMapStats | null>;
  initForgeMap: (workspacePath: string) => Promise<ForgeMapInitResult>;
}
```

---

## 18. UI Integration (StatusBar)

ForgeMap is surfaced in the application's **footer StatusBar** as a compact metrics chip. There is no dedicated view, route, or Redux slice — the StatusBar uses local React state (`useState`/`useRef`).

### StatusBar Chip

- Displays `FM x%` when stats are loaded (coverage percentage).
- Displays `ForgeMap: Init` as an action button when no annotations exist.
- Clicking opens a popup with detailed stats.

### Popup Contents

- **Coverage bar:** `annotatedFiles / totalFiles (coveragePercent%)`
- **Package count:** number of detected packages
- **Export count:** total exports across all files
- **Dep edge count:** total import edges in the dependency graph
- **Top packages:** up to 5, each with file count and key files
- **Manifest indicator:** whether `.forgemap` exists
- **Last scan duration:** milliseconds
- **Actions:**
  - **Refresh** — re-fetch stats
  - **Init** — run `initForgeMap()` on the workspace
  - **"What is ForgeMap?"** — toggles an info panel with the reading protocol

### State Management

```typescript
const [forgeMapStats, setForgeMapStats] = useState<ForgeMapStats | null>(null);
const [forgeMapInitRunning, setForgeMapInitRunning] = useState(false);
const [forgeMapRefreshing, setForgeMapRefreshing] = useState(false);
const [showForgeMapPopup, setShowForgeMapPopup] = useState(false);
const [showForgeMapInfo, setShowForgeMapInfo] = useState(false);
const forgeMapRef = useRef<HTMLDivElement>(null);
```

Stats are fetched on mount and re-fetched when the popup opens. Click-outside closes the popup.

---

## 19. Critical Algorithms & Invariants

These rules were discovered through bugs in the original Python implementation. Do not re-discover them.

### R1: CRLF Normalization

```typescript
source = source.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
```

**Always** normalize before any line splitting. Mixed line endings corrupt writes.

### R2: Agent Line Format

The `agent:` line **must** have exactly 5 pipe-separated fields:
```
model | provider | date | session | narrative
```
Downstream parsers split by `|` and count.

### R3: Message Field Position

`message:` can live inside an `agent:` entry as a continuation, or as a top-level field. Both are valid. The CLI emits the latter; readers handle both.

### R4: Never Degrade

Refresh never degrades to `"none"` if a real value is present. If the AST extractor returns empty exports but the existing header has real content, keep the existing value.

```typescript
if (newValue === 'none' && existingValue.trim() !== '' && existingValue.trim().toLowerCase() !== 'none') {
  return existingValue;
}
```

### R5: Session Rolling Window

`agent_sessions:` is append-only with rolling window of 3. Split on `  - agent:` marker and keep trailing entries.

### R6: Cross-Cutting Patterns Preservation

`cross_cutting_patterns:` is preserved **verbatim** across `manifest` runs. Users may write free-form notes there.

### R7: Idempotent Injection

Inject only when missing (init) **or** when refreshing structural fields. Never duplicate headers.

### R8: Export Cap

Cap `exports:` at 20 entries with `(+N more)` suffix for readability.

### R9: Purpose Brevity

Purpose line must be ≤ 15 words. Enforces concision, keeps headers scannable.

### R10: Deterministic Wikilinks

Wikilink slugs are deterministic from the rel path with extension stripped. Otherwise the Obsidian graph silently breaks.

### R11: Deduplication

```typescript
deps[key] = Array.from(new Set(deps[key])).sort();
```

Same import may be visited multiple times (static + dynamic). Always de-duplicate.

### R12: Atomic Writes

Use write-then-rename pattern to prevent corruption:
```typescript
const tmp = absPath + '.forgemap-tmp';
fs.writeFileSync(tmp, content, 'utf8');
fs.renameSync(tmp, absPath);
```

---

## 20. Acceptance Tests

When porting ForgeMap, these scenarios must work end-to-end:

1. **`forge forgemap init src/`** annotates every `.ts`/`.tsx` file under `src/` with a Level 1 header. Re-running it is a no-op.
2. **`forge forgemap init src/ --force`** rewrites every header.
3. **`forge forgemap update src/`** annotates only files that lacked a header. Files with a header are untouched.
4. **`forge forgemap check src/`** exits 0 when fully annotated, 1 otherwise. With `-v` it lists missing files.
5. **`forge forgemap refresh src/`** updates `exports:` and `used_by:` for files whose API or callers changed; never touches `rules:`/`agent:`/`message:`. Files with no changes report "unchanged".
6. **`forge forgemap manifest .`** generates `.forgemap` at the repo root. Running twice is idempotent except for the auto-generated header. Existing `agent_sessions:` and `cross_cutting_patterns:` blocks survive verbatim.
7. **`forge forgemap wiki bootstrap . --out docs/wiki`** emits one markdown page per annotated file plus `README.md` and `log.md`. Re-running preserves content under `<!-- AGENT NOTES -->` markers.
8. **`forge forgemap install`** drops a pre-commit hook and `CLAUDE.md`/`AGENTS.md` at the repo root if absent. Idempotent.
9. **A staged commit** of an unannotated `.ts` file is blocked by the pre-commit hook with a clear message.
10. **Round-trip safety:** parse a known-good file's header, re-format it, write it back, parse again — the second parse must equal the first byte-for-byte (after CRLF normalization).

---

## 21. Implementation Order

If porting ForgeMap to another application, follow this order:

1. **`types.ts` + `constants.ts`** — write these first; everything depends on them.
2. **`util/fs.ts` + `util/fmt.ts`** — file system and formatting helpers.
3. **`collect.ts`** — file walking. Smoke-test it returns the right set on `src/`.
4. **`scan.ts`** — the AST extractor. Most of the work. Write fixture-driven tests (a tiny TS file → snapshot the `FileInfo`).
5. **`header.ts`** — parse + format. Write round-trip tests: parse → format → parse → assert equal.
6. **`inject.ts`** — idempotent injection. Test "inject twice yields the same source" explicitly.
7. **`graph.ts`** — `buildUsedBy`, `detectPackages`, `dependsOn`. Easy once 1–4 are solid.
8. **`commands/init.ts`, `check.ts`, `refresh.ts`, `update.ts`** — thin pipelines on top of the above.
9. **`manifest-io.ts` + `commands/manifest.ts`** — line parser + writer. Test preservation of `agent_sessions:` and `cross_cutting_patterns:` blocks across a write cycle.
10. **`wiki-emitter.ts` + `commands/wiki.ts`** — page renderer + bootstrap. Lower priority.
11. **`hook.ts` + `commands/install.ts`** — pre-commit hook + tool prompts. Last.
12. **`cli.ts`** — CLI wiring with `commander`.
13. **IPC handlers** — Electron integration (optional, depends on app framework).
14. **UI (StatusBar)** — React component for metrics display (optional).

A reasonable v1 ships steps 1–9 + CLI. Wiki and install can be a follow-up.

### Dependencies

- `typescript` — for AST parsing (dynamic require, optional at runtime).
- `minimatch` — for glob pattern matching on exclude patterns.
- `commander` — for CLI argument parsing (if the host app already uses it).
- `node:fs`, `node:path`, `node:crypto` — standard Node.js modules.

No YAML library. No LLM calls. No network calls.

---

*This document is self-contained. Read it top to bottom before writing any code. Re-implement against this spec — do not clone the upstream Python repo.*
