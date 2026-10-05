#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF_DIR="${ROOT_DIR}/proofs/lean"
GENERATED="${ROOT_DIR}/target/lean-proof/Generated.lean"
RUNTIME_GENERATED="${ROOT_DIR}/target/lean-proof/RuntimeGenerated.lean"
PUBLIC_OPERATION_GENERATED="${ROOT_DIR}/target/lean-proof/PublicOperationGenerated.lean"
CAUSAL_GENERATED="${ROOT_DIR}/target/lean-proof/CausalGenerated.lean"
PARALLEL_GENERATED="${ROOT_DIR}/target/lean-proof/ParallelGenerated.lean"
EXPECTED_TOOLCHAIN="leanprover/lean4:v4.30.0"
EXPECTED_MARKER="hibana Lean generated certificate check passed traces=14 frames=66 projections=22 exact-descriptors=22 progress=4 projectability=8 distributed-progress=8 verified-protocols=8"
EXPECTED_PRODUCTION_MARKER="hibana Lean production artifact passed transitions=7 operations=6 owners=8 kernel-refinement=external-premise owner-evidence=external-premise codecs=3 family=8 deployments=8 deployment-rejections=3 capabilities=6 agreement=static-exact-family profile=closing"
EXPECTED_RUNTIME_MARKER="hibana Lean runtime proof passed regions=4 poison=1 generation=1 atomic-failures=4"
EXPECTED_PUBLIC_OPERATION_MARKER="hibana Lean public-operation kernel proof passed states=9 edges=16 transitions=144"
EXPECTED_GENERATED_AXIOM_MARKER="Generated Lean axiom audit passed theorems=506 kernel=466 native=40 contracts=48 obligations=458 native-decisions=16 claims=506"
EXPECTED_RUNTIME_AUDIT_MARKER="Generated Lean kernel artifact audit passed artifact=RuntimeGenerated theorems=16 claims=16"
EXPECTED_PUBLIC_OPERATION_AUDIT_MARKER="Generated Lean kernel artifact audit passed artifact=PublicOperationGenerated theorems=2 claims=2"
EXPECTED_STATIC_AUDIT_MARKER="Lean static theorem audit passed theorems=709 both=364 propext=248 free=97"
GENERATED_CLAIM_SNAPSHOT="${PROOF_DIR}/generated-claim-surface.txt"
RUNTIME_CLAIM_SNAPSHOT="${PROOF_DIR}/runtime-generated-claim-surface.txt"
PUBLIC_OPERATION_CLAIM_SNAPSHOT="${PROOF_DIR}/public-operation-generated-claim-surface.txt"
STATIC_CLAIM_SNAPSHOT="${PROOF_DIR}/all-claim-surface.txt"
EXAMPLE_CLAIM_SNAPSHOT="${PROOF_DIR}/example-claim-surface.txt"
TOOLCHAIN="${TOOLCHAIN:-1.95.0}"

# Preserve the auditor's failure evidence even when the caller captures stdout.
run_audit() {
  local audit_output audit_status
  if audit_output="$(python3 "$@")"; then
    printf '%s\n' "${audit_output}"
  else
    audit_status=$?
    printf '%s\n' "${audit_output}" >&2
    return "${audit_status}"
  fi
}

if [[ ! -f "${PROOF_DIR}/lean-toolchain" ]] \
  || [[ "$(< "${PROOF_DIR}/lean-toolchain")" != "${EXPECTED_TOOLCHAIN}" ]]; then
  echo "Lean proof gate requires pinned toolchain ${EXPECTED_TOOLCHAIN}" >&2
  exit 1
fi
if ! command -v lake >/dev/null 2>&1; then
  echo "Lean proof gate requires lake from pinned elan" >&2
  exit 1
fi
if [[ "$(cd "${PROOF_DIR}" && lake env lean --version)" != *"version 4.30.0"* ]]; then
  echo "Lean proof gate toolchain mismatch" >&2
  exit 1
