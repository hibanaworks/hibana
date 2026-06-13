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

check_absent "ResolverSnapshotProvider" "ResolverSnapshotProvider" src README.md
check_absent "EpfInputProvider" "EpfInputProvider" src README.md
check_absent "ContextProvider" "ContextProvider" src README.md
check_absent "shared_context_query" "shared_context_query" src README.md
check_absent "with_epf_route" "with_epf_route" src README.md
check_absent "route_keys::|RESOLVER_INPUT0" "route_keys/RESOLVER_INPUT0" src README.md
check_absent "ResolverCtx|HostSlots|pub\\(crate\\) enum Action|AbortInfo|run_resolver\\(|resolver_mode_tag\\(" "in-core resolver appliance bypass path" \
  src/resolver_audit.rs src/rendezvous/port.rs src/rendezvous/core.rs src/endpoint/kernel/core.rs
check_absent "RouteResolverDecision|route_resolver_decision_from_action|Defer""Source::Epf" "EPF route authority bypass path" \
  src/endpoint/kernel/authority.rs src/endpoint/kernel/core.rs

allow_paths=(src tests)
if [[ -d examples ]]; then
  allow_paths+=(examples)
fi

if rg -n "#!?\\[allow\\(dead_code\\)\\]" "${allow_paths[@]}"; then
  echo "forbidden #[allow(dead_code)] detected" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "resolver surface hygiene check passed"
