#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF_DIR="${ROOT_DIR}/proofs/lean"
GENERATED="${ROOT_DIR}/target/lean-proof/Generated.lean"
RUNTIME_GENERATED="${ROOT_DIR}/target/lean-proof/RuntimeGenerated.lean"
PUBLIC_OPERATION_GENERATED="${ROOT_DIR}/target/lean-proof/PublicOperationGenerated.lean"
CAUSAL_GENERATED="${ROOT_DIR}/target/lean-proof/CausalGenerated.lean"
PARALLEL_GENERATED="${ROOT_DIR}/target/lean-proof/ParallelGenerated.lean"
PROGRAM_COLUMNS_GENERATED="${ROOT_DIR}/target/lean-proof/ProgramColumnsGenerated.lean"
PARTIAL_CHOICE_GENERATED="${ROOT_DIR}/target/lean-proof/PartialChoiceGenerated.lean"
CONTROLLER_GENERATED="${ROOT_DIR}/target/lean-proof/ControllerGenerated.lean"
INGRESS_GENERATED="${ROOT_DIR}/target/lean-proof/IngressGenerated.lean"
REENTRY_GENERATED="${ROOT_DIR}/target/lean-proof/ReentryGenerated.lean"
OWNED_RECOVERY_GENERATED="${ROOT_DIR}/target/lean-proof/OwnedRecoveryGenerated.lean"
LOCAL_RETURN_GENERATED="${ROOT_DIR}/target/lean-proof/LocalReturnGenerated.lean"
PENDING_BORROW_GENERATED="${ROOT_DIR}/target/lean-proof/PendingBorrowGenerated.lean"
FUTURE_CUSTODY_GENERATED="${ROOT_DIR}/target/lean-proof/FutureCustodyGenerated.lean"
OWNED_DROP_GENERATED="${ROOT_DIR}/target/lean-proof/OwnedDropGenerated.lean"
COMPLETED_RETIREMENT_GENERATED="${ROOT_DIR}/target/lean-proof/CompletedViewRetirementGenerated.lean"
SOURCE_ORDER_GENERATED="${ROOT_DIR}/target/lean-proof/SourceOrderGenerated.lean"
OWNED_ENTRY_GENERATED="${ROOT_DIR}/target/lean-proof/OwnedEntryGenerated.lean"
ACTOR_COMMIT_GENERATED="${ROOT_DIR}/target/lean-proof/ActorCommitGenerated.lean"
ACTOR_ADMISSION_GENERATED="${ROOT_DIR}/target/lean-proof/ActorAdmissionGenerated.lean"
ACTOR_VISIT_GENERATED="${ROOT_DIR}/target/lean-proof/ActorVisitProgressGenerated.lean"
ACTOR_MUTATION_GENERATED="${ROOT_DIR}/target/lean-proof/ActorMutationCoverageGenerated.lean"
SOURCE_INTERVAL_GENERATED="${ROOT_DIR}/target/lean-proof/SourceHandoffIntervalGenerated.lean"
FUTURE_PREDECESSOR_GENERATED="${ROOT_DIR}/target/lean-proof/OwnedFuturePredecessorGenerated.lean"
SEND_COMPLETION_GENERATED="${ROOT_DIR}/target/lean-proof/SendCompletionGenerated.lean"
ACTOR_REENTRY_COMMIT_GENERATED="${ROOT_DIR}/target/lean-proof/ActorReentryCommitGenerated.lean"
ACTOR_PARALLEL_REENTRY_GENERATED="${ROOT_DIR}/target/lean-proof/ActorParallelReentryGenerated.lean"
RECEIPT_REFRESH_GENERATED="${ROOT_DIR}/target/lean-proof/ReceiptRefreshGenerated.lean"
ACTOR_ROUTE_LEFT_REENTRY_GENERATED="${ROOT_DIR}/target/lean-proof/ActorRouteLeftReentryGenerated.lean"
ACTOR_ROUTE_RIGHT_REENTRY_GENERATED="${ROOT_DIR}/target/lean-proof/ActorRouteRightReentryGenerated.lean"
GENERATED_CLAIM_SNAPSHOT="${PROOF_DIR}/generated-claim-surface.txt"
RUNTIME_CLAIM_SNAPSHOT="${PROOF_DIR}/runtime-generated-claim-surface.txt"
PUBLIC_OPERATION_CLAIM_SNAPSHOT="${PROOF_DIR}/public-operation-generated-claim-surface.txt"
STATIC_CLAIM_SNAPSHOT="${PROOF_DIR}/all-claim-surface.txt"
EXAMPLE_CLAIM_SNAPSHOT="${PROOF_DIR}/example-claim-surface.txt"
TOOLCHAIN="${TOOLCHAIN:-stable}"

