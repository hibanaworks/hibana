#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n "${pattern}" "$@"; then
    echo "forbidden token detected: ${label}" >&2
    FAILED=1
  fi
}

check_absent "PolicySnapshotProvider" "PolicySnapshotProvider" src README.md docs
check_absent "EpfInputProvider" "EpfInputProvider" src README.md docs
check_absent "ContextProvider" "ContextProvider" src README.md docs
check_absent "shared_context_query" "shared_context_query" src README.md docs
check_absent "with_epf_route" "with_epf_route" src README.md docs
check_absent "route_keys::|POLICY_INPUT0" "route_keys/POLICY_INPUT0" src README.md docs
check_absent "PolicyCtx|HostSlots|pub\\(crate\\) enum Action|AbortInfo|run_policy\\(|policy_mode_tag\\(" "in-core policy appliance shim" \
  src/policy_runtime.rs src/rendezvous/port.rs src/rendezvous/core.rs src/endpoint/kernel/core.rs
check_absent "RoutePolicyDecision|route_policy_decision_from_action|DeferSource::Epf" "EPF route authority shim" \
  src/endpoint/kernel/route_frontier/authority.rs src/endpoint/kernel/core.rs

if rg -n "#!?\\[allow\\(dead_code\\)\\]" src tests examples; then
  echo "forbidden #[allow(dead_code)] detected" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "policy surface hygiene check passed"
