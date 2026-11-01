#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
OUTPUT="${ROOT_DIR}/target/recv-contract-sensitivity"
MUTANT="${OUTPUT}/source"
CHANGED="src/endpoint/kernel/recv.rs"
mkdir -p "${OUTPUT}"
if [[ -e "${MUTANT}" ]]; then
  echo "Previous sensitivity workspace remains: ${MUTANT}" >&2
  exit 1
fi
: > "${OUTPUT}/result.txt"
cd "${ROOT_DIR}"
git rev-parse HEAD > "${OUTPUT}/revision.txt"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
for rank in 6 7; do
  bash .github/scripts/check_protocol_shapes.sh --contracts "${rank}" 1 \
    > "${OUTPUT}/positive-${rank}.log" 2>&1
done
git clone --quiet --shared --no-checkout "${ROOT_DIR}" "${MUTANT}"
git ls-files --cached --others --exclude-standard -z |
  perl -0ne 'print "$_\0" for grep { -f $_ } split /\0/' |
  rsync -a --from0 --files-from=- ./ "${MUTANT}/"
git -C "${MUTANT}" add -A
git -C "${MUTANT}" diff --cached --binary HEAD > "${OUTPUT}/current-source.patch"
patch="${ROOT_DIR}/proofs/checks/runtime/parallel-offer-ingress/RestoreReceiveFault.patch"
git -C "${MUTANT}" apply --unidiff-zero --check "${patch}"
git -C "${MUTANT}" apply --unidiff-zero "${patch}"
if [[ "$(git -C "${MUTANT}" diff --name-only)" != "${CHANGED}" ]]; then
  echo "Sensitivity mutation changed an unexpected source file" >&2
  exit 1
fi
git -C "${MUTANT}" diff --binary > "${OUTPUT}/mutation.patch"
for rank in 6 7; do
  status=0
  (
    cd "${MUTANT}"
    bash .github/scripts/check_protocol_shapes.sh --contracts "${rank}" 1
  ) > "${OUTPUT}/mutant-${rank}.log" 2>&1 || status=$?
  if [[ "${rank}" == 6 ]]; then
    # Short-prefix exploration extends successful runtime calls. Restoring the
    # defect rejects a required receive, so later required probes are missing.
    expected='uncaught exception: shape 19: incomplete probe inventory'
  else
    # Delegated source traces already supply these prefixes; the same defect
    # therefore produces an explicit model-only admission mismatch.
    expected='generated Endpoint/source conformance failed: runtime-only=0 model-only=[1-9]'
  fi
  if [[ "${status}" == 0 ]] || ! grep -Eq "${expected}" "${OUTPUT}/mutant-${rank}.log"; then
    echo "Generated case ${rank} did not detect the restored receive-selection defect" >&2
    tail -50 "${OUTPUT}/mutant-${rank}.log" >&2
    exit 1
  fi
  if [[ "${rank}" == 7 ]] && ! grep -Fq 'shape 22 choice=' "${OUTPUT}/mutant-${rank}.log"; then
    echo "The delegated receive mutation failed outside its required admission probe" >&2
    exit 1
  fi
  relative="target/protocol-shapes/contracts/range-${rank}-count-1"
  # Generated manifests and #[path] imports refer to their own checkout.
  # Verify each raw manifest, then compare every input with only that root
  # replaced. Protocols, contracts, scripts and compiler options stay exact.
  for checkout in "${ROOT_DIR}" "${MUTANT}"; do
    (cd "${checkout}/${relative}" && shasum -a 256 --check inputs.sha256) \
      > "${OUTPUT}/inputs-check-${rank}-$(basename "${checkout}").log"
  done
  sed 's/^[^ ]*  //' "${ROOT_DIR}/${relative}/inputs.sha256" > "${OUTPUT}/inputs-${rank}.txt"
  sed 's/^[^ ]*  //' "${MUTANT}/${relative}/inputs.sha256" > "${OUTPUT}/mutant-inputs-${rank}.txt"
  cmp "${OUTPUT}/inputs-${rank}.txt" "${OUTPUT}/mutant-inputs-${rank}.txt"
  while IFS= read -r file; do
    cmp <(SENSITIVITY_ROOT="${ROOT_DIR}" perl -pe \
      's/\Q$ENV{SENSITIVITY_ROOT}\E/__HIBANA_SOURCE__/g' "${ROOT_DIR}/${relative}/${file}") \
      <(SENSITIVITY_ROOT="${MUTANT}" perl -pe \
      's/\Q$ENV{SENSITIVITY_ROOT}\E/__HIBANA_SOURCE__/g' "${MUTANT}/${relative}/${file}")
  done < "${OUTPUT}/inputs-${rank}.txt"
  mkdir -p "${OUTPUT}/mutant-inputs/range-${rank}-count-1"
  rsync -a --exclude=target "${MUTANT}/${relative}/" \
    "${OUTPUT}/mutant-inputs/range-${rank}-count-1/"
  printf 'rank=%s\tmutant_exit=%s\n' "${rank}" "${status}" >> "${OUTPUT}/result.txt"
done
rm -r -- "${MUTANT}"
echo "Receive contract sensitivity: current source passes; restored defect fails generated cases"