# Preserve the auditor's failure evidence even when the caller captures stdout.
run_audit() {
  local audit_output audit_status
  if audit_output="$(bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" "$@")"; then
    printf '%s\n' "${audit_output}"
  else
    audit_status=$?
    printf '%s\n' "${audit_output}" >&2
    return "${audit_status}"
  fi
}

bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard toolchain "${PROOF_DIR}"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" theorem-inventory --self-test
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" generated-axioms --self-test

bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" build-serial "${PROOF_DIR}"
bash "${ROOT_DIR}/.github/scripts/check_lean_claim_surface.sh"
static_audit_output="$(run_audit \
  theorem-inventory \
  --static "${PROOF_DIR}" "${STATIC_CLAIM_SNAPSHOT}" 988 583 293 112)"
printf '%s\n' "${static_audit_output}"
printf '%s\n' "${static_audit_output}" | bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard markers static
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" theorem-inventory \
  --example-types "${PROOF_DIR}" "${EXAMPLE_CLAIM_SNAPSHOT}" 36

source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
mkdir -p "$(dirname "${GENERATED}")"
rm -f "${GENERATED}" "${RUNTIME_GENERATED}" "${PUBLIC_OPERATION_GENERATED}" "${CAUSAL_GENERATED}" "${PARALLEL_GENERATED}" "${PROGRAM_COLUMNS_GENERATED}" "${PARTIAL_CHOICE_GENERATED}" "${CONTROLLER_GENERATED}" "${INGRESS_GENERATED}" "${REENTRY_GENERATED}" "${ACTOR_COMMIT_GENERATED}" "${ACTOR_REENTRY_COMMIT_GENERATED}" "${ACTOR_PARALLEL_REENTRY_GENERATED}" "${ACTOR_ROUTE_LEFT_REENTRY_GENERATED}" "${ACTOR_ROUTE_RIGHT_REENTRY_GENERATED}"
rm -f "${OWNED_RECOVERY_GENERATED}"
rm -f "${ACTOR_ADMISSION_GENERATED}"
rm -f "${ACTOR_VISIT_GENERATED}"
rm -f "${ACTOR_MUTATION_GENERATED}"
rm -f "${SOURCE_INTERVAL_GENERATED}"
rm -f "${FUTURE_PREDECESSOR_GENERATED}"
rm -f "${SEND_COMPLETION_GENERATED}"
rm -f "${RECEIPT_REFRESH_GENERATED}"
rm -f "${LOCAL_RETURN_GENERATED}"
rm -f "${PENDING_BORROW_GENERATED}"
rm -f "${FUTURE_CUSTODY_GENERATED}"
rm -f "${OWNED_DROP_GENERATED}" "${COMPLETED_RETIREMENT_GENERATED}"
rm -f "${SOURCE_ORDER_GENERATED}"
rm -f "${OWNED_ENTRY_GENERATED}"
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::admission::source_ready_rows_match_actual_admission_across_a_parallel_owned_visit \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_ADMISSION_GENERATED}" "source and actual cursor admission"
run_audit generated-axioms --kernel "${ACTOR_ADMISSION_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-admission-generated-claim-surface.txt" 54
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::mutation_coverage::actual_reset_and_recommit_cannot_disappear_between_equal_snapshots \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_MUTATION_GENERATED}" "actual completion mutation coverage"
run_audit generated-axioms --kernel "${ACTOR_MUTATION_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-mutation-generated-claim-surface.txt" 11
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::handoff_interval::a_last_receipt_keeps_past_and_future_handoffs_outside_pending_parallel_work \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${SOURCE_INTERVAL_GENERATED}" "measured source handoff interval"
run_audit generated-axioms --kernel "${SOURCE_INTERVAL_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/source-interval-generated-claim-surface.txt" 19
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::receipt_refresh::a_reentered_earlier_body_refreshes_the_value_after_a_later_source_receipt \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${RECEIPT_REFRESH_GENERATED}" "actual chronological resource receipt"
run_audit generated-axioms --kernel "${RECEIPT_REFRESH_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/receipt-refresh-generated-claim-surface.txt" 13
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::send_wait::an_owned_send_rejects_a_predecessor_that_needs_its_payload_view \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${FUTURE_PREDECESSOR_GENERATED}" "actual owned future prerequisite"
run_audit generated-axioms --kernel "${FUTURE_PREDECESSOR_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/owned-future-predecessor-generated-claim-surface.txt" 4
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::visit_progress::a_complete_actor_visit_exposes_real_parallel_admissions_and_completion \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_VISIT_GENERATED}" "complete actor visit progress"
run_audit generated-axioms --kernel "${ACTOR_VISIT_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-visit-generated-claim-surface.txt" 24
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  endpoint::kernel::core::commit_delta::send_completion::a_prepared_send_publishes_before_returning_success \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${SEND_COMPLETION_GENERATED}" "prepared send completion"
run_audit generated-axioms --kernel "${SEND_COMPLETION_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/send-completion-generated-claim-surface.txt" 7
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::source_order::source_order_rejects_a_missing_join_and_spurious_parallel_order \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${SOURCE_ORDER_GENERATED}" "source operation dependencies"
run_audit generated-axioms --kernel "${SOURCE_ORDER_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/source-order-generated-claim-surface.txt" 3
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::entry_image::missing_wrong_owner_and_unrelated_roll_entry_resources_are_rejected \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${OWNED_ENTRY_GENERATED}" "owned entry resources"
run_audit generated-axioms --kernel "${OWNED_ENTRY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/owned-entry-generated-claim-surface.txt" 5
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::recovery_image::missing_wrong_and_out_of_bounds_owned_receipts_are_rejected \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${OWNED_RECOVERY_GENERATED}" "owned resource receipts"
run_audit generated-axioms --kernel "${OWNED_RECOVERY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/owned-recovery-generated-claim-surface.txt" 5
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::local_return::a_local_branch_waits_for_another_view_to_receive_its_entry_resource \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${LOCAL_RETURN_GENERATED}" "local resource return"
run_audit generated-axioms --kernel "${LOCAL_RETURN_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/local-return-generated-claim-surface.txt" 12
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::pending_reentry::a_pending_owned_send_keeps_its_payload_unavailable_for_resource_dependent_reentry \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${PENDING_BORROW_GENERATED}" "pending payload borrow"
run_audit generated-axioms --kernel "${PENDING_BORROW_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/pending-borrow-generated-claim-surface.txt" 11
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::owned::dropping_a_completed_owned_send_does_not_retire_the_returned_resource_view \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${FUTURE_CUSTODY_GENERATED}" "future and frame custody"
run_audit generated-axioms --kernel "${FUTURE_CUSTODY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/future-custody-generated-claim-surface.txt" 66
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib -- \
  g::role_projection::tests::send_wait::dropping_an_owned_send_cancels_the_run_before_or_during_preparation \
  g::role_projection::tests::retirement::a_completed_view_retires_without_cancelling_independent_work --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${OWNED_DROP_GENERATED}" "owned future drop"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${COMPLETED_RETIREMENT_GENERATED}" "completed view retirement"
