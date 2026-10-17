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
ACTOR_COMMIT_GENERATED="${ROOT_DIR}/target/lean-proof/ActorCommitGenerated.lean"
ACTOR_REENTRY_COMMIT_GENERATED="${ROOT_DIR}/target/lean-proof/ActorReentryCommitGenerated.lean"
ACTOR_PARALLEL_REENTRY_GENERATED="${ROOT_DIR}/target/lean-proof/ActorParallelReentryGenerated.lean"
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
  --static "${PROOF_DIR}" "${STATIC_CLAIM_SNAPSHOT}" 818 442 270 106)"
printf '%s\n' "${static_audit_output}"
printf '%s\n' "${static_audit_output}" | bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard markers static
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" theorem-inventory \
  --example-types "${PROOF_DIR}" "${EXAMPLE_CLAIM_SNAPSHOT}" 36

source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
mkdir -p "$(dirname "${GENERATED}")"
rm -f "${GENERATED}" "${RUNTIME_GENERATED}" "${PUBLIC_OPERATION_GENERATED}" "${CAUSAL_GENERATED}" "${PARALLEL_GENERATED}" "${PROGRAM_COLUMNS_GENERATED}" "${PARTIAL_CHOICE_GENERATED}" "${CONTROLLER_GENERATED}" "${INGRESS_GENERATED}" "${REENTRY_GENERATED}" "${ACTOR_COMMIT_GENERATED}" "${ACTOR_REENTRY_COMMIT_GENERATED}" "${ACTOR_PARALLEL_REENTRY_GENERATED}" "${ACTOR_ROUTE_LEFT_REENTRY_GENERATED}" "${ACTOR_ROUTE_RIGHT_REENTRY_GENERATED}"
rm -f "${OWNED_RECOVERY_GENERATED}"
rm -f "${LOCAL_RETURN_GENERATED}"
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
  "${PROOF_DIR}/local-return-generated-claim-surface.txt" 8
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
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  g::role_projection::tests::conditional_controller::a_conditional_controller_does_not_take_authority_from_its_co_resident_observer \
  -- --exact
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_ROUTE_LEFT_REENTRY_GENERATED}" "remote route reentry commits"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard artifact "${ACTOR_ROUTE_RIGHT_REENTRY_GENERATED}" "co-resident route reentry commits"
run_audit generated-axioms --kernel "${ACTOR_ROUTE_LEFT_REENTRY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-route-left-reentry-generated-claim-surface.txt" 82
run_audit generated-axioms --kernel "${ACTOR_ROUTE_RIGHT_REENTRY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-route-right-reentry-generated-claim-surface.txt" 86
run_audit generated-axioms --kernel "${ACTOR_REENTRY_COMMIT_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-reentry-commit-generated-claim-surface.txt" 36
run_audit generated-axioms --kernel "${ACTOR_PARALLEL_REENTRY_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-parallel-reentry-generated-claim-surface.txt" 12
run_audit generated-axioms --kernel "${ACTOR_COMMIT_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/actor-commit-generated-claim-surface.txt" 48
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

echo "Lean proof gate passed toolchain=v4.30.0 traces=14 frames=66 projections=22 exact-descriptors=22 progress=4 projectability=8 distributed-progress=8 verified-protocols=8 static-theorems=818 owned-recovery-correspondence=5 local-return-correspondence=8 actor-commit-correspondence=48 actor-body-reentry-correspondence=36 actor-parallel-reentry-correspondence=12 actor-route-reentry-correspondence=168 program-column-correspondence=139 partial-choice-correspondence=30 controller-correspondence=57 actor-ingress-correspondence=16 actor-reentry-correspondence=97 parallel-descriptors=10 parallel-correspondence=182 causal-correspondence=36 anonymous-regressions=36 generated-theorems=506 generated-obligations=458 production-transitions=7 production-operations=6 production-owners=8 verified-codecs=3 verified-family=8 static-deployments=8 deployment-rejections=3 capabilities=6 runtime-regions=4 atomic-failures=4 public-operation-edges=18 public-operation-transitions=180 native-regressions=32"
