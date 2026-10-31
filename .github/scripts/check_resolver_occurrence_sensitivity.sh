#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
OUTPUT="${ROOT_DIR}/target/resolver-occurrence-sensitivity"
MUTANT="${OUTPUT}/source"
CHANGED="src/global/typestate/cursor/scope_route/send_preview.rs"
mkdir -p "${OUTPUT}"
if [[ -e "${MUTANT}" ]]; then
  echo "Previous sensitivity workspace remains: ${MUTANT}" >&2
  exit 1
fi
cd "${ROOT_DIR}"
git rev-parse HEAD > "${OUTPUT}/revision.txt"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
bash .github/scripts/check_protocol_shapes.sh --contracts 2 1 \
  > "${OUTPUT}/positive.log" 2>&1
git clone --quiet --shared --no-checkout "${ROOT_DIR}" "${MUTANT}"
# Copy the actual current source, including new verification files. Build
# directories are ignored, and the mutant receives its own Cargo target.
git ls-files --cached --others --exclude-standard -z |
  perl -0ne 'print "$_\0" for grep { -f $_ } split /\0/' |
  rsync -a --from0 --files-from=- ./ "${MUTANT}/"
git -C "${MUTANT}" add -A
git -C "${MUTANT}" diff --cached --binary HEAD > "${OUTPUT}/current-source.patch"
git -C "${MUTANT}" apply --unidiff-zero --check \
  "${ROOT_DIR}/proofs/checks/runtime/resolver-occurrence/RestoreFault.patch"
git -C "${MUTANT}" apply --unidiff-zero \
  "${ROOT_DIR}/proofs/checks/runtime/resolver-occurrence/RestoreFault.patch"
if [[ "$(git -C "${MUTANT}" diff --name-only)" != "${CHANGED}" ]]; then
  echo "Sensitivity mutation changed an unexpected source file" >&2
  exit 1
fi
git -C "${MUTANT}" diff --binary > "${OUTPUT}/mutation.patch"
status=0
(
  cd "${MUTANT}"
  bash .github/scripts/check_protocol_shapes.sh --contracts 2 1
) > "${OUTPUT}/mutant.log" 2>&1 || status=$?
if [[ "${status}" == 0 ]] ||
   ! rg -q 'Error: "generated test shape_8 failed:' "${OUTPUT}/mutant.log"; then
  echo "Generated cases did not detect the restored send-selection defect" >&2
  tail -50 "${OUTPUT}/mutant.log" >&2
  exit 1
fi
mkdir -p "${OUTPUT}/mutant-inputs"
rsync -a --exclude=target \
  "${MUTANT}/target/protocol-shapes/contracts/range-2-count-1/" \
  "${OUTPUT}/mutant-inputs/"
printf 'mutant_exit=%s\nexpected_failed_case=shape_8\n' "${status}" \
  > "${OUTPUT}/result.txt"
rm -r -- "${MUTANT}"
echo "Resolver occurrence sensitivity: current source passes; restored defect fails shape_8"
