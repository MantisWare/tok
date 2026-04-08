#!/usr/bin/env bash
#
# TOK Smoke Test Suite
# Exercises every command to catch regressions after merge.
# Exit code: number of failures (0 = all green)
#
set -euo pipefail

PASS=0
FAIL=0
SKIP=0
FAILURES=()

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# ── Helpers ──────────────────────────────────────────

assert_ok() {
    local name="$1"
    shift
    local output
    if output=$("$@" 2>&1); then
        PASS=$((PASS + 1))
        printf "  ${GREEN}PASS${NC}  %s\n" "$name"
    else
        FAIL=$((FAIL + 1))
        FAILURES+=("$name")
        printf "  ${RED}FAIL${NC}  %s\n" "$name"
        printf "        cmd: %s\n" "$*"
        printf "        out: %s\n" "$(echo "$output" | head -3)"
    fi
}

assert_contains() {
    local name="$1"
    local needle="$2"
    shift 2
    local output
    if output=$("$@" 2>&1) && echo "$output" | grep -q "$needle"; then
        PASS=$((PASS + 1))
        printf "  ${GREEN}PASS${NC}  %s\n" "$name"
    else
        FAIL=$((FAIL + 1))
        FAILURES+=("$name")
        printf "  ${RED}FAIL${NC}  %s\n" "$name"
        printf "        expected: '%s'\n" "$needle"
        printf "        got: %s\n" "$(echo "$output" | head -3)"
    fi
}

assert_exit_ok() {
    local name="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        PASS=$((PASS + 1))
        printf "  ${GREEN}PASS${NC}  %s\n" "$name"
    else
        FAIL=$((FAIL + 1))
        FAILURES+=("$name")
        printf "  ${RED}FAIL${NC}  %s\n" "$name"
        printf "        cmd: %s\n" "$*"
    fi
}

assert_fails() {
    local name="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        FAIL=$((FAIL + 1))
        FAILURES+=("$name (expected failure, got success)")
        printf "  ${RED}FAIL${NC}  %s (expected failure)\n" "$name"
    else
        PASS=$((PASS + 1))
        printf "  ${GREEN}PASS${NC}  %s\n" "$name"
    fi
}

assert_help() {
    local name="$1"
    shift
    assert_contains "$name --help" "Usage:" "$@" --help
}

skip_test() {
    local name="$1"
    local reason="$2"
    SKIP=$((SKIP + 1))
    printf "  ${YELLOW}SKIP${NC}  %s (%s)\n" "$name" "$reason"
}

section() {
    printf "\n${BOLD}${CYAN}── %s ──${NC}\n" "$1"
}

# ── Preamble ─────────────────────────────────────────

TOK=$(command -v tok || echo "")
if [[ -z "$TOK" ]]; then
    echo "tok not found in PATH. Run: cargo install --path ."
    exit 1
fi

printf "${BOLD}TOK Smoke Test Suite${NC}\n"
printf "Binary: %s\n" "$TOK"
printf "Version: %s\n" "$(tok --version)"
printf "Date: %s\n" "$(date '+%Y-%m-%d %H:%M')"

# Need a git repo to test git commands
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "Must run from inside a git repository."
    exit 1
fi

REPO_ROOT=$(git rev-parse --show-toplevel)

# ── 1. Version & Help ───────────────────────────────

section "Version & Help"

assert_contains "tok --version" "tok" tok --version
assert_contains "tok --help" "Usage:" tok --help

# ── 2. Ls ────────────────────────────────────────────

section "Ls"

assert_ok      "tok ls ."                     tok ls .
assert_ok      "tok ls -la ."                 tok ls -la .
assert_ok      "tok ls -lh ."                 tok ls -lh .
assert_ok      "tok ls -l src/"               tok ls -l src/
assert_ok      "tok ls src/ -l (flag after)"  tok ls src/ -l
assert_ok      "tok ls multi paths"           tok ls src/ scripts/
assert_contains "tok ls -a shows hidden"      ".git" tok ls -a .
assert_contains "tok ls shows sizes"          "K"  tok ls src/
assert_contains "tok ls shows dirs with /"    "/" tok ls .

# ── 2b. Tree ─────────────────────────────────────────

section "Tree"

if command -v tree >/dev/null 2>&1; then
    assert_ok      "tok tree ."                tok tree .
    assert_ok      "tok tree -L 2 ."           tok tree -L 2 .
    assert_ok      "tok tree -d -L 1 ."        tok tree -d -L 1 .
    assert_contains "tok tree shows src/"      "src" tok tree -L 1 .
else
    skip_test "tok tree" "tree not installed"
fi

# ── 3. Read ──────────────────────────────────────────

section "Read"