fi
python3 "${ROOT_DIR}/.github/scripts/check_lean_theorem_inventory.py" --self-test
python3 "${ROOT_DIR}/.github/scripts/check_generated_lean_axioms.py" --self-test

(
  cd "${PROOF_DIR}"
  lake build
)
bash "${ROOT_DIR}/.github/scripts/check_lean_claim_surface.sh"
static_audit_output="$(run_audit \
  "${ROOT_DIR}/.github/scripts/check_lean_theorem_inventory.py" \
  --static "${PROOF_DIR}" "${STATIC_CLAIM_SNAPSHOT}" 709 364 248 97)"
printf '%s\n' "${static_audit_output}"
if [[ "${static_audit_output}" != *"${EXPECTED_STATIC_AUDIT_MARKER}"* ]]; then
  echo "Lean proof gate static theorem boundary mismatch" >&2
  exit 1
fi
python3 "${ROOT_DIR}/.github/scripts/check_lean_theorem_inventory.py" \
  --example-types "${PROOF_DIR}" "${EXAMPLE_CLAIM_SNAPSHOT}" 36

source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
mkdir -p "$(dirname "${GENERATED}")"
rm -f "${GENERATED}" "${RUNTIME_GENERATED}" "${PUBLIC_OPERATION_GENERATED}" "${CAUSAL_GENERATED}" "${PARALLEL_GENERATED}"
CARGO_BUILD_JOBS=1 \
  RUST_TEST_THREADS=1 \
cargo +"${TOOLCHAIN}" test -p hibana --lib \
    global::event_program_cursor_tests::lean_proof_export::export_production_trace_for_lean \
    -- --ignored --exact
if [[ ! -s "${GENERATED}" ]]; then
  echo "Lean proof gate production trace exporter did not create a nonempty artifact" >&2
  exit 1
fi
CARGO_BUILD_JOBS=1 \
  RUST_TEST_THREADS=1 \
  cargo +"${TOOLCHAIN}" test -p hibana --lib \
    rendezvous::core::storage_layout::capacity::tests::formal_certificate_export::export_runtime_certificates_for_lean \
    -- --ignored --exact
if [[ ! -s "${RUNTIME_GENERATED}" ]]; then
  echo "Lean proof gate runtime exporter did not create a nonempty artifact" >&2
  exit 1
fi
CARGO_BUILD_JOBS=1 \
  RUST_TEST_THREADS=1 \
  cargo +"${TOOLCHAIN}" test -p hibana --lib \
    endpoint::kernel::core::public_operation::tests::export_public_operation_kernel_for_lean \
    -- --ignored --exact
if [[ ! -s "${PUBLIC_OPERATION_GENERATED}" ]]; then
  echo "Lean proof gate public-operation exporter did not create a nonempty artifact" >&2
  exit 1
fi
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  global::const_dsl::receive_lane_causality::tests::export_causal_flow_for_lean \
  -- --ignored --exact
if [[ ! -s "${CAUSAL_GENERATED}" ]]; then
  echo "Lean proof gate causal exporter did not create a nonempty artifact" >&2
  exit 1
fi
CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo +"${TOOLCHAIN}" test -p hibana --lib \
  global::event_program_cursor_tests::lean_proof_export::parallel_certificate::export_parallel_dependencies_for_lean \
  -- --ignored --exact
if [[ ! -s "${PARALLEL_GENERATED}" ]]; then
  echo "Lean proof gate parallel exporter did not create a nonempty artifact" >&2
  exit 1