run_audit generated-axioms --kernel "${OWNED_DROP_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/owned-drop-generated-claim-surface.txt" 54
run_audit generated-axioms --kernel "${COMPLETED_RETIREMENT_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/completed-retirement-generated-claim-surface.txt" 18
CARGO_BUILD_JOBS=1 \
  RUST_TEST_THREADS=1 \
cargo +"${TOOLCHAIN}" test -p hibana --lib \
    global::event_program_cursor_tests::lean_proof_export::export_production_trace_for_lean \
    -- --ignored --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${GENERATED}" "production trace"
CARGO_BUILD_JOBS=1 \
  RUST_TEST_THREADS=1 \
  cargo +"${TOOLCHAIN}" test -p hibana --lib \
    rendezvous::core::storage_layout::capacity::tests::formal_certificate_export::export_runtime_certificates_for_lean \
    -- --ignored --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${RUNTIME_GENERATED}" "runtime"
CARGO_BUILD_JOBS=1 \
  RUST_TEST_THREADS=1 \
  cargo +"${TOOLCHAIN}" test -p hibana --lib \
    endpoint::kernel::core::public_operation::tests::export_public_operation_kernel_for_lean \
    -- --ignored --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${PUBLIC_OPERATION_GENERATED}" "public-operation"
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  global::const_dsl::receive_lane_causality::tests::export_causal_flow_for_lean \
  -- --ignored --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${CAUSAL_GENERATED}" "causal"
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  global::event_program_cursor_tests::lean_proof_export::parallel_certificate::export_parallel_dependencies_for_lean \
  -- --ignored --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${PARALLEL_GENERATED}" "parallel"
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::program_columns::export_program_columns_for_lean \
  -- --ignored --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${PROGRAM_COLUMNS_GENERATED}" "program columns"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${PARTIAL_CHOICE_GENERATED}" "partial choices"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${CONTROLLER_GENERATED}" "controller authority"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${INGRESS_GENERATED}" "actor ingress"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${REENTRY_GENERATED}" "actor roll preparation"
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::commit::owned_commits_publish_the_shared_actor_step_and_resource_view_together \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_COMMIT_GENERATED}" "actor commits"
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::reentry_choice::an_enclosing_choice_survives_two_delegated_roll_visits \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_REENTRY_COMMIT_GENERATED}" "actor body reentry commits"
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::reentry_parallel::parallel_loan_reentry_preserves_the_other_body_and_completed_tail \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_PARALLEL_REENTRY_GENERATED}" "actor parallel reentry commits"
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib -- \
  g::role_projection::tests::conditional_controller::remote_controller::a_conditional_controller_does_not_take_authority_from_its_co_resident_observer \
  g::role_projection::tests::conditional_controller::co_resident_controller::a_conditional_controller_does_not_take_authority_from_its_co_resident_observer \
  --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_ROUTE_LEFT_REENTRY_GENERATED}" "remote route reentry commits"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_ROUTE_RIGHT_REENTRY_GENERATED}" "co-resident route reentry commits"
