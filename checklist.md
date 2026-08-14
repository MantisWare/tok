# TOK gain — client reporting checklist

> **Confidence:** 94%

## Phase 1 (implemented)

- [x] `client` column on `commands` in `history.db`
- [x] Record client from `TOK_CLIENT` env on each `tok` invocation
- [x] Hooks prefix rewritten commands (`TOK_CLIENT=<id> tok …`) and export `TOK_CLIENT` before `tok rewrite`
  - Claude Code (`claude`), Cursor (`cursor`), Copilot (`copilot`), Gemini (`gemini`), OpenCode (`opencode`)
- [x] `tok rewrite` applies client prefix via `apply_hook_client_prefix` (`src/hooks/rewrite_cmd.rs`)
- [x] `tok hook copilot` / `tok hook gemini` tag rewrites in Rust (`prefix_command_with_client`)
- [x] `src/core/client.rs` — normalize ids, prefix helpers, legacy empty → `unknown` display
- [x] `tok gain` shows **By Client** in the default summary
- [x] `tok gain --by-client` — client breakdown only
- [x] JSON export includes `by_client` array
- [x] Tests: `test_tracker_client_attribution` (unit), `gain_by_client` CLI smoke (`tests/cli/test_gain.rs`)

### Client ids

| Id | Source |
|----|--------|
| `cursor` | `hooks/cursor/tok-rewrite.sh` |
| `claude` | `hooks/claude/tok-rewrite.sh`, `.claude/hooks/tok-rewrite.sh` |
| `copilot` | `tok hook copilot` (VS Code + Copilot CLI) |
| `gemini` | `tok hook gemini` |
| `opencode` | `hooks/opencode/tok.ts` |
| `terminal` | Direct shell / no `TOK_CLIENT` (default) |
| `unknown` | Rows recorded before client tracking (empty `client` column) |

Re-run `tok init -g` (or your agent-specific init) so installed hooks pick up `TOK_CLIENT` exports.

---

## Phase 2 (future) — LLM API usage by client

Possible follow-ups (not implemented):

- [ ] **Cursor API spend** — ingest Cursor usage/billing if a stable API or export exists; correlate with TOK filter savings by date
- [ ] **Claude Code** — extend `tok cc-economics` with per-session client labels (today: global ccusage + TOK savings)
- [ ] **Copilot / Gemini / other hosts** — provider-specific usage sources where available
- [ ] **Unified dashboard** — one view: API tokens consumed vs TOK output tokens saved, per client and per day
- [ ] **Session join** — link `history.db` rows to agent session IDs when transcripts expose them (Claude JSONL today; Cursor TBD)
- [ ] **Backfill** — optional heuristic backfill of `unknown` rows from hook audit logs (`TOK_HOOK_AUDIT=1`) if present

---

# Code Graph — graft port

> Port graft's tree-sitter context-graph engine into TOK, upgrading `tok mem` in
> place. Strictly additive: every existing command keeps its flags, output, and
> exit codes.

## Phase 0a — Regression baseline (complete)

- [x] Multi-language fixture repo at `tests/fixtures/code_graph/` (Rust, TypeScript, Python, Go)
- [x] Golden snapshot of all `tok mem` subcommand output (`tests/cli/test_mem_baseline.rs`)
- [x] Exit-code contract test — not-found paths return `1`
- [x] `memory.db` schema snapshot (`schema_baseline` in `src/mem/db.rs`)
- [x] Pinned today's `--incremental` behaviour so Phase 2's fix is visible in review
- [x] Guard that the fixture lands outside any git repo (keeps `evolution`/`timeline` snapshots stable)
- [x] Startup benchmark gate (`scripts/bench-startup.sh`, requires `hyperfine`)
- [x] `insta` added as a dev-dependency

### What the baseline proved

The snapshot confirms the defects that motivate this work, on a 6-file fixture:

