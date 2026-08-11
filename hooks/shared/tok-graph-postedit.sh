#!/usr/bin/env bash
# TOK code graph — post-edit refresh.
# Brings the graph back in step with a file that just changed, so the next
# query sees the edit.
#
# Silent by design: this runs inside the agent's edit, and anything printed
# here is either parsed as a hook directive or shown to the user as noise.
# A failure must never fail the edit that triggered it.
set -euo pipefail

if ! command -v tok &>/dev/null; then
  exit 0
fi

# --stdin drains the tool payload. A host writing to a pipe nobody reads can
# block on a full buffer.
tok hook graph-postedit --stdin >/dev/null 2>&1 || true
exit 0