run_audit generated-axioms --kernel "${ACTOR_ROUTE_LEFT_REENTRY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-route-left-reentry-generated-claim-surface.txt" 82
run_audit generated-axioms --kernel "${ACTOR_ROUTE_RIGHT_REENTRY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-route-right-reentry-generated-claim-surface.txt" 86
run_audit generated-axioms --kernel "${ACTOR_REENTRY_COMMIT_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-reentry-commit-generated-claim-surface.txt" 36
run_audit generated-axioms --kernel "${ACTOR_PARALLEL_REENTRY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-parallel-reentry-generated-claim-surface.txt" 38
run_audit generated-axioms --kernel "${ACTOR_COMMIT_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-commit-generated-claim-surface.txt" 69
run_audit generated-axioms --kernel "${PROGRAM_COLUMNS_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/program-columns-generated-claim-surface.txt" 139
run_audit generated-axioms --kernel "${PARTIAL_CHOICE_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/partial-choice-generated-claim-surface.txt" 30
run_audit generated-axioms --kernel "${CONTROLLER_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/controller-generated-claim-surface.txt" 57
run_audit generated-axioms --kernel "${INGRESS_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/ingress-generated-claim-surface.txt" 16
run_audit generated-axioms --kernel "${REENTRY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/reentry-generated-claim-surface.txt" 97
parallel_output="$(run_audit generated-axioms \
  --kernel "${PARALLEL_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/parallel-generated-claim-surface.txt" 182)"
printf '%s\n' "${parallel_output}"
printf '%s\n' "${parallel_output}" | bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard markers parallel
causal_output="$(run_audit generated-axioms \
  --kernel "${CAUSAL_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/causal-generated-claim-surface.txt" 36)"
printf '%s\n' "${causal_output}"
printf '%s\n' "${causal_output}" | bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard markers causal
lean_output="$(run_audit generated-axioms \
  "${GENERATED}" "${PROOF_DIR}" "${GENERATED_CLAIM_SNAPSHOT}")"
printf '%s\n' "${lean_output}"
printf '%s\n' "${lean_output}" | bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard markers generated
runtime_lean_output="$(run_audit generated-axioms \
  --kernel "${RUNTIME_GENERATED}" "${PROOF_DIR}" "${RUNTIME_CLAIM_SNAPSHOT}" 16)"
printf '%s\n' "${runtime_lean_output}"
printf '%s\n' "${runtime_lean_output}" | bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard markers runtime
public_operation_lean_output="$(run_audit \
  generated-axioms \
  --kernel "${PUBLIC_OPERATION_GENERATED}" "${PROOF_DIR}" \
  "${PUBLIC_OPERATION_CLAIM_SNAPSHOT}" 2)"
printf '%s\n' "${public_operation_lean_output}"
printf '%s\n' "${public_operation_lean_output}" | bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard markers public-operation

echo "Lean proof gate passed"