| Symptom | Evidence |
|---|---|
| No call graph | 43 symbols but only **3 edges**, all `IMPLEMENTS` |
| `relations` non-functional | "No find_callers relationships found" |
| `impact` non-functional | "No impact detected" |
| `dead-code` unreliable | **30 of 43** symbols flagged, including `normalize` which `slugify` calls two lines below |
| Symbol ID collisions | `trait Store::get` and `impl Store for MemoryStore::get` hash identically; index reports 43, table holds 42 |
| No real spans | `line_end` equals `line_start` throughout |

### Recorded baseline numbers (release build, macOS arm64)

| Metric | Value | Budget |
|---|---|---|
| Binary size | **8.21 MB** | documented target is <5 MB (already exceeded before this work) |
| Startup, `tok --version` | **5.31 ms** mean / 5.30 ms median / 4.63 ms min over 50 runs | <10 ms |

Re-measure both at the end of Phase 9. `hyperfine` was not installed locally, so
these came from a 50-run Python harness; `scripts/bench-startup.sh` is the
preferred tool once `hyperfine` is available.

## Phase 0b — Scaffolding (complete)

- [x] tree-sitter + grammar dependencies, `graph` feature in the **default** set
- [x] Per-language sub-features (`lang-typescript`, `lang-python`, `lang-go`, `lang-rust`)
- [x] `--no-default-features` and single-language builds verified to compile clean
- [x] ABI skew resolved — `tree-sitter` 0.26.12 verified against all four grammars
- [x] `src/graph/` module with `types.rs` (format) and `lang.rs` (detection + grammars)
- [x] `src/query/` module with graft's ranking constants pinned
- [x] `GraphV1` / `NodeV1` / `EdgeV1` serde types with deterministic `normalize()`
- [x] Additive `graph_id` column on `symbols`, plus an idempotent `ALTER TABLE` upgrade path
- [x] `mem-ast` retained as an alias for `graph` so existing build scripts keep working

Resolved versions: `tree-sitter` 0.26.12, `tree-sitter-typescript` 0.23.2,
`tree-sitter-python` 0.25.0, `tree-sitter-go` 0.25.0, `tree-sitter-rust` 0.24.2 —
all sharing `tree-sitter-language` 0.1.7. The `every_enabled_grammar_parses_a_sample`
test parses a real snippet per language, so ABI drift fails the suite rather than
degrading silently at runtime.

Two decisions worth noting, both made to protect existing behaviour:

- **`symbols.id` is unchanged.** The graph's readable ids live in the new
  `graph_id` column instead. Rewriting `symbols.id` would orphan every row in
  `episodes`, which references it.
- **`EXTENDS` projects into SQLite as `IMPLEMENTS`.** `tok mem relations
  --query-type class_hierarchy` filters on `IMPLEMENTS` alone, and the regex
  parser already stored `extends` that way. The graph keeps the two distinct;
  only the projection merges them.

## Phase 1 — Extraction core (complete)

