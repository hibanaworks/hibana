#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF_DIR="${ROOT_DIR}/proofs/lean"
GENERATED="${ROOT_DIR}/target/lean-proof/Generated.lean"
EXPECTED_TOOLCHAIN="leanprover/lean4:v4.30.0"
EXPECTED_MARKER="hibana Lean trace proof passed artifacts=13 frames=55"
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
  program_trace_checker_sound; do
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
if [[ "$(grep -Fc "depends on axioms: [propext, Quot.sound]" <<<"${axiom_output}")" != "14" ]] \
  || [[ "$(grep -Fc "Classical.choice" <<<"${axiom_output}")" != "0" ]] \
  || [[ "$(grep -Fc "depends on axioms: [propext]" <<<"${axiom_output}")" != "8" ]] \
  || [[ "$(grep -Fc "does not depend on any axioms" <<<"${axiom_output}")" != "4" ]] \
  || [[ "$(wc -l <<<"${axiom_output}" | tr -d ' ')" != "26" ]]; then
  echo "Lean proof gate axiom set changed" >&2
  exit 1
fi

source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
mkdir -p "$(dirname "${GENERATED}")"
CARGO_BUILD_JOBS=1 \
  RUST_TEST_THREADS=1 \
  cargo +"${TOOLCHAIN}" test -p hibana --lib \
    global::event_program_cursor_tests::lean_proof_export::export_production_trace_for_lean \
    -- --ignored --exact

lean_output="$(cd "${PROOF_DIR}" && lake env lean "${GENERATED}")"
printf '%s\n' "${lean_output}"
if [[ "${lean_output}" != *"${EXPECTED_MARKER}"* ]]; then
  echo "Lean proof gate artifact-count mismatch" >&2
  exit 1
fi

echo "Lean proof gate passed toolchain=v4.30.0 artifacts=13 frames=55"
