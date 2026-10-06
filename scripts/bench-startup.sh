#!/usr/bin/env bash
# Startup-time gate for the code-graph work.
#
# TOK's contract is <10ms startup for ordinary filter commands. The code graph
# pulls in tree-sitter grammars, so this guards against a regression where
# parsers get constructed eagerly instead of on first extraction.
#
# Usage:
#   scripts/bench-startup.sh              # measure the release binary
#   scripts/bench-startup.sh --save NAME  # record a baseline under NAME
#   scripts/bench-startup.sh --compare A  # measure and diff against baseline A

set -euo pipefail

BIN="${TOK_BIN:-target/release/tok}"
BASELINE_DIR="target/bench-baselines"
THRESHOLD_MS="${TOK_STARTUP_THRESHOLD_MS:-10}"

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "hyperfine not found — install with 'brew install hyperfine' or 'cargo install hyperfine'" >&2
  exit 127
fi

if [ ! -x "$BIN" ]; then
  echo "binary not found at $BIN — run 'cargo build --release' first" >&2
  exit 1
fi

mode="measure"
label=""
case "${1:-}" in
  --save)    mode="save";    label="${2:?--save needs a name}" ;;
  --compare) mode="compare"; label="${2:?--compare needs a name}" ;;
  "")        ;;
  *)         echo "unknown option: $1" >&2; exit 2 ;;
esac

mkdir -p "$BASELINE_DIR"
out="$BASELINE_DIR/${label:-current}.json"

# `--version` is the cheapest path through main(): clap parse then exit. Any
# work that leaks into startup shows up here regardless of subcommand.
hyperfine \
  --warmup 5 \
  --min-runs 50 \
  --export-json "$out" \
  --command-name "tok --version" "$BIN --version" \
  --command-name "tok gain" "$BIN gain"

mean_ms=$(python3 -c "
import json,sys
data=json.load(open('$out'))
print(round(data['results'][0]['mean']*1000, 2))
")

echo
echo "startup mean: ${mean_ms}ms (threshold ${THRESHOLD_MS}ms)"

if [ "$mode" = "compare" ]; then
  base="$BASELINE_DIR/${label}.json"
  if [ ! -f "$base" ]; then
    echo "no baseline named '${label}' — record one with --save ${label}" >&2
    exit 1
  fi
  python3 -c "
import json
base=json.load(open('$base'))['results'][0]['mean']*1000
curr=json.load(open('$out'))['results'][0]['mean']*1000
delta=curr-base
pct=(delta/base*100) if base else 0
print(f'baseline {base:.2f}ms -> current {curr:.2f}ms ({delta:+.2f}ms, {pct:+.1f}%)')
"
fi

python3 -c "
import sys
mean=float('$mean_ms')
limit=float('$THRESHOLD_MS')
if mean > limit:
    print(f'FAIL: startup {mean:.2f}ms exceeds {limit:.2f}ms budget')
    sys.exit(1)
print('OK: within startup budget')
"
