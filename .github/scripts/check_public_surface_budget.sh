#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_max_lines() {
  local path="$1"
  local max="$2"
  local count
  count="$(wc -l < "${path}" | tr -d ' ')"
  if (( count > max )); then
    echo "public surface budget violation: ${path} has ${count} lines, budget is ${max}" >&2
    FAILED=1
  fi
}

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n -U "${pattern}" "$@"; then
    echo "public surface budget violation: ${label}" >&2
    FAILED=1
  fi
}

check_max_lines ".github/allowlists/lib-public-api.txt" 3
check_max_lines ".github/allowlists/g-public-api.txt" 7
check_max_lines ".github/allowlists/endpoint-public-api.txt" 11
check_max_lines ".github/allowlists/substrate-public-api.txt" 43

check_absent \
  "g::advanced|FlowSendArg|SendOutcomeKind|CapFlow|FlowInner|DynamicResolution|from_fn|from_state|fallback|legacy|compat|heuristic|rescue|state machine|TransportSnapshotParts|ConfigParts|RegisteredTokenParts" \
  "forbidden final-form names in public API allowlists" \
  .github/allowlists

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "public surface budget check passed"
