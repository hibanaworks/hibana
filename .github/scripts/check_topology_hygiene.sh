#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n -U "${pattern}" "$@"; then
    echo "topology hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

check_absent \
  "\\bsplice\\b" \
  "legacy splice topology vocabulary in core source or public README" \
  src README.md

check_absent \
  "validate_splice_generation|cached_splice|splice_table|splice_graph|splice operands|splice control" \
  "legacy splice topology vocabulary" \
  src tests \
  -g '!tests/ui.rs' \
  -g '!tests/docs_surface.rs' \
  -g '!tests/ui/core_splice_kind_reintroduction.rs' \
  -g '!tests/ui/core_splice_kind_reintroduction.stderr'

check_absent \
  "topology_operands_from_route_input|prepare_topology_operands_from_policy|validate_topology_operands_from_policy" \
  "topology operands decoded from policy route input" \
  src tests

check_absent \
  "topology_flags|FENCES_PRESENT" \
  "topology handle flags reintroduced instead of reserved-zero descriptors" \
  src tests

check_absent \
  "POLICY_MODE_ENFORCE_TAG|PolicyVerdict::Proceed" \
  "core audit conflates no-engine with enforce/proceed" \
  src tests

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "topology hygiene check passed"
