#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

MIRI_TOOLCHAIN="$(< .github/miri-toolchain)"
MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_TIMEOUT_SECONDS:-180}"
readonly MIRI_TIMEOUT_GRACE_SECONDS=10
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

miri_totals="$(mktemp "${TMPDIR:-/tmp}/hibana-miri-totals.XXXXXX")"
trap 'rm -f "$miri_totals"' EXIT
run_miri_test() {
  local label="$1"
  shift
  local output
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-miri.XXXXXX")"
  if timeout --kill-after="${MIRI_TIMEOUT_GRACE_SECONDS}s" "${MIRI_TIMEOUT_SECONDS}s" \
    cargo +"${MIRI_TOOLCHAIN}" miri test "$@" >"${output}" 2>&1; then
    cat "${output}"
  else
    local status=$?
    cat "${output}" >&2
    rm -f "${output}"
    if [[ "${status}" == 124 ]]; then
      echo "miri gate timed out after ${MIRI_TIMEOUT_SECONDS}s: ${label}" >&2
    else
      echo "miri gate failed (exit ${status}): ${label}" >&2
    fi
    return "${status}"
  fi
  if ! TOOLCHAIN="${MIRI_TOOLCHAIN}" bash "${ROOT_DIR}/.github/scripts/lib/test_result_tool.sh" miri "${output}" "${label}" "${miri_totals}" 0; then
    rm -f "${output}"
    return 1
  fi
  rm -f "${output}"
}

run_miri_test \
  public-runtime-owner \
  -p hibana \
  --test miri_runtime_owner

run_miri_test \
  borrowed-transport-handles \
  -p hibana \
  --test transport_borrow

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  endpoint-owner-move \
  -p hibana \
  --lib \
  endpoint::owner::tests

run_miri_test \
  initial-endpoint-binding \
  -p hibana \
  --lib \
  transport::binding::tests

run_miri_test \
  prepared-send-completion \
  -p hibana \
  --lib \
  endpoint::kernel::core::commit_delta::send_completion::a_prepared_send_publishes_before_returning_success \
  -- --exact

run_miri_test \
  carrier-actor-borrow \
  -p hibana \
  --lib \
  endpoint::carrier::tests

run_miri_test \
  owned-future-prerequisite \
  -p hibana --lib \
  g::role_projection::tests::send_wait::an_owned_send_rejects_a_predecessor_that_needs_its_payload_view \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  complete-actor-visit-progress \
  -p hibana \
  --lib \
  g::role_projection::tests::visit_progress::a_complete_actor_visit_exposes_real_parallel_admissions_and_completion \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actual-completion-mutations \
  -p hibana --lib \
  g::role_projection::tests::mutation_coverage::actual_reset_and_recommit_cannot_disappear_between_equal_snapshots \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  chronological-resource-receipt \
  -p hibana --lib \
  g::role_projection::tests::receipt_refresh::a_reentered_earlier_body_refreshes_the_value_after_a_later_source_receipt \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  measured-source-handoff-interval \
  -p hibana --lib \
  g::role_projection::tests::handoff_interval::a_last_receipt_keeps_past_and_future_handoffs_outside_pending_parallel_work \
  -- --exact

run_miri_test \
  actor-shared-workspace \
  -p hibana \
  --lib \
  endpoint::kernel::core::public_types::tests

run_miri_test \
  typed-endpoint-ownership-source \
  -p hibana \
  --lib \
  g::source::tests

run_miri_test \
  owned-actor-image \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::actors::tests

