#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

check_absent "ResolverSnapshotProvider" "ResolverSnapshotProvider" src README.md
check_absent "EpfInputProvider" "EpfInputProvider" src README.md
check_absent "ContextProvider" "ContextProvider" src README.md
check_absent "shared_context_query" "shared_context_query" src README.md
check_absent "with_epf_route" "with_epf_route" src README.md
check_absent "route_keys::|RESOLVER_INPUT0" "route_keys/RESOLVER_INPUT0" src README.md
if [[ -e src/resolver_audit.rs ]]; then
  echo "boundary deny pattern detected: resolver audit replay owner returned" >&2
  FAILED=1
fi
check_absent "ResolverCtx|HostSlots|pub\\(crate\\) enum Action|AbortInfo|run_resolver\\(|resolver_mode_tag\\(" "in-core resolver owner forbidden path" \
  src/rendezvous/port.rs src/rendezvous/core.rs src/endpoint/kernel/core.rs
check_absent "emit_endpoint_resolver_audit|endpoint_resolver_args|ResolverSlot::Endpoint(Rx|Tx)|hash_tap_event|emit_resolver_audit_replay|EndpointRxAuditPlan" \
  "endpoint resolver replay audit residue" \
  src README.md
check_absent "RouteResolverDecision|route_resolver_decision_from_action|Defer""Source::Epf" "EPF route authority forbidden path" \
  src/endpoint/kernel/authority.rs src/endpoint/kernel/core.rs

check_absent "#!?\\[[^]]*allow[[:space:]]*\\([^]]*dead[_]code" \
  "forbidden dead_code allow detected" \
  src tests --optional examples

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "resolver surface hygiene check passed"