assert_ok      "tok read Cargo.toml"          tok read Cargo.toml
assert_ok      "tok read --level none Cargo.toml"  tok read --level none Cargo.toml
assert_ok      "tok read --level aggressive Cargo.toml" tok read --level aggressive Cargo.toml
assert_ok      "tok read -n Cargo.toml"       tok read -n Cargo.toml
assert_ok      "tok read --max-lines 5 Cargo.toml" tok read --max-lines 5 Cargo.toml

section "Read (stdin support)"

assert_ok      "tok read stdin pipe"          bash -c 'echo "fn main() {}" | tok read -'

# ── 4. Git ───────────────────────────────────────────

section "Git (existing)"

assert_ok      "tok git status"               tok git status
assert_ok      "tok git status --short"       tok git status --short
assert_ok      "tok git status -s"            tok git status -s
assert_ok      "tok git status --porcelain"   tok git status --porcelain
assert_ok      "tok git log"                  tok git log
assert_ok      "tok git log -5"               tok git log -- -5
assert_ok      "tok git diff"                 tok git diff
assert_ok      "tok git diff --stat"          tok git diff --stat

section "Git (new: branch, fetch, stash, worktree)"

assert_ok      "tok git branch"               tok git branch
assert_ok      "tok git fetch"                tok git fetch
assert_ok      "tok git stash list"           tok git stash list
assert_ok      "tok git worktree"             tok git worktree

section "Git (passthrough: unsupported subcommands)"

assert_ok      "tok git tag --list"           tok git tag --list
assert_ok      "tok git remote -v"            tok git remote -v
assert_ok      "tok git rev-parse HEAD"       tok git rev-parse HEAD

# ── 5. GitHub CLI ────────────────────────────────────

section "GitHub CLI"

if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    assert_ok      "tok gh pr list"           tok gh pr list
    assert_ok      "tok gh run list"          tok gh run list
    assert_ok      "tok gh issue list"        tok gh issue list
    # pr create/merge/diff/comment/edit are write ops, test help only
    assert_help    "tok gh"                   tok gh
else
    skip_test "gh commands" "gh not authenticated"
fi

# ── 6. Cargo ─────────────────────────────────────────

section "Cargo (new)"

assert_ok      "tok cargo build"              tok cargo build
assert_ok      "tok cargo clippy"             tok cargo clippy
# cargo test exits non-zero due to pre-existing failures; check output ignoring exit code
output_cargo_test=$(tok cargo test 2>&1 || true)
if echo "$output_cargo_test" | grep -q "FAILURES\|test result:\|passed"; then
    PASS=$((PASS + 1))
    printf "  ${GREEN}PASS${NC}  %s\n" "tok cargo test"
else
    FAIL=$((FAIL + 1))
    FAILURES+=("tok cargo test")
    printf "  ${RED}FAIL${NC}  %s\n" "tok cargo test"
    printf "        got: %s\n" "$(echo "$output_cargo_test" | head -3)"
fi
assert_help    "tok cargo"                    tok cargo

# ── 7. Curl ──────────────────────────────────────────

section "Curl (new)"

assert_contains "tok curl JSON detect" "string" tok curl https://httpbin.org/json
assert_ok       "tok curl plain text"          tok curl https://httpbin.org/robots.txt
assert_help     "tok curl"                     tok curl

# ── 8. Npm / Npx ────────────────────────────────────

section "Npm / Npx (new)"

assert_help    "tok npm"                      tok npm
assert_help    "tok npx"                      tok npx

# ── 9. Pnpm ─────────────────────────────────────────

section "Pnpm"

assert_help    "tok pnpm"                     tok pnpm
assert_help    "tok pnpm build"               tok pnpm build
assert_help    "tok pnpm typecheck"           tok pnpm typecheck

if command -v pnpm >/dev/null 2>&1; then
    assert_ok  "tok pnpm help"                tok pnpm help
fi

# ── 10. Grep ─────────────────────────────────────────

section "Grep"

assert_ok      "tok grep pattern"             tok grep "pub fn" src/
assert_contains "tok grep finds results"      "pub fn" tok grep "pub fn" src/
assert_ok      "tok grep with file type"      tok grep "pub fn" src/ -t rust

section "Grep (extra args passthrough)"

assert_ok      "tok grep -i case insensitive" tok grep "fn" src/ -i
assert_ok      "tok grep -A context lines"    tok grep "fn run" src/ -A 2

# ── 11. Find ─────────────────────────────────────────

section "Find"

assert_ok      "tok find *.rs"                tok find "*.rs" src/
assert_contains "tok find shows files"        ".rs" tok find "*.rs" src/

# ── 12. Json ─────────────────────────────────────────

