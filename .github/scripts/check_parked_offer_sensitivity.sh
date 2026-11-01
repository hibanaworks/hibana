#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
OUTPUT="${ROOT_DIR}/target/parked-offer-sensitivity"
MUTANT="${OUTPUT}/source"
mkdir -p "${OUTPUT}"
if [[ -e "${MUTANT}" ]]; then
  echo "Previous sensitivity workspace remains: ${MUTANT}" >&2
  exit 1
fi
cd "${ROOT_DIR}"
git rev-parse HEAD > "${OUTPUT}/revision.txt"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
bash .github/scripts/check_protocol_shapes.sh --contracts 8 1 > "${OUTPUT}/positive.log" 2>&1
git clone --quiet --shared --no-checkout "${ROOT_DIR}" "${MUTANT}"
# Retain the current source and give the mutant a separate Cargo target.
git ls-files --cached --others --exclude-standard -z |
  perl -0ne 'print "$_\0" for grep { -f $_ } split /\0/' |
  rsync -a --from0 --files-from=- ./ "${MUTANT}/"
git -C "${MUTANT}" add -A
git -C "${MUTANT}" diff --cached --binary HEAD > "${OUTPUT}/current-source.patch"
git -C "${MUTANT}" apply --check "${ROOT_DIR}/proofs/checks/runtime/parked-offer/RestoreFault.patch"
git -C "${MUTANT}" apply "${ROOT_DIR}/proofs/checks/runtime/parked-offer/RestoreFault.patch"
git -C "${MUTANT}" diff --binary > "${OUTPUT}/mutation.patch"
status=0
(
  cd "${MUTANT}"
  bash .github/scripts/check_protocol_shapes.sh --contracts 8 1
) > "${OUTPUT}/mutant.log" 2>&1 || status=$?
if [[ "${status}" == 0 ]] ||
   ! grep -Fq 'Error: "generated test shape_24 failed: exit status: 101"' "${OUTPUT}/mutant.log" ||
   ! grep -Fq 'EndpointError { operation: "send", kind: PhaseInvariant }' "${OUTPUT}/mutant.log"; then
  echo "Generated nested localside did not detect the restored offer defect" >&2
  tail -50 "${OUTPUT}/mutant.log" >&2
  exit 1
fi
relative="target/protocol-shapes/contracts/range-8-count-1"
for checkout in "${ROOT_DIR}" "${MUTANT}"; do
  (cd "${checkout}/${relative}" && shasum -a 256 --check inputs.sha256) \
    > "${OUTPUT}/inputs-check-$(basename "${checkout}").log"
done
sed 's/^[^ ]*  //' "${ROOT_DIR}/${relative}/inputs.sha256" > "${OUTPUT}/inputs.txt"
sed 's/^[^ ]*  //' "${MUTANT}/${relative}/inputs.sha256" > "${OUTPUT}/mutant-inputs.txt"
cmp "${OUTPUT}/inputs.txt" "${OUTPUT}/mutant-inputs.txt"
while IFS= read -r file; do
  cmp <(SENSITIVITY_ROOT="${ROOT_DIR}" perl -pe \
    's/\Q$ENV{SENSITIVITY_ROOT}\E/__HIBANA_SOURCE__/g' "${ROOT_DIR}/${relative}/${file}") \
    <(SENSITIVITY_ROOT="${MUTANT}" perl -pe \
    's/\Q$ENV{SENSITIVITY_ROOT}\E/__HIBANA_SOURCE__/g' "${MUTANT}/${relative}/${file}")
done < "${OUTPUT}/inputs.txt"
mkdir -p "${OUTPUT}/mutant-inputs"
rsync -a --exclude=target \
  "${MUTANT}/target/protocol-shapes/contracts/range-8-count-1/" \
  "${OUTPUT}/mutant-inputs/"
printf 'mutant_exit=%s\nexpected_failed_case=shape_24\nexpected_failure=completed_localside_send_phase_invariant\n' "${status}" > "${OUTPUT}/result.txt"
rm -r -- "${MUTANT}"
echo "Parked offer sensitivity: current source passes; restored production fails a generated localside"
