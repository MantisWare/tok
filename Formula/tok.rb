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
      tok #{version} — Token Optimization Kit
      Squeeze noisy CLI output before it hits your LLM

    ── Quick Start ─────────────────────────────────────

      # 1. Install for your AI tool
      tok init -g                  # Claude Code (recommended)
      tok init -g --gemini         # Gemini CLI
      tok init -g --codex          # Codex (OpenAI)
      tok init -g --agent cursor   # Cursor

      # 2. Restart your AI tool, then test
      tok --version                # Verify installation
      tok gain                     # View token savings

    ── What It Does ──────────────────────────────────

      tok sits between your shell and your LLM, filtering
      command output for 60-90% token savings:

      tok git status          # Compact status
      tok cargo test          # Failures only (-90%)
      tok ls .                # Token-optimized tree
      tok grep "pattern" .    # Grouped results

    ── Resources ─────────────────────────────────────

      Docs:   https://github.com/MantisWare/tok
      Help:   tok --help
      Issues: https://github.com/MantisWare/tok/issues

    EOS
  end

  test do
    assert_match "tok #{version}", shell_output("#{bin}/tok --version")
  end
end