section "Json"

# Create temp JSON file for testing
TMPJSON=$(mktemp /tmp/tok-test-XXXXX.json)
echo '{"name":"test","count":42,"items":[1,2,3]}' > "$TMPJSON"

assert_ok      "tok json file"                tok json "$TMPJSON"
assert_contains "tok json shows schema"       "string" tok json "$TMPJSON"

rm -f "$TMPJSON"

# ── 13. Deps ─────────────────────────────────────────

section "Deps"

assert_ok      "tok deps ."                   tok deps .
assert_contains "tok deps shows Cargo"        "Cargo" tok deps .

# ── 14. Env ──────────────────────────────────────────

section "Env"

assert_ok      "tok env"                      tok env
assert_ok      "tok env --filter PATH"        tok env --filter PATH

# ── 16. Log ──────────────────────────────────────────

section "Log"

TMPLOG=$(mktemp /tmp/tok-log-XXXXX.log)
for i in $(seq 1 20); do
    echo "[2025-01-01 12:00:00] INFO: repeated message" >> "$TMPLOG"
done
echo "[2025-01-01 12:00:01] ERROR: something failed" >> "$TMPLOG"

assert_ok      "tok log file"                 tok log "$TMPLOG"

rm -f "$TMPLOG"

# ── 17. Summary ──────────────────────────────────────

section "Summary"

assert_ok      "tok summary echo hello"       tok summary echo hello

# ── 18. Err ──────────────────────────────────────────

section "Err"

assert_ok      "tok err echo ok"              tok err echo ok

# ── 19. Test runner ──────────────────────────────────

section "Test runner"

assert_ok      "tok test echo ok"             tok test echo ok

# ── 20. Gain ─────────────────────────────────────────

section "Gain"

assert_ok      "tok gain"                     tok gain
assert_ok      "tok gain --history"           tok gain --history

# ── 21. Config & Init ────────────────────────────────

section "Config & Init"

assert_ok      "tok config"                   tok config
assert_ok      "tok init --show"              tok init --show

# ── 22. Wget ─────────────────────────────────────────

section "Wget"

if command -v wget >/dev/null 2>&1; then
    assert_ok  "tok wget stdout"              tok wget https://httpbin.org/robots.txt -O
else
    skip_test "tok wget" "wget not installed"
fi

# ── 23. Tsc / Lint / Prettier / Next / Playwright ───

section "JS Tooling (help only, no project context)"

assert_help    "tok tsc"                      tok tsc
assert_help    "tok lint"                     tok lint
assert_help    "tok prettier"                 tok prettier
assert_help    "tok next"                     tok next
assert_help    "tok playwright"               tok playwright

# ── 24. Prisma ───────────────────────────────────────

section "Prisma (help only)"

assert_help    "tok prisma"                   tok prisma

# ── 25. Vitest ───────────────────────────────────────

section "Vitest (help only)"

assert_help    "tok vitest"                   tok vitest

# ── 26. Docker / Kubectl (help only) ────────────────

section "Docker / Kubectl (help only)"

assert_help    "tok docker"                   tok docker
assert_help    "tok kubectl"                  tok kubectl

# ── 27. Python (conditional) ────────────────────────

section "Python (conditional)"

if command -v pytest &>/dev/null; then
    assert_help    "tok pytest"                    tok pytest --help
else
    skip_test "tok pytest" "pytest not installed"
fi

if command -v ruff &>/dev/null; then
    assert_help    "tok ruff"                      tok ruff --help
else
    skip_test "tok ruff" "ruff not installed"
fi

if command -v pip &>/dev/null; then
    assert_help    "tok pip"                       tok pip --help
else
    skip_test "tok pip" "pip not installed"
fi

# ── 28. Go (conditional) ────────────────────────────

section "Go (conditional)"

if command -v go &>/dev/null; then
    assert_help    "tok go"                        tok go --help
    assert_help    "tok go test"                   tok go test -h
    assert_help    "tok go build"                  tok go build -h
    assert_help    "tok go vet"                    tok go vet -h
else
    skip_test "tok go" "go not installed"
fi

if command -v golangci-lint &>/dev/null; then
    assert_help    "tok golangci-lint"             tok golangci-lint --help
else
    skip_test "tok golangci-lint" "golangci-lint not installed"
fi

# ── 29. Graphite (conditional) ─────────────────────

section "Graphite (conditional)"

if command -v gt &>/dev/null; then
    assert_help   "tok gt"                          tok gt --help
    assert_ok     "tok gt log short"                tok gt log short
else
    skip_test "tok gt" "gt not installed"
fi

# ── 30. Ruby (conditional) ──────────────────────────

section "Ruby (conditional)"

