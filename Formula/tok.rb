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
      url "https://github.com/MantisWare/tok/releases/download/v#{version}/tok-x86_64-unknown-linux-gnu.tar.gz"
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

  test do
    assert_match "tok #{version}", shell_output("#{bin}/tok --version")
  end
end
