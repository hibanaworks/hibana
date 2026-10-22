#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUTPUT="${ROOT_DIR}/target/nested-roll-admission"
mkdir -p "${OUTPUT}" "${ROOT_DIR}/target/lean-proof"
cd "${ROOT_DIR}"
git rev-parse HEAD > "${OUTPUT}/revision.txt"
git diff HEAD --binary > "${OUTPUT}/source.patch"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
source_hashes() {
  git ls-files --cached --others --exclude-standard -z -- src Cargo.toml Cargo.lock \
    tests/nested_roll_admission.rs tests/nested_parallel_reentry.rs \
    tests/data/nested_roll_admission.bin tests/data/nested_roll_scopes.bin tests/common \
    .github/ci-tests/nested_roll_admission.lean .github/scripts/check_nested_roll_admission.sh \
    .github/ci-tests/endpoint_conformance.lean proofs/lean |
    while IFS= read -r -d '' file; do shasum -a 256 -- "$file"; done
}
source_hashes > "${OUTPUT}/source.sha256"
cat .github/ci-tests/endpoint_conformance.lean .github/ci-tests/nested_roll_admission.lean \
  > target/lean-proof/NestedRollAdmissionGenerated.lean
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" build-serial \
  "${ROOT_DIR}/proofs/lean" --native Hibana.SourceOccurrenceTrace
(cd proofs/lean && lake build nestedRollAdmission)
timeout --kill-after=10s 300s proofs/lean/.lake/build/bin/nested-roll-admission \
  "${OUTPUT}/reference.bin" "${OUTPUT}/scoped.bin" | tee "${OUTPUT}/source-model.log"
cmp "${OUTPUT}/reference.bin" tests/data/nested_roll_admission.bin
cmp "${OUTPUT}/scoped.bin" tests/data/nested_roll_scopes.bin
runtime_status=0
if timeout --kill-after=10s 180s cargo +"${TOOLCHAIN:-stable}" test --locked \
    --test nested_parallel_reentry --test nested_roll_admission -- --nocapture > "${OUTPUT}/runtime.log" 2>&1; then
  runtime_status=0
else
  runtime_status=$?
fi
cat "${OUTPUT}/runtime.log"
source_hashes > "${OUTPUT}/source-after.sha256"
cmp "${OUTPUT}/source.sha256" "${OUTPUT}/source-after.sha256"
exit "${runtime_status}"
