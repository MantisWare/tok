# Agent Memory (`tok memory`)

Local-first conversation memory: rules, preferences, project facts. **Not** structural code memory — use `tok mem` for symbols and call graphs.

## Layout

| Path | Role |
|------|------|
| `types.rs` | Records, scope, memory types |
| `config.rs` | `[memory]` in `config.toml` |
| `provider.rs` | `TokMemoryProvider` trait |
| `sqlite/` | Default backend (`tok-memory.db`) |
| `retrieval/` | Core + FTS + hybrid scoring |
| `context/` | Token-budgeted context pack for hooks |
| `extraction/` | Heuristic extract + async queue |
| `cli/` | `tok memory` subcommands |
| `service.rs` | Shared entry for CLI and hooks |

## Hooks

- `tok hook memory-retrieve --json` — sessionStart injection
- `tok hook memory-extract` — stdin `{"user":"...","assistant":"..."}`

## Docs

- User: [README.md](../../README.md#tok-memory--agent-memory-rules--preferences)
- Design: [docs/TOK_Memory_Gateway_Mem0_Inspired_Architecture.md](../../docs/TOK_Memory_Gateway_Mem0_Inspired_Architecture.md)
- Architecture: [docs/contributing/ARCHITECTURE.md](../../docs/contributing/ARCHITECTURE.md#agent-memory-gateway)