- [x] tree-sitter extraction for TypeScript/TSX/JS, Python, Go
- [x] tree-sitter extraction for Rust (beyond graft's coverage; needed to index TOK itself)
- [x] ID minting with `~N` dedup ordinals (fixes the collision above)
- [x] Receiver-type inference (`bindings`)
- [x] Edge resolution with typed member dispatch, depth-3 extends BFS, ambiguity drop
- [x] `parser_regex` retained as fallback and for ForgeMap
- [x] Byte-safe source slicing so multi-byte UTF-8 cannot panic under `panic = "abort"`
- [x] Import specifier resolution (`src/graph/modpath.rs`) so `./util` narrows to one file
- [x] Named ownership for Go methods declared outside their receiver type

Three problems surfaced during implementation that the plan did not anticipate:

- **Methods whose owner is not lexical.** Go writes `func (c *Cache) Get()`
  outside the struct body, so the walker has no enclosing type to attribute the
  method to — and the receiver type may even live in another file of the same
  package. Ownership is therefore recorded as a *name* during extraction and
  resolved to an id afterwards, which is also what makes `CONTAINS` come out
  identical across all four languages.
- **Globally unique names are too strict.** `Cache` exists in three fixture
  languages at once, so a repo-wide uniqueness test resolved nothing. Lookups
  for inherently local relations (a Go receiver, a Rust `impl`) now prefer the
  same file, then the same directory and language, before giving up.
- **Bare specifiers are ambiguous across languages.** `react` and `util` are the
  same string; only the importing file's extension says whether it names a
  package or a sibling module, so dotted-module resolution is gated on Python.

### Non-regression: a pre-existing determinism bug, fixed

The Phase 0a baseline started failing with `bridges` naming `python/cache.py`
where it had recorded `ts/cache.ts`. The cause was in existing code, not the
graph: `detect_communities` walked a `HashMap`/`HashSet`, and both it and
`find_bridge_symbols` sorted on score alone. Rust randomises hash iteration per
process, so tied results — and the five members a community reports — varied
between identical runs.

- [x] `detect_communities` walks ordered maps and sorts component members
- [x] `find_bridge_symbols` collects into a `BTreeMap` and tie-breaks on path, then name
- [x] `analyze_complexity` tie-breaks on path, then line
- [x] Four regression tests in `src/mem/quality.rs` run each query 25 times over a tied fixture

The baseline snapshot was re-recorded, since the value it captured was one
arbitrary outcome of the old behaviour rather than a specification.

## Phase 2 — Build pipeline, caching, SQLite projection (complete)

- [x] Deterministic graph write/load (`write.rs` / `load.rs`, atomic temp+rename)
- [x] Extract cache keyed by content hash + extractor stamp (`cache.rs`)
- [x] Fingerprint drift probe (size+mtime fast path, hash on suspect) (`fingerprint.rs`)
- [x] Refresh lock protocol, restricted to query commands only (`refresh.rs`)
- [x] Dual-write projection into `symbols`/`edges` with the frozen ID formula (`project.rs`)
- [x] Genuinely incremental `--incremental`, including removal of deleted files
- [x] `TOK_GRAPH_NO_REFRESH` and `TOK_GRAPH_REFRESH=hash` honoured
- [x] Regex extractor retained for languages without a grammar (Ruby, C#, Java, …)

The pipeline is wired into `tok mem index`, so the graph is built and projected
on every index with no new flags. Files split by language: tree-sitter handles
TypeScript/TSX/JS, Python, Go, and Rust; everything else goes to `parser_regex`
exactly as before. The two sets are disjoint, so no file is parsed twice.

Every cache decision is keyed on **content hash plus extractor stamp**, never on
mtime alone. A checkout that rewrites timestamps costs nothing, a moved file
reuses its entry, and changing extraction logic invalidates the cache instead of
silently serving results from the previous implementation.

### Measured effect on the Phase 0a baseline

All 18 subcommands still run, with unchanged output shape and exit codes. What
changed is the content, and every difference is an improvement the plan
predicted:

| Command | Before | After |
| --- | --- | --- |
| `index` | 43 symbols, 3 edges | 45 symbols, 28 edges |
| `impact Cache` | "No impact detected" | 5 affected symbols across 2 hops |
| `central` | 6 symbols, all score 1 | 20 symbols with real in/out degrees |
| `communities` | "No communities detected" | 4 communities |
| `dead-code` | 30 symbols | 15 symbols |
| `detect` | `get`/`put` as `Function` | correctly `Method` |

Two of those deserve explanation rather than celebration:

- **`dead-code` halving is a false-positive fix, not lost coverage.** The regex
  indexer produced no `CALLS` edges at all, so every method looked unreferenced.
  The 15 that remain are genuinely unreferenced within the fixture.
- **Complexity scores dropped for small functions** because `analyze_complexity`
  falls back to scanning `line_start + 100` whenever `line_end <= line_start`.
  With the regex indexer's collapsed spans that fallback always fired, so every
  symbol was counting branches belonging to *neighbouring* functions. Real spans
  confine the count to the actual body. The fallback is still needed for
  regex-parsed languages and was left in place.

The baseline snapshot was re-recorded against this output. `class_hierarchy`
still reports nothing for `Cache` because the name resolves to the Go struct,
which declares no inheritance — identical to the pre-graph baseline, so not a
regression.

## Phase 3 — Retrieval layer (ask/skeleton/grep/map complete)

- [x] Ask index sidecar (`ask-index.json`, field weights folded in at write time)
- [x] `tok mem ask` — IDF + BM25 + personalized PageRank, structural and lexical modes
- [x] `tok mem skeleton`, `tok mem grep`, `tok mem map`
- [x] Shared identifier tokenizer used by both the index and the query path
- [x] RRF fusion (`query/fuse.rs`), ready for the multi-scope phase
- [x] Bounded graph traversal (`query/traverse.rs`)
- [x] Pre-query auto-refresh with `TOK_GRAPH_NO_REFRESH` / `TOK_GRAPH_REFRESH=hash`
- [ ] `tok mem check` — deferred to Phase 4, where drift reporting belongs

All four commands are **new surfaces**. The 18 pre-existing `tok mem`
subcommands keep their SQLite queries and output formats untouched, so an
existing script or agent prompt sees exactly what it saw before.

### Ranking constants

Ported verbatim from graft and collected in `src/query/constants.rs` so a
retrieval-quality change is one reviewable diff rather than a magic number in a
scoring loop: name ×3, path ×2, BM25 k1 1.2 / b 0.75, PageRank damping 0.25 over
25 iterations, structural blend 0.5, rescue floor 0.15, test penalty 0.35, RRF k
60.

Two are worth understanding because they look wrong at a glance:

- **PageRank damping 0.25**, far below the classic 0.85. A code graph is small
  and densely connected, so a long walk converges on the same globally central
  symbols — the logger, the config loader — no matter what was asked. Low
  damping keeps mass near the seeds, which is what query relevance means here.
- **Test penalty 0.35, not 0.** Tests are demoted, never excluded: "how is this
  exercised" is a real question whose answer is a test. `--no-tests` drops them
  for callers who want only implementation.

### Why auto-refresh, and what it refuses to do

Query commands refresh the graph before answering, because an agent edits a file
and immediately asks about it; a stale answer is worse than a slow one. Three
refusals keep that from becoming a liability: it never blocks behind a peer's
build (falls through to the existing graph), never fails a query because a
refresh failed (degrades to the last good graph), and never refreshes when
`TOK_GRAPH_NO_REFRESH=1` is set.

### Token economy, measured on the fixture

`skeleton ts/cache.ts` renders the file's 10 symbols in 10 lines against 48
lines of source, and the gap widens with file size because signatures are
roughly constant per symbol while bodies are not.

**Auto-updated by Cursor:** Checked off the Phase 3 retrieval items on
2026-08-10. `tok mem check` moved to Phase 4 alongside the manifest it reports
drift against.

## Phase 4 — Markdown layer (complete)

- [x] Per-file wiring cards and `INDEX.md` (`tok mem cards`)
- [x] Frontmatter, generated-block markers, and Notes preservation
- [x] Deterministic slug generation, collision-safe and case-insensitive
- [x] `manifest.json` + `tok mem check` drift reporting
- [x] `--strict` for CI

Output lands in `.tok/map/`, which is **committed**, unlike the gitignored
`.tok/graph/` cache. That is the whole point: a card is readable on GitHub, by
an agent with no TOK binary, and by a human who wants to leave a note.

### Notes preservation

Every file is `frontmatter` → generated block → `## Notes`. Regeneration
replaces the first two and never touches the third. Three edge cases are handled
explicitly rather than optimised away, because each is a real thing a person
does:

- **Markers deleted** — the file is treated as hand-written and the generated
  block is prepended, so nothing is lost in either direction.
- **Markers damaged** (start with no end) — the write is refused and reported.
  Guessing where the block ends risks deleting prose that exists nowhere else.
- **Source deleted but the card has notes** — the card is kept and reported as
  orphaned, because the note may be the only record of why the file went away.

Frontmatter sits *above* the generated markers, not inside them. YAML
frontmatter is only recognised as the first bytes of a file; nesting it would
render it as a stray horizontal rule and no note tool would index it.

### Drift, and why it is four categories

`tok mem check` reads the manifest instead of regenerating and diffing, so it is
cheap enough for a pre-commit hook. The categories are separate because the
fixes differ: **content drift** and **coverage** are fixed by `tok mem cards`,
**removed** by the same command's prune step, and **index drift** requires
`tok mem index` first, since regenerating from an outdated extractor would bake
in stale results.

Hashes cover the *generated block only*, so editing Notes never registers as
drift. Reporting it would train people to ignore the check.

**Auto-updated by Cursor:** Checked off Phase 4 on 2026-08-10, including the
`tok mem check` item carried over from Phase 3.

## Phase 5 — MCP server (complete)

- [x] `tok mcp` — stdio JSON-RPC 2.0, protocol `2024-11-05`
- [x] Six tools (`ask`, `skeleton`, `grep`, `map`, `relations`, `check`) with
      `graft_*` aliases
- [x] Pre-query auto-refresh via the shared graph session
- [x] Startup notices bypassed so stdout stays a clean protocol stream

The transport is unforgiving in one specific way: **stdout is the protocol**. A
single stray line — a hook warning, an update notice — lands mid-stream and the
client drops the connection. `tok mcp` is therefore routed around the notices in
`run_cli`, and everything human-readable goes to stderr.

JSON-RPC is hand-rolled rather than taken from an SDK. The surface actually
needed is six methods and one error shape, while every MCP crate brings an async
runtime with it — a fixed cost paid by every `tok git log`, not just by the
server, against a startup budget under 10ms.

### Why the aliases

Each tool is registered twice, under `tok_*` and `graft_*`. Agents already
configured against graft keep working without their prompts being rewritten,
which is the entire reason to accept a second name for the same thing.

### Errors versus results

A tool that ran and found nothing returns text with `isError: false`, not a
JSON-RPC error. Reporting "no matches" as a transport failure makes clients
retry or abort instead of showing the model the answer. Genuine caller mistakes
— unknown tool, missing `name` — stay RPC errors, because the client should
surface those as faults.

**Auto-updated by Cursor:** Checked off Phase 5 on 2026-08-10. Tool set recorded
as implemented; `relations` is the sixth tool, wrapping the Phase 3 traversal.

## Phase 6 — Agent wiring (complete)

- [x] MCP registration for all eight hosts, in each one's own config format
- [x] Graph instruction section in every host instruction file (all but Cursor,
      which has no single file TOK owns)
- [x] `graph-session` / `graph-postedit` / `graph-sync` hook subcommands, reusing
      the agent-memory `additional_context` contract
- [x] Hook registration in host configs — Claude (`SessionStart` +
      `PostToolUse`) and Cursor (`afterFileEdit` + composed `sessionStart`)
- [x] Uninstall parity for everything above, via `tok init --uninstall`

Wiring runs by default on `tok init`; `--no-graph` opts out. An agent that does
not know the graph exists keeps reading whole files, which is the cost this
subsystem was built to remove — so leaving it opt-in would have meant most users
never getting the saving.

### Eight hosts, five config shapes

There is no agreed format. Most take `mcpServers`, VS Code takes `servers` with
an explicit `"type": "stdio"`, OpenCode nests under `mcp.servers` and wants the
executable and its arguments as one array, and Codex is TOML. Two rules hold
across all of them:

- **Merge, never rewrite.** These files hold the user's other servers and their
  editor settings. Registration adds one key and leaves the rest untouched,
  which is also what makes re-running `tok init` safe.
- **Register an absolute path.** An editor launched from a desktop icon inherits
  a minimal `PATH` that often lacks `~/.cargo/bin`, so a bare `tok` works from a
  terminal and silently fails in the GUI.

Codex is appended textually rather than round-tripped through a value tree,
because `config.toml` is hand-edited and a round trip would strip the comments.

### Hook placement

`graph-postedit` is registered against editing tools only — Claude's matcher is
`Edit|Write|MultiEdit|NotebookEdit`, Cursor's event is `afterFileEdit`. It also
declines to build a graph that does not exist yet: a cold build on a large
repository would stall the first edit of a session with no indication why.

Cursor composes graph orientation into the existing `tok-session.sh` rather than
registering a second `sessionStart` entry, because Cursor honours one and the
second would be dropped without a word.

**Auto-updated by Cursor:** Checked off Phase 6 on 2026-08-10. Hook registration
is Claude and Cursor; the remaining hosts get MCP plus instructions, which is
the part that carries the token saving.

## Phase 7 — Optional `--deep` LLM layer (complete)

- [x] OpenAI-compatible and native Anthropic providers over `ureq` (sync, no
      async runtime)
- [x] File summaries and per-symbol crux, cached by content hash
- [x] `[graph.llm]` config section plus `TOK_GRAPH_PROVIDER` / `_MODEL` /
      `_BASE_URL` / `_API_KEY` overrides
- [x] Outbound payloads routed through the existing secret scanner

Enrichment is reached through `tok mem index --deep` rather than a command of
its own, so there is one way to index and one flag that decides whether it costs
money. Plain indexing stays offline, deterministic, and free.

Two things are non-negotiable in this layer. Redaction runs in strict mode on
every outbound payload, because the alternative is shipping a customer's secrets
to a third party on a flag they set once and forgot. And the provider `Client`
implements `Debug` by hand to blank the API key, since the derived one would
print it into any panic message that happens to carry the client along.

Results are keyed by content hash, so re-running `--deep` on an unchanged file
is free rather than billed again.

**Auto-updated by Cursor:** Checked off Phase 7 on 2026-08-10.

## Phase 8 — Monorepo and multi-repo (complete)

- [x] Scope discovery from workspace markers — `pnpm-workspace.yaml`,
      `package.json#workspaces`, Cargo `[workspace] members`, `go.work`, and
      per-directory manifests
- [x] Per-scope ranking with its own IDF, then RRF fusion with a participation
      gate
- [x] `[scope/]` labels in output
- [x] `--in` narrowing to a scope, a path prefix, or a child repository
- [x] Parent-level federation across sibling git repositories

Each sub-project is ranked on its own corpus before fusion. Sharing one corpus
would let a large package's vocabulary set the IDF for a small one, so a query
about "cache" in a monorepo would rank the web app's incidental mentions above
the cache package's actual implementation.

Two guards keep this from over-firing. Scopes are collapsed past depth 2 and
folded back into the root when they hold fewer than five non-file nodes — a
directory with a `package.json` and two constants is not a project worth ranking
separately. And a scope only participates in the fused result if its best hit
clears a strength floor, so a package that merely contains the word does not
push aside the one that implements it.

A repository with no workspace markers stores no scopes at all, which keeps
single-project graphs byte-identical to what they were before this phase.

**Auto-updated by Cursor:** Checked off Phase 8 on 2026-08-10.

## Phase 9 — Savings, tests, docs (complete)

- [x] `TimedExecution` and a savings footer on `ask`, `skeleton`, `grep`, `map`
- [x] Cold-vs-incremental determinism test, plus a rebuild-from-scratch test
- [x] Ranking snapshot pinning the retrieval surface
- [x] Migration test proving `episodes` still resolve after the upgrade
- [x] Full Phase 0a baseline re-run — all five snapshots pass
- [x] Startup benchmark against the <10ms gate
- [x] `--no-default-features` and single-language builds, wired into CI
- [x] README, CLAUDE.md, `docs/contributing/`, CHANGELOG

Retrieval has no raw command to measure against — the thing it replaces is an
agent opening files until it finds what it needs. So the baseline is exactly
that: the source of every file a result points at. The comparison is
conservative in the direction that matters, since it assumes the agent guesses
the right files first try and never reads a wrong one. A query that returns
nothing claims nothing, because an overstated saving makes every other number
less believable.

Measured on the multi-language fixture: 79% on a focused `ask`, 57% on a
`skeleton`, 62% on `map`. A broad `ask` returning twenty symbols across three
files reports 28% — the honest number for a query that touched most of what it
summarised.

Startup is **5.3ms** mean for `tok --version` against the 10ms gate. `hyperfine`
was not installed on the development machine, so this was measured with a
30-iteration timing loop after three warmups; the gate itself is enforced in CI.
The graph adds nothing to startup for unrelated commands, because grammars are
constructed on first extraction rather than at load.

The binary is **13.5MB** against the documented <5MB target. The C grammars
account for the difference and the target predates them; per-language `lang-*`
features exist for anyone who needs a slim build, and `--no-default-features`
drops tree-sitter entirely. The stated target needs revising rather than
pretending it still holds.

### One behaviour change, deliberately

`tok mem index --incremental` used to skip only the pre-index clear: it still
re-parsed every file and left rows behind for files that had been deleted, which
is how `search` and `dead-code` could report code that no longer existed. It is
now genuinely incremental, and deleted files have their rows removed. Results
for repositories with deleted files will differ from previous runs. This is a
fix, but it is a visible one, so it is in the CHANGELOG rather than passing
quietly.

**Auto-updated by Cursor:** Checked off Phase 9 on 2026-08-10, and recorded the
measured savings, startup, and binary size.

## Phase 10 — Post-port review (complete)

A full pass over the finished port, looking for the things a phase-by-phase
build does not catch: behaviour that only shows up on an existing install, and
tests that pass for the wrong reason.

- [x] `tok init --no-graph` no longer installs graph hooks. The flag was parsed
      and then ignored; `graph` is now threaded through the install path.
- [x] MCP aliases renamed to the names graft actually published
      (`graft_find_code`, `graft_file_api`, `graft_find_all`, `graft_repo_map`,
      `graft_trace_calls`, `graft_check_freshness`), so an existing graft agent
      config keeps working.
- [x] `Contains` edges excluded from PageRank and traversal. A file contains
      every symbol in it, so walking containment let the file outrank all of
      them and turned "what does this question touch" into "which file is
      biggest".
- [x] `tok mcp [dir]` accepts the repository to serve, for clients that launch
      the server from their own install directory.
- [x] `tok mcp` skips the telemetry ping, which shares stdout with the JSON-RPC
      stream.
- [x] `tok mem impact` refreshes its SQLite projection before querying, so it
      does not answer "is this safe to change" from a stale index.
- [x] A build without the `graph` feature says so, instead of telling the user
      to run an index that cannot produce a graph.
- [x] **Re-running `tok init` upgrades an existing install.** It previously
      returned early once the rewrite hook was present, so anyone who had run
      `tok init` before the memory or graph hooks existed never received them.
      Each hook family is now reconciled independently.
- [x] Retrieval snapshot regenerated: file nodes no longer displace real symbols
      in structural results.
- [x] Test isolation: the tracking database is bound per-thread under `cfg(test)`,
      so filter tests neither cross-contaminate the tracking tests nor write into
      the developer's real history.

### Known failure, not ours

`test_cc_economics::cc_economics_default` fails on machines where the external
`ccusage` tool returns its newer monthly schema, which no longer carries a
`month` field. It shells out to a third-party binary and predates this work.

**Auto-updated by Cursor:** Added Phase 10 on 2026-08-11 recording the review
findings and their fixes.

> **Auto-updated by Cursor:** Added the Code Graph section and checked off all
> Phase 0a items on 2026-08-10. Checked off Phase 0b and Phase 1, and recorded
> the `quality.rs` determinism fix, on 2026-08-10. Checked off Phase 2 and
> recorded the baseline's measured improvements on 2026-08-10. Checked off
> Phases 7, 8, and 9 on 2026-08-10, completing the port.