# Shared views include both conditional holders and independent rolled routes.
MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-owned-progress \
  -p hibana \
  --lib \
  g::role_projection::tests::owned::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-local-progress \
  -p hibana \
  --lib \
  g::role_projection::tests \
  -- --skip g::role_projection::tests::owned:: \
  --skip g::role_projection::tests::mutation_coverage:: \
  --skip g::role_projection::tests::handoff_interval:: \
  --skip g::role_projection::tests::receipt_refresh:: \
  --skip g::role_projection::tests::send_wait::an_owned_send_rejects_a_predecessor_that_needs_its_payload_view \
  --skip g::role_projection::tests::admission:: \
  --skip g::role_projection::tests::parallel_pending:: \
  --skip g::role_projection::tests::pending_reentry:: \
  --skip g::role_projection::tests::order:: \
  --skip g::role_projection::tests::routing:: \
  --skip g::role_projection::tests::return_reentry:: \
  --skip g::role_projection::tests::local_return:: \
  --skip g::role_projection::tests::recovery_image:: \
  --skip g::role_projection::tests::receipt_alternatives:: \
  --skip g::role_projection::tests::entry_image:: \
  --skip g::role_projection::tests::reentry:: \
  --skip g::role_projection::tests::composed:: \
  --skip g::role_projection::tests::shared_holder_can_finish_either_parallel_loan_first_and_join_requires_both \
  --skip g::role_projection::tests::reentry_choice::an_enclosing_choice_survives_two_delegated_roll_visits \
  --skip g::role_projection::tests::reentry_parallel::parallel_loan_reentry_preserves_the_other_body_and_completed_tail \
  --skip g::role_projection::tests::conditional_controller::a_completed_right_arm_reopens_the_public_route_without_receiving \
  --skip g::role_projection::tests::conditional_controller::remote_controller:: \
  --skip g::role_projection::tests::conditional_controller::co_resident_controller::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-source-admission \
  -p hibana --lib \
  g::role_projection::tests::admission::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-parallel-owned-pending \
  -p hibana --lib \
  g::role_projection::tests::parallel_pending::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-explicit-inner-roll \
  -p hibana --lib \
  g::role_projection::tests::reentry::an_explicit_inner_roll_repeats_without_restarting_its_parent \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-inner-route-boundary \
  -p hibana --lib \
  g::role_projection::tests::reentry::an_inner_route_cannot_repeat_before_its_rolled_body_finishes \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-loan-after-continuation \
  -p hibana --lib \
  g::role_projection::tests::reentry::a_loan_reenters_after_its_sequential_continuation_without_reusing_old_work \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-composed-service \
  -p hibana --lib \
  g::role_projection::tests::composed::service_grants_a_resource_then_observes_choices_in_parallel_rolled_loans \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-independent-loan-join \
  -p hibana --lib \
  g::role_projection::tests::shared_holder_can_finish_either_parallel_loan_first_and_join_requires_both \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-resource-return-reentry \
  -p hibana --lib \
  g::role_projection::tests::return_reentry::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-local-resource-return-reentry \
  -p hibana --lib \
  g::role_projection::tests::local_return::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-resource-receipt-alternatives \
  -p hibana --lib \
  g::role_projection::tests::receipt_alternatives::

run_miri_test \
  actor-resource-receipt-image \
  -p hibana --lib \
  g::role_projection::tests::recovery_image::

run_miri_test \
  actor-resource-entry-image \
  -p hibana --lib \
  g::role_projection::tests::entry_image::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-pending-send-admission \
  -p hibana --lib \
  g::role_projection::tests::pending_reentry::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-parallel-order \
  -p hibana --lib \
  g::role_projection::tests::order::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-nested-routing \
  -p hibana --lib \
  g::role_projection::tests::routing::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  public-route-reentry \
  -p hibana --lib \
  g::role_projection::tests::conditional_controller::a_completed_right_arm_reopens_the_public_route_without_receiving \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-remote-route-reentry-commit \
  -p hibana --lib \
  g::role_projection::tests::conditional_controller::remote_controller::a_conditional_controller_does_not_take_authority_from_its_co_resident_observer \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-co-resident-route-reentry-commit \
  -p hibana --lib \
  g::role_projection::tests::conditional_controller::co_resident_controller::a_conditional_controller_does_not_take_authority_from_its_co_resident_observer \
  -- --exact

# Keep both observation-heavy reentries under their own existing deadline.
MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-body-reentry-commit \
  -p hibana --lib \
  g::role_projection::tests::reentry_choice::an_enclosing_choice_survives_two_delegated_roll_visits \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  actor-parallel-reentry-commit \
  -p hibana --lib \
  g::role_projection::tests::reentry_parallel::parallel_loan_reentry_preserves_the_other_body_and_completed_tail \
  -- --exact

# Keep every nested rework, delayed ingress and shared parallel prefix case.
# Separate the two larger cases without changing their per-run deadline.
MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  nested-resolver-reentry \
  -p hibana \
  --test nested_resolver_reentry \
  -- --skip sequential_parallel_prefix::next_resolver_preserves_a_parallel_rolled_prefix \
  --skip rolled_service_requests_resume_after_review_rework_while_another_flow_is_idle

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  rolled-service-review-rework \
  -p hibana \
  --test nested_resolver_reentry \
  rolled_service_requests_resume_after_review_rework_while_another_flow_is_idle \
  -- --exact

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  sequential-parallel-resolver-prefix \
  -p hibana \
  --test nested_resolver_reentry \
  sequential_parallel_prefix::next_resolver_preserves_a_parallel_rolled_prefix \
  -- --exact

run_miri_test \
  transport-requeue-owner \
  -p hibana \
  --lib \
  transport_requeue_callback_reentry_revalidates_generation

