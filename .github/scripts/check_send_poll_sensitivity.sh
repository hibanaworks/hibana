#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
OUTPUT="${ROOT_DIR}/target/send-poll-sensitivity"
mkdir -p "${OUTPUT}"
cd "${ROOT_DIR}"
git rev-parse HEAD > "${OUTPUT}/revision.txt"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
cargo +"${TOOLCHAIN:-stable}" test --locked --test tap_send_poll > "${OUTPUT}/positive.log" 2>&1
: > "${OUTPUT}/results.tsv"
for defect in omit completion-only; do
  mutant="${OUTPUT}/${defect}"
  if [[ -e "${mutant}" ]]; then
    echo "Previous sensitivity workspace remains: ${mutant}" >&2
    exit 1
  fi
  git clone --quiet --shared --no-checkout "${ROOT_DIR}" "${mutant}"
  git ls-files --cached --others --exclude-standard -z |
    perl -0ne 'print "$_\0" for grep { -f $_ } split /\0/' |
    rsync -a --from0 --files-from=- ./ "${mutant}/"
  git -C "${mutant}" add -A
  git -C "${mutant}" diff --cached --binary HEAD > "${OUTPUT}/${defect}-baseline.patch"
  if [[ "${defect}" == omit ]]; then
    perl -0pi -e '
      s/    crate::observe::core::emit\(port\.tap\(\), observation\);\n// == 1 or die "missing send-poll observation\n";
    ' "${mutant}/src/endpoint/kernel/lane_port.rs"
  else
    perl -0pi -e '
      s/    crate::observe::core::emit\(port\.tap\(\), observation\);\n// == 1 or die "missing send-poll observation\n";
      s/(        Poll::Ready\(result\) => \{\n)(            pending\.state = SendIoState::Unpolled;)/$1            crate::observe::core::emit(port.tap(), observation);\n$2/ == 1 or die "missing completed send poll\n";
    ' "${mutant}/src/endpoint/kernel/lane_port.rs"
  fi
  git -C "${mutant}" diff --binary > "${OUTPUT}/${defect}.patch"
  status=0
  (
    cd "${mutant}"
    git ls-files -z | xargs -0 shasum -a 256 -- > "${OUTPUT}/${defect}-source.sha256"
    cargo +"${TOOLCHAIN:-stable}" test --locked --test tap_send_poll
  ) > "${OUTPUT}/${defect}.log" 2>&1 || status=$?
  if [[ "${status}" != 101 ]] || ! grep -Eq '^test result: FAILED\. [0-9]+ passed; 2 failed; 0 ignored; 0 measured; 0 filtered out;' "${OUTPUT}/${defect}.log"; then
    echo "The ${defect} mutation did not fail both pending wire histories" >&2
    tail -40 "${OUTPUT}/${defect}.log" >&2
    exit 1
  fi
  for name in each_checked_wire_poll_precedes_frame_delivery_and_keeps_its_prepared_scope a_received_resource_can_be_used_before_the_loan_sender_publishes; do
    grep -Fxq "test ${name} ... FAILED" "${OUTPUT}/${defect}.log"
  done
  for name in an_out_of_order_send_never_reaches_a_transport_poll_observation local_unit_operations_do_not_invent_transport_polls; do
    grep -Fxq "test ${name} ... ok" "${OUTPUT}/${defect}.log"
  done
  for kind in positive "${defect}"; do
    awk '$1 == "test" && ($NF == "ok" || $NF == "FAILED") {print $2}' "${OUTPUT}/${kind}.log" |
      LC_ALL=C sort > "${OUTPUT}/${kind}-tests.txt"
  done
  cmp "${OUTPUT}/positive-tests.txt" "${OUTPUT}/${defect}-tests.txt"
  find "${mutant}/target" -type f -perm -100 -exec shasum -a 256 -- {} + > "${OUTPUT}/${defect}-binaries.sha256"
  printf '%s\t%s\t2\n' "${defect}" "${status}" >> "${OUTPUT}/results.tsv"
  rm -r -- "${mutant}"
done
echo 'Send-poll observation sensitivity: missing and completion-only records fail both pending histories'
