#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

# Manager load/activate/revert primitives must not leak as public methods.
if rg -n "pub fn (load_begin|load_begin_raw|load_chunk|load_chunk_raw|load_commit|load_commit_raw|activate|schedule_activate|on_decision_boundary|revert|set_policy_mode|set_policy_mode_staged)\\b" src/runtime; then
  echo "mgmt boundary violation: manager mutators must not be public" >&2
  exit 1
fi

echo "mgmt boundary check passed"
