#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF_DIR="${ROOT_DIR}/proofs/lean"
GENERATED="${ROOT_DIR}/target/lean-proof/Generated.lean"
RUNTIME_GENERATED="${ROOT_DIR}/target/lean-proof/RuntimeGenerated.lean"
PUBLIC_OPERATION_GENERATED="${ROOT_DIR}/target/lean-proof/PublicOperationGenerated.lean"
EXPECTED_TOOLCHAIN="leanprover/lean4:v4.30.0"
EXPECTED_MARKER="hibana Lean generated proof passed traces=14 frames=66 projections=22 exact-descriptors=22 progress=4 projectability=8 distributed-progress=8 verified-protocols=8"
EXPECTED_PRODUCTION_MARKER="hibana Lean production evidence passed transitions=7 operations=6 owners=8 codecs=3 family=8 deployments=8 deployment-rejections=3 capabilities=6 agreement=static-exact-family profile=closing"
EXPECTED_RUNTIME_MARKER="hibana Lean runtime proof passed regions=4 poison=1 generation=1 atomic-failures=4"
EXPECTED_PUBLIC_OPERATION_MARKER="hibana Lean public-operation kernel proof passed states=9 transitions=81"
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
axiom_both_count="$(awk '
  /depends on axioms: \[propext, Quot\.sound\]/ { count += 1 }
  /depends on axioms: \[propext,$/ { count += 1 }
  END { print count + 0 }
' <<<"${axiom_output}")"
readonly EXPECTED_AXIOM_BOTH_COUNT=274
readonly EXPECTED_AXIOM_PROPEXT_COUNT=195
readonly EXPECTED_AXIOM_FREE_COUNT=68
readonly EXPECTED_EXPORTED_THEOREM_COUNT=537
if [[ "${axiom_both_count}" != "${EXPECTED_AXIOM_BOTH_COUNT}" ]] \
  || [[ "$(grep -Fc "Classical.choice" <<<"${axiom_output}")" != "0" ]] \
  || [[ "$(grep -Fc "native_decide.ax" <<<"${axiom_output}")" != "0" ]] \
  || [[ "$(grep -Fc "depends on axioms: [propext]" <<<"${axiom_output}")" != "${EXPECTED_AXIOM_PROPEXT_COUNT}" ]] \
  || [[ "$(grep -Fc "does not depend on any axioms" <<<"${axiom_output}")" != "${EXPECTED_AXIOM_FREE_COUNT}" ]] \
  || [[ "$(grep -Ec "^'Hibana\\." <<<"${axiom_output}")" != "${EXPECTED_EXPORTED_THEOREM_COUNT}" ]]; then
  echo "Lean proof gate axiom set changed" >&2
  exit 1
fi

source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
mkdir -p "$(dirname "${GENERATED}")"
rm -f "${GENERATED}" "${RUNTIME_GENERATED}" "${PUBLIC_OPERATION_GENERATED}"
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
    endpoint::kernel::core::public_types::tests::export_public_operation_kernel_for_lean \
    -- --ignored --exact
if [[ ! -s "${PUBLIC_OPERATION_GENERATED}" ]]; then
  echo "Lean proof gate public-operation exporter did not create a nonempty artifact" >&2
  exit 1
fi

lean_output="$(cd "${PROOF_DIR}" && lake env lean "${GENERATED}")"
printf '%s\n' "${lean_output}"
if [[ "${lean_output}" != *"${EXPECTED_MARKER}"* ]]; then
  echo "Lean proof gate generated certificate mismatch" >&2
  exit 1
fi
if [[ "${lean_output}" != *"${EXPECTED_PRODUCTION_MARKER}"* ]]; then
  echo "Lean proof gate production evidence mismatch" >&2
  exit 1
fi
runtime_lean_output="$(cd "${PROOF_DIR}" && lake env lean "${RUNTIME_GENERATED}")"
printf '%s\n' "${runtime_lean_output}"
if [[ "${runtime_lean_output}" != *"${EXPECTED_RUNTIME_MARKER}"* ]]; then
  echo "Lean proof gate runtime certificate mismatch" >&2
  exit 1
fi
public_operation_lean_output="$(cd "${PROOF_DIR}" && lake env lean "${PUBLIC_OPERATION_GENERATED}")"
printf '%s\n' "${public_operation_lean_output}"
if [[ "${public_operation_lean_output}" != *"${EXPECTED_PUBLIC_OPERATION_MARKER}"* ]]; then
  echo "Lean proof gate public-operation certificate mismatch" >&2
  exit 1
fi

echo "Lean proof gate passed toolchain=v4.30.0 traces=14 frames=66 projections=22 exact-descriptors=22 progress=4 projectability=8 distributed-progress=8 verified-protocols=8 production-transitions=7 production-operations=6 production-owners=8 verified-codecs=3 verified-family=8 static-deployments=8 deployment-rejections=3 capabilities=6 runtime-regions=4 atomic-failures=4 public-operation-transitions=81"
