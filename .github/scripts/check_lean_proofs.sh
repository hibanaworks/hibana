#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF_DIR="${ROOT_DIR}/proofs/lean"
GENERATED="${ROOT_DIR}/target/lean-proof/Generated.lean"
RUNTIME_GENERATED="${ROOT_DIR}/target/lean-proof/RuntimeGenerated.lean"
EXPECTED_TOOLCHAIN="leanprover/lean4:v4.30.0"
EXPECTED_MARKER="hibana Lean generated proof passed traces=13 frames=55 projections=16 exact-descriptors=16 progress=4 projectability=7 verified-protocols=7"
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

declared_theorems="$(mktemp "${TMPDIR:-/tmp}/hibana-lean-theorems.XXXXXX")"
audited_theorems="$(mktemp "${TMPDIR:-/tmp}/hibana-lean-audit.XXXXXX")"
trap 'rm -f "${declared_theorems}" "${audited_theorems}"' EXIT
rg --no-filename '^(protected[[:space:]]+)?theorem[[:space:]]+[A-Za-z0-9_.]+' \
  "${PROOF_DIR}/Hibana" --glob '*.lean' \
  | sed -E 's/^(protected[[:space:]]+)?theorem[[:space:]]+([A-Za-z0-9_.]+).*/\2/' \
  | LC_ALL=C sort -u >"${declared_theorems}"
sed -n 's/^#print axioms Hibana\.\([A-Za-z0-9_.]*\)$/\1/p' \
  "${PROOF_DIR}/Hibana/AxiomAudit.lean" \
  | LC_ALL=C sort -u >"${audited_theorems}"
if ! diff -u "${declared_theorems}" "${audited_theorems}"; then
  echo "Lean proof gate requires an axiom audit for every exported theorem" >&2
  exit 1
fi

(
  cd "${PROOF_DIR}"
  lake build
)

axiom_output="$(cd "${PROOF_DIR}" && lake env lean Hibana/AxiomAudit.lean)"
printf '%s\n' "${axiom_output}"
if [[ "$(grep -Fc "depends on axioms: [propext, Quot.sound]" <<<"${axiom_output}")" != "163" ]] \
  || [[ "$(grep -Fc "Classical.choice" <<<"${axiom_output}")" != "0" ]] \
  || [[ "$(grep -Fc "depends on axioms: [propext]" <<<"${axiom_output}")" != "130" ]] \
  || [[ "$(grep -Fc "does not depend on any axioms" <<<"${axiom_output}")" != "26" ]] \
  || [[ "$(wc -l <<<"${axiom_output}" | tr -d ' ')" != "319" ]]; then
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

echo "Lean proof gate passed toolchain=v4.30.0 traces=13 frames=55 projections=16 exact-descriptors=16 progress=4 projectability=7 verified-protocols=7 runtime-regions=5 atomic-failures=4"
