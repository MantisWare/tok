// TOK OpenCode plugin — rewrites commands to use tok for token savings.
// Requires: tok >= 0.1.23 in PATH (rewrite engine + TOK_CLIENT attribution).
// Requires: OpenCode V2 (V1 plugin implementations do not run in V2).
//
// This is a thin delegating plugin: all rewrite logic lives in `tok rewrite`,
// which is the single source of truth (src/discover/registry.rs).
// To add or change rewrite rules, edit the Rust registry — not this file.
//
// Deliberately dependency-free: OpenCode validates the default export
// directly, and `tok init -g --opencode` installs this file without a
// node_modules tree, so importing `@opencode/plugin` is not an option.
// The V2 loader requires a default export with an `id` and a `setup` (or
// `effect`) function; `Plugin.define` from `@opencode/plugin` is just a
// typed wrapper around that shape.

import { execFile } from "node:child_process"
import { promisify } from "node:util"

const run = promisify(execFile)

/** Loose shape of the V2 tool hook event; OpenCode passes a mutable draft. */
interface TokToolHookEvent {
  tool?: string
  input?: Record<string, unknown>
}

export default {
  id: "tok",
  async setup(ctx: { tool: { hook(name: string, fn: (event: TokToolHookEvent) => Promise<void>): Promise<unknown> } }) {
    // Verify tok is present before registering the hook. The V1 `$` shell
    // helper is no longer injected into the plugin context, so use execFile.
    try {
      await run("which", ["tok"])
    } catch {
      console.warn("[tok] tok binary not found in PATH — plugin disabled")
      return
    }

    // V1 "tool.execute.before" maps to ctx.tool.hook("execute.before", ...).
    await ctx.tool.hook("execute.before", async (event) => {
      const tool = String(event.tool ?? "").toLowerCase()
      if (tool !== "bash" && tool !== "shell") return

      const args = event.input
      const command = args?.command
      if (typeof command !== "string" || !command) return

      try {
        const result = await run("tok", ["rewrite", command], {
          env: { ...process.env, TOK_CLIENT: "opencode" },
        })
        const rewritten = result.stdout.trim()
        if (rewritten && rewritten !== command) {
          args.command = rewritten
        }
      } catch {
        // tok rewrite failed — pass through unchanged
      }
    })
  },
}