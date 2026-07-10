#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF_DIR="${ROOT_DIR}/proofs/lean"
GENERATED="${ROOT_DIR}/target/lean-proof/Generated.lean"
RUNTIME_GENERATED="${ROOT_DIR}/target/lean-proof/RuntimeGenerated.lean"
EXPECTED_TOOLCHAIN="leanprover/lean4:v4.30.0"
EXPECTED_MARKER="hibana Lean generated proof passed traces=13 frames=55 projections=7 progress=4"
EXPECTED_RUNTIME_MARKER="hibana Lean runtime proof passed regions=5 poison=1 generation=1 atomic-failures=4"
TOOLCHAIN="${TOOLCHAIN:-1.95.0}"

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
if rg -n --glob '*.lean' \
  '\b(sorry|admit)\b|^[[:space:]]*(axiom|constant|opaque|unsafe)\b' \
  "${PROOF_DIR}"; then
  echo "Lean proof gate forbids sorry, admit, and custom axioms" >&2
  exit 1
fi
if rg -n 'Mathlib|^[[:space:]]*\[\[require\]\]|^[[:space:]]*git[[:space:]]*=' \
  "${PROOF_DIR}/lakefile.toml" "${PROOF_DIR}/lake-manifest.json"; then
  echo "Lean proof gate must remain Core/Std-only" >&2
  exit 1
fi

for theorem in \
  artifact_checker_sound \
  trace_checker_sound \
  commit_marks_event_done \
  selected_arm_unique \
  reset_roll_preserves_outside_done \
  reset_roll_clears_done \
  reset_roll_preserves_outside_selection \
  reset_roll_clears_selection \
  reset_route_arm_preserves_selection \
  reset_route_arm_preserves_outside_done \
  reset_route_arm_clears_done \
  local_action_send_recv_duality \
  local_action_uninvolved_projects_none \
  local_action_self_send_projects_local \
  local_action_preserves_label \
  commit_exact_successor \
  resolver_reject_never_selects \
  resolver_select_never_rejects \
  resolver_selection_exact_authority \
  selected_route_is_not_resolvable \
  unresolved_dynamic_route_cannot_commit \
  unresolved_dynamic_commit_rejected \
  wrong_resolver_id_rejected \
  resolver_reuse_without_reset_rejected \
  resolver_reject_is_terminal \
  program_trace_checker_sound \
  projection_certificate_sound \
  slab_layout_certificate_sound \
  logical_progress_checker_sound \
  progress_certificate_sound \
  reachable_state_has_logical_progress \
  next_lease_generation_strictly_increases \
  next_lease_generation_stays_nonzero \
  next_lease_generation_stays_in_domain \
  max_lease_generation_is_exhausted \
  session_generation_run_sound \
  session_generation_trace_checker_sound \
  poisoned_generation_step_never_revives \
  poisoned_generation_attach_rejected \
  poisoned_generation_publish_rejected \
  publication_permitted_implies_live \
  poisoned_generation_trace_never_revives \
  failed_lease_allocation_preserves_state \
  poisoned_generation_aborts_lease_publication \
  successful_lease_allocation_commits_exact_plan \
  prepared_lease_generation_strictly_increases \
  prepared_lease_capacity_never_shrinks \
  lease_allocation_failure_certificate_sound \
  lease_allocation_abort_certificate_sound \
  route_arm_decode_encode_round_trip \
  route_arm_decode_accepts_only_binary \
  invalid_route_arm_decode_rejected \
  invalid_route_arm_has_no_publication \
  valid_route_arm_publication_is_exact \
  invalid_ready_arm_mask_rejected \
  valid_ready_arm_mask_is_accepted \
  selected_ready_arm_mask_is_exact; do
  if ! rg -q "^theorem ${theorem}\b" "${PROOF_DIR}/Hibana"; then
    echo "Lean proof gate missing theorem: ${theorem}" >&2
    exit 1
  fi
done

(
  cd "${PROOF_DIR}"
  lake build
)

axiom_output="$(cd "${PROOF_DIR}" && lake env lean Hibana/AxiomAudit.lean)"
printf '%s\n' "${axiom_output}"
if [[ "$(grep -Fc "depends on axioms: [propext, Quot.sound]" <<<"${axiom_output}")" != "21" ]] \
  || [[ "$(grep -Fc "Classical.choice" <<<"${axiom_output}")" != "0" ]] \
  || [[ "$(grep -Fc "depends on axioms: [propext]" <<<"${axiom_output}")" != "28" ]] \
  || [[ "$(grep -Fc "does not depend on any axioms" <<<"${axiom_output}")" != "8" ]] \
  || [[ "$(wc -l <<<"${axiom_output}" | tr -d ' ')" != "57" ]]; then
  echo "Lean proof gate axiom set changed" >&2
  exit 1
fi

source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
mkdir -p "$(dirname "${GENERATED}")"
rm -f "${GENERATED}" "${RUNTIME_GENERATED}"
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

lean_output="$(cd "${PROOF_DIR}" && lake env lean "${GENERATED}")"
printf '%s\n' "${lean_output}"
if [[ "${lean_output}" != *"${EXPECTED_MARKER}"* ]]; then
  echo "Lean proof gate generated certificate mismatch" >&2
  exit 1
fi
runtime_lean_output="$(cd "${PROOF_DIR}" && lake env lean "${RUNTIME_GENERATED}")"
printf '%s\n' "${runtime_lean_output}"
if [[ "${runtime_lean_output}" != *"${EXPECTED_RUNTIME_MARKER}"* ]]; then
  echo "Lean proof gate runtime certificate mismatch" >&2
  exit 1
fi

echo "Lean proof gate passed toolchain=v4.30.0 traces=13 frames=55 projections=7 progress=4 runtime-regions=5 atomic-failures=4"
