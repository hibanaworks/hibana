#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
mkdir -p "${ROOT_DIR}/target/scope-reentry-sensitivity"
OUTPUT="$(mktemp -d "${ROOT_DIR}/target/scope-reentry-sensitivity/run.XXXXXXXX")"
printf 'Sensitivity evidence: %s\n' "${OUTPUT}"
MUTANT="${OUTPUT}/source"
cd "${ROOT_DIR}"
git rev-parse HEAD > "${OUTPUT}/revision.txt"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
cargo +"${TOOLCHAIN:-stable}" test --locked --test tap_control_actor > "${OUTPUT}/positive.log" 2>&1
git clone --quiet --shared --no-checkout "${ROOT_DIR}" "${MUTANT}"
git ls-files --cached --others --exclude-standard -z |
  perl -0ne 'print "$_\0" for grep { -f $_ } split /\0/' |
  rsync -a --from0 --files-from=- ./ "${MUTANT}/"
git -C "${MUTANT}" add -A
git -C "${MUTANT}" diff --cached --binary HEAD > "${OUTPUT}/current-source.patch"
git -C "${MUTANT}" apply --check "${ROOT_DIR}/proofs/checks/runtime/control-observation/OmitReentry.patch"
git -C "${MUTANT}" apply "${ROOT_DIR}/proofs/checks/runtime/control-observation/OmitReentry.patch"
git -C "${MUTANT}" diff --binary > "${OUTPUT}/mutation.patch"
status=0
(
  cd "${MUTANT}"
  git ls-files -z | xargs -0 shasum -a 256 -- > "${OUTPUT}/mutant-source.sha256"
  cargo +"${TOOLCHAIN:-stable}" test --locked --test tap_control_actor
) > "${OUTPUT}/mutant.log" 2>&1 || status=$?
if [[ "${status}" != 101 ]] || ! grep -Eq '^test result: FAILED\. [0-9]+ passed; 2 failed; 0 ignored; 0 measured; 0 filtered out;' "${OUTPUT}/mutant.log"; then
  echo "Omitting reentry records did not fail both real Endpoint history tests" >&2
  tail -50 "${OUTPUT}/mutant.log" >&2
  exit 1
fi
for name in committed_reentries_distinguish_inner_rolls_from_their_parent delegated_control_records_use_the_holder_actor_and_the_static_controller_role; do
  grep -Fxq "test ${name} ... FAILED" "${OUTPUT}/mutant.log"
done
for name in a_rejected_resolver_still_identifies_its_actor_without_an_endpoint_commit repeated_control_records_identify_the_actor_instead_of_its_lane passive_replies_do_not_claim_a_local_resolver_decision; do
  grep -Fxq "test ${name} ... ok" "${OUTPUT}/mutant.log"
done
for kind in positive mutant; do
  awk '$1 == "test" && ($NF == "ok" || $NF == "FAILED") {print $2}' "${OUTPUT}/${kind}.log" |
    LC_ALL=C sort > "${OUTPUT}/${kind}-tests.txt"
done
cmp "${OUTPUT}/positive-tests.txt" "${OUTPUT}/mutant-tests.txt"
find "${MUTANT}/target" -type f -perm -100 -exec shasum -a 256 -- {} + > "${OUTPUT}/mutant-binaries.sha256"
printf 'mutant_exit=%s\nfailed_tests=2\nmutation=omit_committed_scope_reentries\n' "${status}" > "${OUTPUT}/result.txt"
rm -r -- "${MUTANT}"
echo "Reentry observation sensitivity: current records pass; omitting them fails nested-roll and delegated histories"
