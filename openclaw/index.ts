/**
 * TOK Rewrite Plugin for OpenClaw
 *
 * Transparently rewrites exec tool commands to TOK equivalents
 * before execution, achieving 60-90% LLM token savings.
 *
 * All rewrite logic lives in `tok rewrite` (src/discover/registry.rs).
 * This plugin is a thin delegate — to add or change rules, edit the
 * Rust registry, not this file.
 */

import { execSync } from "node:child_process";

let tokAvailable: boolean | null = null;

function checkTok(): boolean {
  if (tokAvailable !== null) return tokAvailable;
  try {
    execSync("which tok", { stdio: "ignore" });
    tokAvailable = true;
  } catch {
    tokAvailable = false;
  }
  return tokAvailable;
}

function tryRewrite(command: string): string | null {
  try {
    const result = execSync(`tok rewrite ${JSON.stringify(command)}`, {
      encoding: "utf-8",
      timeout: 2000,
    }).trim();
    return result && result !== command ? result : null;
  } catch {
    return null;
  }
}

export default function register(api: any) {
  const pluginConfig = api.config ?? {};
  const enabled = pluginConfig.enabled !== false;
  const verbose = pluginConfig.verbose === true;

  if (!enabled) return;

  if (!checkTok()) {
    console.warn("[tok] tok binary not found in PATH — plugin disabled");
    return;
  }

  api.on(
    "before_tool_call",
    (event: { toolName: string; params: Record<string, unknown> }) => {
      if (event.toolName !== "exec") return;

      const command = event.params?.command;
      if (typeof command !== "string") return;

      const rewritten = tryRewrite(command);
      if (!rewritten) return;

      if (verbose) {
        console.log(`[tok] ${command} -> ${rewritten}`);
      }

      return { params: { ...event.params, command: rewritten } };
    },
    { priority: 10 }
  );

  if (verbose) {
    console.log("[tok] OpenClaw plugin registered");
  }
}
