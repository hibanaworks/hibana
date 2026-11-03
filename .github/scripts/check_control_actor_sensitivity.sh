#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
OUTPUT="${ROOT_DIR}/target/control-actor-sensitivity"
MUTANT="${OUTPUT}/source"
mkdir -p "${OUTPUT}"
if [[ -e "${MUTANT}" ]]; then
  echo "Previous sensitivity workspace remains: ${MUTANT}" >&2
  exit 1
fi
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
git -C "${MUTANT}" apply --check "${ROOT_DIR}/proofs/checks/runtime/control-observation/RestoreLane.patch"
git -C "${MUTANT}" apply "${ROOT_DIR}/proofs/checks/runtime/control-observation/RestoreLane.patch"
git -C "${MUTANT}" diff --binary > "${OUTPUT}/mutation.patch"
status=0
(
  cd "${MUTANT}"
  git ls-files -z | xargs -0 shasum -a 256 -- > "${OUTPUT}/mutant-source.sha256"
  cargo +"${TOOLCHAIN:-stable}" test --locked --test tap_control_actor -- \
    --skip committed_reentries_distinguish_inner_rolls_from_their_parent
) > "${OUTPUT}/mutant.log" 2>&1 || status=$?
if [[ "${status}" != 101 ]] || ! grep -Eq '^test result: FAILED\. [0-9]+ passed; 3 failed;' "${OUTPUT}/mutant.log"; then
  echo "Restoring lane-based control records did not fail all three actor observations" >&2
  tail -50 "${OUTPUT}/mutant.log" >&2
  exit 1
fi
for name in \
  a_rejected_resolver_still_identifies_its_actor_without_an_endpoint_commit \
  delegated_control_records_use_the_holder_actor_and_the_static_controller_role \
  repeated_control_records_identify_the_actor_instead_of_its_lane; do
  if ! grep -Fxq "test ${name} ... FAILED" "${OUTPUT}/mutant.log"; then
    echo "Restored lane records did not fail ${name}" >&2
    exit 1
  fi
done
grep -Fxq 'test passive_replies_do_not_claim_a_local_resolver_decision ... ok' "${OUTPUT}/mutant.log"
find "${MUTANT}/target" -type f -perm -100 -exec shasum -a 256 -- {} + > "${OUTPUT}/mutant-binaries.sha256"
printf 'mutant_exit=%s\nfailed_tests=3\nmutation=restore_lane_in_route_and_resolver_records\n' "${status}" > "${OUTPUT}/result.txt"
rm -r -- "${MUTANT}"
echo "Control observation sensitivity: current actor records pass; both restored lane paths fail all three real Endpoint tests"