run_miri_test \
  receive-frame-receipt-owner \
  -p hibana \
  --lib \
  rendezvous::recv_frame_receipt::tests

run_miri_test \
  public-operation-kernel \
  -p hibana \
  --lib \
  endpoint::kernel::core::public_operation::tests

run_miri_test \
  tap-ring-owner \
  -p hibana \
  --lib \
  observe::core::tests

run_miri_test \
  descriptor-derived-route-history-owner \
  -p hibana \
  --lib \
  endpoint::kernel::decision_state::tests::route_arm_history_accepts_257_descriptor_relations

run_miri_test \
  descriptor-domain-offer-frontier-owner \
  -p hibana \
  --lib \
  endpoint::kernel::frontier::entry_sets::tests::observed_entry_set_streams_the_full_lane_domain

run_miri_test \
  exact-frontier-cursor-position-owner \
  -p hibana \
  --lib \
  endpoint::kernel::frontier::visit_set::tests

run_miri_test \
  constant-state-frontier-selection-owner \
  -p hibana \
  --lib \
  endpoint::kernel::frontier::progress_selection::tests

run_miri_test \
  root-frontier-packed-pool-owner \
  -p hibana \
  --lib \
  endpoint::kernel::frontier_state::tests

run_miri_test \
  frontier-scratch-workspace-owner \
  -p hibana \
  --lib \
  endpoint::kernel::frontier::scratch::tests

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_PROOF_EXPORT_TIMEOUT_SECONDS:-360}" \
MIRIFLAGS="${MIRIFLAGS} -Zmiri-disable-isolation" run_miri_test \
  production-proof-artifact-exporter \
  -p hibana \
  --lib \
  global::event_program_cursor_tests::lean_proof_export::export_production_trace_for_lean \
  -- \
  --ignored

run_miri_test \
  rolled-output-pipeline-owner \
  -p hibana \
  --lib \
  global::event_program_cursor_tests::production_cursor_pipelines_rolled_send_before_remote_receive

run_miri_test \
  immutable-event-admission \
  -p hibana \
  --lib \
  global::event_program_cursor_tests::event_admission_

run_miri_test \
  endpoint-waiter-owner \
  -p hibana \
  --lib \
  rendezvous::core::endpoint_waiter::tests

run_miri_test \
  affine-send-owner \
  -p hibana \
  --test affine_progression

run_miri_test \
  direct-recv-owner \
  -p hibana \
  --test cursor_send_recv_direct_recv

run_miri_test \
  forgotten-recv-owner \
  -p hibana \
  --test cursor_send_recv_session_forget_recv

run_miri_test \
  forgotten-send-owner \
  -p hibana \
  --test cursor_send_recv_session_forget_send

run_miri_test \
  endpoint-drop-wake-owner \
  -p hibana \
  --test cursor_send_recv_session_drop_wake

run_miri_test \
  session-fault-cancel-owner \
  -p hibana \
  --test cursor_send_recv_session_fault_cancel

run_miri_test \
  local-action-owner \
  -p hibana \
  --test local_action

run_miri_test \
  transport-contract-owner \
  -p hibana \
  --lib \
  transport::tests::transport_contract_

run_miri_test \
  rolled-causal-exit-owner \
  -p hibana \
  --test cursor_send_recv_send_recv \
  rolled_same_label_recv_requires_causal_exit_handoff

run_miri_test \
  intrinsic-route-in-band-owner \
  -p hibana \
  --test cursor_send_recv_send_recv \
  intrinsic_route_passive_same_label_recv_commits_by_frame_evidence

run_miri_test \
  rolled-resolved-route-reentry-owner \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_resolved_route_reenters_left_right_left_rows

run_miri_test \
  rolled-buffered-route-order-owner \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_resolved_route_preserves_buffered_decision_order

run_miri_test \
  rolled-same-label-schema-reentry-owner \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_resolved_same_label_reenters_with_selected_schema

run_miri_test \
  rolled-nested-resolver-reentry-owner \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_nested_resolved_route_reenters_asymmetric_paths

run_miri_test \
  rolled-current-frontier-reuse-owner \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_nested_resolved_route_reenters_passive_offer_asymmetric_paths

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  rolled-elastic-route-path-color-owner \
  -p hibana \
  --test rolled_resolver_reentry \
  rolled_deep_right_spine_passive_offer_reenters_across_all_depths

run_miri_test \
  session-family-isolation \
  -p hibana \
  --test cursor_send_recv_send_recv \
  session_template_instances_interleave_and_fault_independently

run_miri_test \
  session-waiter-isolation \
  -p hibana \
  --test cursor_send_recv_send_recv \
  same_lane_session_waiters_are_isolated