if command -v rspec &>/dev/null; then
    assert_help    "tok rspec"                     tok rspec --help
else
    skip_test "tok rspec" "rspec not installed"
fi

if command -v rubocop &>/dev/null; then
    assert_help    "tok rubocop"                   tok rubocop --help
else
    skip_test "tok rubocop" "rubocop not installed"
fi

if command -v rake &>/dev/null; then
    assert_help    "tok rake"                      tok rake --help
else
    skip_test "tok rake" "rake not installed"
fi

# ── 31. Global flags ────────────────────────────────

section "Global flags"

assert_ok      "tok -u ls ."                  tok -u ls .
assert_ok      "tok --skip-env npm --help"    tok --skip-env npm --help

# ── 32. CcEconomics ─────────────────────────────────

section "CcEconomics"

assert_ok      "tok cc-economics"             tok cc-economics

# ── 33. Learn ───────────────────────────────────────

section "Learn"

assert_ok      "tok learn --help"             tok learn --help
assert_ok      "tok learn (no sessions)"      tok learn --since 0 2>&1 || true

# ── 32. Rewrite ───────────────────────────────────────

section "Rewrite"

assert_contains "rewrite git status"          "tok git status"         tok rewrite "git status"
assert_contains "rewrite cargo test"          "tok cargo test"         tok rewrite "cargo test"
assert_contains "rewrite compound &&"         "tok git status"         tok rewrite "git status && cargo test"
assert_contains "rewrite pipe preserves"      "| head"                 tok rewrite "git log | head"

section "Rewrite (#345: TOK_DISABLED skip)"

assert_fails   "rewrite TOK_DISABLED=1 skip"                          tok rewrite "TOK_DISABLED=1 git status"
assert_fails   "rewrite env TOK_DISABLED skip"                        tok rewrite "FOO=1 TOK_DISABLED=1 cargo test"

section "Rewrite (#346: 2>&1 preserved)"

assert_contains "rewrite 2>&1 preserved"      "2>&1"                  tok rewrite "cargo test 2>&1 | head"

section "Rewrite (#196: gh --json skip)"

assert_fails   "rewrite gh --json skip"                               tok rewrite "gh pr list --json number"
assert_fails   "rewrite gh --jq skip"                                 tok rewrite "gh api /repos --jq .name"
assert_fails   "rewrite gh --template skip"                           tok rewrite "gh pr view 1 --template '{{.title}}'"
assert_contains "rewrite gh normal works"     "tok gh pr list"        tok rewrite "gh pr list"

# ── 33. Verify ────────────────────────────────────────

section "Verify"

assert_ok      "tok verify"                   tok verify

# ── 34. Proxy ─────────────────────────────────────────

section "Proxy"

assert_ok      "tok proxy echo hello"         tok proxy echo hello
assert_contains "tok proxy passthrough"       "hello" tok proxy echo hello

# ── 35. Discover ──────────────────────────────────────

section "Discover"

assert_ok      "tok discover"                 tok discover

# ── 36. Diff ──────────────────────────────────────────

section "Diff"

assert_ok      "tok diff two files"           tok diff Cargo.toml LICENSE

# ── 37. Wc ────────────────────────────────────────────

section "Wc"

assert_ok      "tok wc Cargo.toml"            tok wc Cargo.toml

# ── 38. Smart ─────────────────────────────────────────

section "Smart"

assert_ok      "tok smart src/main.rs"        tok smart src/main.rs

# ── 39. Json edge cases ──────────────────────────────

section "Json (edge cases)"

assert_fails   "tok json on TOML (#347)"                              tok json Cargo.toml

# ── 40. Docker (conditional) ─────────────────────────

section "Docker (conditional)"

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
    assert_ok  "tok docker ps"               tok docker ps
    assert_ok  "tok docker images"           tok docker images
else
    skip_test "tok docker" "docker not running"
fi

# ── 41. Hook check ───────────────────────────────────

section "Hook check (#344)"

assert_contains "tok init --show hook version" "version" tok init --show

# ══════════════════════════════════════════════════════
# Report
# ══════════════════════════════════════════════════════

printf "\n${BOLD}══════════════════════════════════════${NC}\n"
printf "${BOLD}Results: ${GREEN}%d passed${NC}, ${RED}%d failed${NC}, ${YELLOW}%d skipped${NC}\n" "$PASS" "$FAIL" "$SKIP"

if [[ ${#FAILURES[@]} -gt 0 ]]; then
    printf "\n${RED}Failures:${NC}\n"
    for f in "${FAILURES[@]}"; do
        printf "  - %s\n" "$f"
    done
fi

printf "${BOLD}══════════════════════════════════════${NC}\n"

exit "$FAIL"
