# OpenCode Hooks

> Part of [`hooks/`](../README.md) — see also [`src/hooks/`](../../src/hooks/README.md) for installation code

## Specifics

- TypeScript plugin targeting the **OpenCode V2 plugin API** (V1 plugin
  implementations do not run in V2)
- Default export is a plain `{ id, setup }` definition — no
  `@opencode/plugin` import, because `tok init -g --opencode` writes this
  single file into `~/.config/opencode/plugins/` without a node_modules tree
  and the OpenCode loader resolves no packages from there
- Registers `ctx.tool.hook("execute.before", ...)` (the V2 equivalent of the
  V1 `tool.execute.before` string hook) and calls `tok rewrite` as a
  subprocess via `node:child_process` (the V1 `$` shell helper is no longer
  injected into the plugin context)
- Silently ignores failures — rewrite errors pass the command through
  unchanged
- Mutates `event.input.command` in-place if the rewrite differs from the
  original
- Sets `TOK_CLIENT=opencode` on the subprocess for client attribution
- Installed to `~/.config/opencode/plugins/tok.ts` by `tok init -g --opencode`