run_miri_test \
  route-branch-send-owner \
  -p hibana \
  --test route_branch_send

run_miri_test \
  resolved-send-owner \
  -p hibana \
  --test send_route_authority

run_miri_test \
  resolver-identity-owner \
  -p hibana \
  --test dynamic_route_scope_resolver \
  same_scope_sites_with_distinct_resolver_ids_keep_distinct_authority

run_miri_test \
  resolver-full-id-domain-owner \
  -p hibana \
  --test dynamic_route_scope_resolver \
  maximum_resolver_id_runs_end_to_end

run_miri_test \
  resolver-reject-cancellation-owner \
  -p hibana \
  --test dynamic_route_scope_resolver \
  resolver_reject_does_not_encode_or_stage_send_payload

run_miri_test \
  dynamic-membership-seal-owner \
  -p hibana \
  --test dynamic_route_scope_resolver \
  dynamic_resolution_seals_runtime_local_membership_before_evaluation

run_miri_test \
  offer-branch-owner \
  -p hibana \
  --test offer_branch_recv_evidence

run_miri_test \
  resident-sidecar-owner \
  -p hibana \
  --lib \
  storage_layout::capacity::tests

run_miri_test \
  resident-descriptor-validation \
  -p hibana \
  --lib \
  global::role_program::image_impl::tests::resident_

run_miri_test \
  descriptor-lane-byte-view-owner \
  -p hibana \
  --lib \
  global::role_program::tests::descriptor_lane_byte_view_remains_byte_aligned_and_covers_lane_255

run_miri_test \
  compiled-program-descriptor-validation \
  -p hibana \
  --lib \
  global::compiled::images::image::route_resolvers::tests::compiled_program_descriptor_rejects_

run_miri_test \
  compiled-program-atom-validation \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::compiled_program_atom_descriptor_

run_miri_test \
  compiled-program-atom-decoder-boundaries \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::compiled_program_atom_decoder_rejects_exact_invalid_boundaries

run_miri_test \
  compiled-program-owned-payload-kind \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::program_image_owned_kind_cannot_alias_a_wire_schema_or_truncate_a_role

run_miri_test \
  compiled-program-atom-lookup \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::compiled_program_atom_lookup_is_exact_for_dense_rows

run_miri_test \
  compiled-program-image-identity \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::compiled_program_image_identity_is_exact_over_facts_columns_and_blob

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  rolled-route-visit-ownership \
  -p hibana \
  --test security_report_regressions \
  rolled_routes::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  elastic-roll-wire-ownership \
  -p hibana \
  --test security_report_regressions \
  reentry_colors::

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  nested-send-continuation-ownership \
  -p hibana \
  --test security_report_regressions \
  send_continuations::

run_miri_test \
  nested-controller-offer-ownership \
  -p hibana \
  --test nested_resolver_self_continuations

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  nested-input-visit-ownership \
  -p hibana \
  --test visible_route_reentry \
  nested_input_roll_

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  shared-query-tail-reentry \
  -p hibana \
  --test shared_query_tail_reentry \
  switching_query_source_after_parallel_release_keeps_the_shared_reply_live

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  completed-body-suffix-rejection \
  -p hibana \
  --test shared_query_tail_reentry \
  a_completed_reply_cannot_be_reused_without_receiving_a_fresh_query

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  inactive-parallel-continuation-reentry \
  -p hibana \
  --test shared_query_tail_reentry \
  inactive_parallel_dependency_does_not_block_outer_roll_reentry

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  parallel-local-resolver-progress \
  -p hibana \
  --test shared_query_tail_reentry \
  pending_local_resolvers_survive_other_parallel_requests

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  send-selected-receive-continuation \
  -p hibana \
  --test shared_query_tail_reentry \
  completed_send_choice_keeps_its_receive_continuation

MIRI_TIMEOUT_SECONDS="${HIBANA_MIRI_DEEP_ROUTE_TIMEOUT_SECONDS:-480}" run_miri_test \
  ordinary-parallel-input-reentry \
  -p hibana \
  --test shared_query_tail_reentry \
  ordinary_parallel::

run_miri_test \
  program-image-storage-validation \
  -p hibana \
  --lib \
  global::compiled::images::image::program_ref::tests::program_image_

run_miri_test \
  endpoint-delegation-projection \
  --manifest-path proofs/endpoint-delegation/compiler/Cargo.toml \
  --locked --lib

TOOLCHAIN="${MIRI_TOOLCHAIN}" bash "${ROOT_DIR}/.github/scripts/lib/test_result_tool.sh" miri-total "${MIRI_TOOLCHAIN}" "${miri_totals}"
