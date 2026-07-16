#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

MIRI_TOOLCHAIN="$(< .github/miri-toolchain)"
MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_TIMEOUT_SECONDS:-180}"
export MIRIFLAGS="-Zmiri-strict-provenance"

if [[ ! "${MIRI_TOOLCHAIN}" =~ ^nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
  echo "miri gate invalid pinned toolchain: ${MIRI_TOOLCHAIN}" >&2
  exit 1
fi

source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg

if ! rustup run "${MIRI_TOOLCHAIN}" cargo miri --version >/dev/null 2>&1; then
  echo "miri gate missing pinned toolchain/component: ${MIRI_TOOLCHAIN}" >&2
  echo "install with: rustup toolchain install ${MIRI_TOOLCHAIN} --profile minimal --component miri --component rust-src" >&2
  exit 1
fi
if ! rustup component list --toolchain "${MIRI_TOOLCHAIN}" \
  | grep -Eq '^rust-src.*\(installed\)$'; then
  echo "miri gate missing rust-src for pinned toolchain: ${MIRI_TOOLCHAIN}" >&2
  exit 1
fi
if ! command -v timeout >/dev/null 2>&1; then
  echo "miri gate requires GNU timeout" >&2
  exit 1
fi

miri_passed_total=0
miri_ignored_total=0

run_miri_test() {
  local label="$1"
  local expected_listed="$2"
  local expected_passed="$3"
  local expected_ignored="$4"
  shift 4
  local output
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-miri.XXXXXX")"
  if ! timeout "${MIRI_TIMEOUT_SECONDS}s" \
    cargo +"${MIRI_TOOLCHAIN}" miri test "$@" >"${output}" 2>&1; then
    cat "${output}" >&2
    rm -f "${output}"
    echo "miri gate failed: ${label}" >&2
    return 1
  fi
  cat "${output}"
  if ! grep -Fq "running ${expected_listed} test" "${output}" \
    || ! grep -Fq \
      "test result: ok. ${expected_passed} passed; 0 failed; ${expected_ignored} ignored;" \
      "${output}"; then
    rm -f "${output}"
    echo "miri gate test-count mismatch: ${label} listed=${expected_listed} passed=${expected_passed} ignored=${expected_ignored}" >&2
    return 1
  fi
  rm -f "${output}"
  miri_passed_total=$((miri_passed_total + expected_passed))
  miri_ignored_total=$((miri_ignored_total + expected_ignored))
}

run_miri_test \
  public-runtime-owner \
  28 \
  28 \
  0 \
  -p hibana \
  --test miri_runtime_owner

run_miri_test \
  transport-requeue-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --lib \
  transport_requeue_callback_reentry_revalidates_generation

run_miri_test \
  receive-frame-receipt-owner \
  6 \
  6 \
  0 \
  -p hibana \
  --lib \
  rendezvous::recv_frame_receipt::tests

run_miri_test \
  public-operation-kernel \
  2 \
  1 \
  1 \
  -p hibana \
  --lib \
  endpoint::kernel::core::public_types::tests

run_miri_test \
  tap-ring-owner \
  2 \
  2 \
  0 \
  -p hibana \
  --lib \
  observe::core::tests

run_miri_test \
  descriptor-derived-route-history-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --lib \
  endpoint::kernel::decision_state::tests::route_arm_history_crosses_the_former_256_boundary

run_miri_test \
  descriptor-domain-offer-frontier-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --lib \
  endpoint::kernel::frontier::entry_sets::tests::observed_entry_set_streams_beyond_the_former_eight_slot_mask

run_miri_test \
  exact-frontier-visit-identity-owner \
  4 \
  4 \
  0 \
  -p hibana \
  --lib \
  endpoint::kernel::frontier::snapshot::tests

run_miri_test \
  root-frontier-packed-pool-owner \
  7 \
  7 \
  0 \
  -p hibana \
  --lib \
  endpoint::kernel::frontier_state::tests

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_PROOF_EXPORT_TIMEOUT_SECONDS:-360}" \
MIRIFLAGS="${MIRIFLAGS} -Zmiri-disable-isolation" run_miri_test \
  production-proof-artifact-exporter \
  1 \
  1 \
  0 \
  -p hibana \
  --lib \
  global::event_program_cursor_tests::lean_proof_export::export_production_trace_for_lean \
  -- \
  --ignored

run_miri_test \
  rolled-output-pipeline-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --lib \
  global::event_program_cursor_tests::production_cursor_pipelines_rolled_send_before_remote_receive

run_miri_test \
  endpoint-waiter-owner \
  2 \
  2 \
  0 \
  -p hibana \
  --lib \
  rendezvous::core::endpoint_waiter::tests

run_miri_test \
  affine-send-owner \
  4 \
  4 \
  0 \
  -p hibana \
  --test affine_progression

run_miri_test \
  direct-recv-owner \
  11 \
  11 \
  0 \
  -p hibana \
  --test cursor_send_recv_direct_recv

run_miri_test \
  forgotten-recv-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test cursor_send_recv_session_forget_recv

run_miri_test \
  forgotten-send-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test cursor_send_recv_session_forget_send

run_miri_test \
  endpoint-drop-wake-owner \
  2 \
  2 \
  0 \
  -p hibana \
  --test cursor_send_recv_session_drop_wake

run_miri_test \
  session-fault-cancel-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test cursor_send_recv_session_fault_cancel

run_miri_test \
  local-action-owner \
  4 \
  4 \
  0 \
  -p hibana \
  --test local_action

run_miri_test \
  transport-contract-owner \
  6 \
  6 \
  0 \
  -p hibana \
  --lib \
  transport::tests::transport_contract_

run_miri_test \
  rolled-causal-exit-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test cursor_send_recv_send_recv \
  rolled_same_label_recv_requires_causal_exit_handoff

run_miri_test \
  intrinsic-route-in-band-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test cursor_send_recv_send_recv \
  intrinsic_route_passive_same_label_recv_commits_by_frame_evidence

run_miri_test \
  rolled-resolved-route-reentry-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_resolved_route_reenters_left_right_left_rows

run_miri_test \
  rolled-buffered-route-order-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_resolved_route_preserves_buffered_decision_order

run_miri_test \
  rolled-same-label-schema-reentry-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_resolved_same_label_reenters_with_selected_schema

run_miri_test \
  rolled-nested-resolver-reentry-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_nested_resolved_route_reenters_asymmetric_paths

run_miri_test \
  rolled-current-frontier-reuse-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_nested_resolved_route_reenters_passive_offer_asymmetric_paths

run_miri_test \
  rolled-elastic-route-path-color-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_deep_right_spine_passive_offer_reenters_across_all_depths

run_miri_test \
  session-family-isolation \
  1 \
  1 \
  0 \
  -p hibana \
  --test cursor_send_recv_send_recv \
  session_template_instances_interleave_and_fault_independently

run_miri_test \
  session-waiter-isolation \
  1 \
  1 \
  0 \
  -p hibana \
  --test cursor_send_recv_send_recv \
  same_lane_session_waiters_are_isolated

run_miri_test \
  route-branch-send-owner \
  6 \
  6 \
  0 \
  -p hibana \
  --test route_branch_send

run_miri_test \
  resolved-send-owner \
  2 \
  2 \
  0 \
  -p hibana \
  --test send_route_authority

run_miri_test \
  resolver-identity-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test dynamic_route_scope_resolver \
  same_scope_sites_with_distinct_resolver_ids_keep_distinct_authority

run_miri_test \
  resolver-reject-cancellation-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test dynamic_route_scope_resolver \
  resolver_reject_does_not_encode_or_stage_send_payload

run_miri_test \
  dynamic-membership-seal-owner \
  1 \
  1 \
  0 \
  -p hibana \
  --test dynamic_route_scope_resolver \
  dynamic_resolution_seals_runtime_local_membership_before_evaluation

run_miri_test \
  offer-branch-owner \
  11 \
  11 \
  0 \
  -p hibana \
  --test offer_branch_recv_evidence

run_miri_test \
  resident-sidecar-owner \
  17 \
  16 \
  1 \
  -p hibana \
  --lib \
  storage_layout::capacity::tests

run_miri_test \
  resident-descriptor-validation \
  40 \
  40 \
  0 \
  -p hibana \
  --lib \
  global::role_program::image_impl::tests::resident_

run_miri_test \
  compiled-program-descriptor-validation \
  13 \
  13 \
  0 \
  -p hibana \
  --lib \
  global::compiled::images::image::route_resolvers::tests::compiled_program_descriptor_rejects_

run_miri_test \
  compiled-program-atom-validation \
  7 \
  7 \
  0 \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::compiled_program_atom_descriptor_

run_miri_test \
  compiled-program-atom-lookup \
  1 \
  1 \
  0 \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::compiled_program_atom_lookup_is_exact_for_sparse_sorted_rows

run_miri_test \
  compiled-program-atom-order-rejection \
  1 \
  1 \
  0 \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::compiled_program_descriptor_rejects_noncanonical_atom_order

run_miri_test \
  program-image-storage-validation \
  2 \
  2 \
  0 \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::program_image_

echo "miri gate passed toolchain=${MIRI_TOOLCHAIN} tests=${miri_passed_total} ignored=${miri_ignored_total}"
