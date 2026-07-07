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