fi
parallel_output="$(run_audit "${ROOT_DIR}/.github/scripts/check_generated_lean_axioms.py" \
  --kernel "${PARALLEL_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/parallel-generated-claim-surface.txt" 182)"
printf '%s\n' "${parallel_output}"
if [[ "${parallel_output}" != *"Generated Lean kernel artifact audit passed artifact=ParallelGenerated theorems=182 claims=182"* ]]; then
  echo "Lean proof gate parallel descriptor correspondence mismatch" >&2
  exit 1
fi
causal_output="$(run_audit "${ROOT_DIR}/.github/scripts/check_generated_lean_axioms.py" \
  --kernel "${CAUSAL_GENERATED}" "${PROOF_DIR}" \
  "${PROOF_DIR}/causal-generated-claim-surface.txt" 36)"
printf '%s\n' "${causal_output}"
if [[ "${causal_output}" != *"Generated Lean kernel artifact audit passed artifact=CausalGenerated theorems=36 claims=36"* ]]; then
  echo "Lean proof gate causal correspondence boundary mismatch" >&2
  exit 1
fi
lean_output="$(run_audit "${ROOT_DIR}/.github/scripts/check_generated_lean_axioms.py" \
  "${GENERATED}" "${PROOF_DIR}" "${GENERATED_CLAIM_SNAPSHOT}")"
printf '%s\n' "${lean_output}"
if [[ "${lean_output}" != *"${EXPECTED_MARKER}"* ]]; then
  echo "Lean proof gate generated certificate mismatch" >&2
  exit 1
fi
if [[ "${lean_output}" != *"${EXPECTED_PRODUCTION_MARKER}"* ]]; then
  echo "Lean proof gate production evidence mismatch" >&2
  exit 1
fi
if [[ "${lean_output}" != *"${EXPECTED_GENERATED_AXIOM_MARKER}"* ]]; then
  echo "Lean proof gate generated axiom boundary mismatch" >&2
  exit 1
fi
runtime_lean_output="$(run_audit "${ROOT_DIR}/.github/scripts/check_generated_lean_axioms.py" \
  --kernel "${RUNTIME_GENERATED}" "${PROOF_DIR}" "${RUNTIME_CLAIM_SNAPSHOT}" 16)"
printf '%s\n' "${runtime_lean_output}"
if [[ "${runtime_lean_output}" != *"${EXPECTED_RUNTIME_MARKER}"* ]]; then
  echo "Lean proof gate runtime certificate mismatch" >&2
  exit 1
fi
if [[ "${runtime_lean_output}" != *"${EXPECTED_RUNTIME_AUDIT_MARKER}"* ]]; then
  echo "Lean proof gate runtime theorem boundary mismatch" >&2
  exit 1
fi
public_operation_lean_output="$(run_audit \
  "${ROOT_DIR}/.github/scripts/check_generated_lean_axioms.py" \
  --kernel "${PUBLIC_OPERATION_GENERATED}" "${PROOF_DIR}" \
  "${PUBLIC_OPERATION_CLAIM_SNAPSHOT}" 2)"
printf '%s\n' "${public_operation_lean_output}"
if [[ "${public_operation_lean_output}" != *"${EXPECTED_PUBLIC_OPERATION_MARKER}"* ]]; then
  echo "Lean proof gate public-operation certificate mismatch" >&2
  exit 1
fi
if [[ "${public_operation_lean_output}" != *"${EXPECTED_PUBLIC_OPERATION_AUDIT_MARKER}"* ]]; then
  echo "Lean proof gate public-operation theorem boundary mismatch" >&2
  exit 1
fi

echo "Lean proof gate passed toolchain=v4.30.0 traces=14 frames=66 projections=22 exact-descriptors=22 progress=4 projectability=8 distributed-progress=8 verified-protocols=8 static-theorems=709 parallel-descriptors=10 parallel-correspondence=182 causal-correspondence=36 anonymous-regressions=36 generated-theorems=506 generated-obligations=458 production-transitions=7 production-operations=6 production-owners=8 verified-codecs=3 verified-family=8 static-deployments=8 deployment-rejections=3 capabilities=6 runtime-regions=4 atomic-failures=4 public-operation-edges=16 public-operation-transitions=144 native-regressions=32"
