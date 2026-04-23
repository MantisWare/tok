# typed: false
# frozen_string_literal: true

# Homebrew formula for tok - Token Optimization Kit
# To install: brew tap MantisWare/tap && brew install tok
class Tok < Formula
  desc "High-performance CLI proxy to minimize LLM token consumption"
  homepage "https://github.com/MantisWare/tok"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/MantisWare/tok/releases/download/v#{version}/tok-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_INTEL"
    end

    on_arm do
      url "https://github.com/MantisWare/tok/releases/download/v#{version}/tok-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_ARM"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/MantisWare/tok/releases/download/v#{version}/tok-x86_64-unknown-linux-musl.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_INTEL"
    end

    on_arm do
      url "https://github.com/MantisWare/tok/releases/download/v#{version}/tok-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_ARM"
    end
  end

  def install
    bin.install "tok"
  end

  def caveats
    <<-'CAVEATS'

    ████████╗  ██████╗   ██╗  ██╗
    ╚══██╔══╝ ██╔═══██╗  ██║ ██╔╝
       ██║    ██║   ██║  █████╔╝
       ██║    ██║   ██║  ██╔═██╗
       ██║     ╚████╔╝   ██║  ██╗
       ╚═╝      ╚═══╝    ╚═╝  ╚═╝

    CAVEATS
    .chomp + "\n" + <<~EOS

      T O K  v#{version} — Token Optimization Kit
      Squeeze noisy CLI output before it hits your LLM

      Author: MantisWare (Waldo Marais)

    ── Setup ───────────────────────────────────────────

      tok init -g                  # Claude Code (recommended)
      tok init -g --agent cursor   # Cursor
      tok init -g --gemini         # Gemini CLI
      tok init --codex             # Codex CLI
      tok init -g --opencode       # OpenCode
      tok init --copilot           # GitHub Copilot
      tok init --all               # ALL agents at once

    ── Usage ───────────────────────────────────────────

      tok <command>                # Any command — auto-filtered
      tok git status               # Git without the wall of text
      tok cargo test               # Test output, failures only
      tok gain                     # Token savings stats
      tok gain --graph             # ASCII graph of daily savings
      tok discover                 # Find missed TOK opportunities
      tok proxy <cmd>              # Passthrough (still tracks stats)
      tok --help                   # All commands and flags

    ── Resources ───────────────────────────────────────

      Docs:   https://github.com/MantisWare/tok
      Help:   tok --help
      Issues: https://github.com/MantisWare/tok/issues

    EOS
  end

  test do
    assert_match "tok #{version}", shell_output("#{bin}/tok --version")
  end
end
