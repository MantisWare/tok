#!/usr/bin/env bash
#
# TOK Smoke Tests — Aristote Project (Vite + React + TS + ESLint)
# Tests TOK commands in a real JS/TS project context.
# Usage: bash scripts/test-aristote.sh
#
set -euo pipefail

ARISTOTE="/Users/florianbruniaux/Sites/MethodeAristote/aristote-school-boost"

PASS=0
FAIL=0
SKIP=0
FAILURES=()

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

assert_ok() {
    local name="$1"; shift
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
    local name="$1"; local needle="$2"; shift 2
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

# Allow non-zero exit but check output
assert_output() {
    local name="$1"; local needle="$2"; shift 2
    local output
    output=$("$@" 2>&1) || true
    if echo "$output" | grep -q "$needle"; then
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

skip_test() {
    local name="$1"; local reason="$2"
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

if [[ ! -d "$ARISTOTE" ]]; then
    echo "Aristote project not found at $ARISTOTE"
    exit 1
fi

printf "${BOLD}TOK Smoke Tests — Aristote Project${NC}\n"
printf "Binary: %s (%s)\n" "$TOK" "$(tok --version)"
printf "Project: %s\n" "$ARISTOTE"
printf "Date: %s\n\n" "$(date '+%Y-%m-%d %H:%M')"

# ── 1. File exploration ──────────────────────────────

section "Ls & Find"

assert_ok       "tok ls project root"           tok ls "$ARISTOTE"
assert_ok       "tok ls src/"                   tok ls "$ARISTOTE/src"
assert_ok       "tok ls --depth 3"              tok ls --depth 3 "$ARISTOTE/src"
assert_contains "tok ls shows components/"      "components" tok ls "$ARISTOTE/src"
assert_ok       "tok find *.tsx"                tok find "*.tsx" "$ARISTOTE/src"
assert_ok       "tok find *.ts"                 tok find "*.ts" "$ARISTOTE/src"
assert_contains "tok find finds App.tsx"        "App.tsx" tok find "*.tsx" "$ARISTOTE/src"

# ── 2. Read ──────────────────────────────────────────

section "Read"

assert_ok       "tok read tsconfig.json"        tok read "$ARISTOTE/tsconfig.json"
assert_ok       "tok read package.json"         tok read "$ARISTOTE/package.json"
assert_ok       "tok read App.tsx"              tok read "$ARISTOTE/src/App.tsx"
assert_ok       "tok read --level aggressive"   tok read --level aggressive "$ARISTOTE/src/App.tsx"
assert_ok       "tok read --max-lines 10"       tok read --max-lines 10 "$ARISTOTE/src/App.tsx"

# ── 3. Grep ──────────────────────────────────────────

section "Grep"

assert_ok       "tok grep import"               tok grep "import" "$ARISTOTE/src"
assert_ok       "tok grep with type filter"     tok grep "useState" "$ARISTOTE/src" -t tsx
assert_contains "tok grep finds components"     "import" tok grep "import" "$ARISTOTE/src"

# ── 4. Git ───────────────────────────────────────────

section "Git (in Aristote repo)"

# tok git doesn't support -C, use git -C via subshell
assert_ok       "tok git status"                bash -c "cd $ARISTOTE && tok git status"
assert_ok       "tok git log"                   bash -c "cd $ARISTOTE && tok git log"
assert_ok       "tok git branch"                bash -c "cd $ARISTOTE && tok git branch"

# ── 5. Deps ──────────────────────────────────────────

section "Deps"

assert_ok       "tok deps"                      tok deps "$ARISTOTE"
assert_contains "tok deps shows package.json"   "package.json" tok deps "$ARISTOTE"

# ── 6. Json ──────────────────────────────────────────

section "Json"

assert_ok       "tok json tsconfig"             tok json "$ARISTOTE/tsconfig.json"
assert_ok       "tok json package.json"         tok json "$ARISTOTE/package.json"

# ── 7. Env ───────────────────────────────────────────

section "Env"

assert_ok       "tok env"                       tok env
assert_ok       "tok env --filter NODE"         tok env --filter NODE

# ── 8. Tsc ───────────────────────────────────────────

section "TypeScript (tsc)"

if command -v npx >/dev/null 2>&1 && [[ -d "$ARISTOTE/node_modules" ]]; then
    assert_output "tok tsc (in aristote)" "error\|✅\|TS" tok tsc --project "$ARISTOTE"
else
    skip_test "tok tsc" "node_modules not installed"
fi

# ── 9. ESLint ────────────────────────────────────────

section "ESLint (lint)"

if command -v npx >/dev/null 2>&1 && [[ -d "$ARISTOTE/node_modules" ]]; then
    assert_output "tok lint (in aristote)" "error\|warning\|✅\|violations\|clean" tok lint --project "$ARISTOTE"
else
    skip_test "tok lint" "node_modules not installed"
fi

# ── 10. Build (Vite) ─────────────────────────────────

section "Build (Vite via tok next)"

if [[ -d "$ARISTOTE/node_modules" ]]; then
    # Aristote uses Vite, not Next — but tok next wraps the build script
    # Test with a timeout since builds can be slow
    skip_test "tok next build" "Vite project, not Next.js — use npm run build directly"
else
    skip_test "tok next build" "node_modules not installed"
fi

# ── 11. Diff ─────────────────────────────────────────

section "Diff"

# Diff two config files that exist in the project
assert_ok       "tok diff tsconfigs"            tok diff "$ARISTOTE/tsconfig.json" "$ARISTOTE/tsconfig.app.json"

# ── 12. Summary & Err ────────────────────────────────

section "Summary & Err"

assert_ok       "tok summary ls"                tok summary ls "$ARISTOTE/src"
assert_ok       "tok err ls"                    tok err ls "$ARISTOTE/src"

# ── 13. Gain ─────────────────────────────────────────

section "Gain (after above commands)"

assert_ok       "tok gain"                      tok gain
assert_ok       "tok gain --history"            tok gain --history

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
