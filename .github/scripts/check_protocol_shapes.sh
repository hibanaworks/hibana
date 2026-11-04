#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
admission_controls=false
check=false
if [[ "${1:-}" == --check ]]; then
  check=true
  shift
fi
if [[ "${1:-}" == --admission-controls ]]; then
  admission_controls=true
  shift
fi
if [[ "${1:-}" == --contracts ]]; then
  if [[ $# != 3 ]]; then
    echo "usage: $0 --contracts FIRST COUNT" >&2
    exit 2
  fi
  GENERATOR_ARGS=(--contracts "$2" "$3")
  OUTPUT="${ROOT_DIR}/target/protocol-shapes/contracts/range-$2-count-$3"
elif [[ "${1:-}" == --frontiers ]]; then
  if [[ $# != 3 ]]; then
    echo "usage: $0 --frontiers FIRST COUNT" >&2
    exit 2
  fi
  GENERATOR_ARGS=(--frontiers "$2" "$3")
  OUTPUT="${ROOT_DIR}/target/protocol-shapes/frontiers/range-$2-count-$3"
elif [[ "${1:-}" == --representatives ]]; then
  if [[ $# != 4 ]]; then
    echo "usage: $0 --representatives DEPTH FIRST COUNT" >&2
    exit 2
  fi
  GENERATOR_ARGS=(--representatives "$2" "$3" "$4")
  OUTPUT="${ROOT_DIR}/target/protocol-shapes/source-representatives-$2/range-$3-count-$4"
elif [[ "${1:-}" == --interactions || "${1:-}" == --spines ]]; then
  if [[ $# != 4 ]]; then
    echo "usage: $0 --interactions LENGTH FIRST COUNT" >&2
    exit 2
  fi
  GENERATOR_ARGS=("$1" "$2" "$3" "$4")
  OUTPUT="${ROOT_DIR}/target/protocol-shapes/${1#--}-$2/range-$3-count-$4"
elif [[ "${1:-}" == --syntax-depth ]]; then
  if [[ $# != 4 ]]; then
    echo "usage: $0 --syntax-depth DEPTH FIRST COUNT" >&2
    exit 2
  fi
  GENERATOR_ARGS=(--syntax-depth "$2" "$3" "$4")
  SYNTAX_OUTPUT="$(cargo +"${TOOLCHAIN:-stable}" run --quiet --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
    --bin protocol-shapes -- syntax-output "$2" "$3" "$4")"
  OUTPUT="${ROOT_DIR}/target/protocol-shapes/${SYNTAX_OUTPUT}"
else
  BUDGET="${1:-2}"
  SHARD="${2:-0}"
  SHARDS="${3:-1}"
  GENERATOR_ARGS=("${BUDGET}" "${SHARD}" "${SHARDS}")
  OUTPUT="${ROOT_DIR}/target/protocol-shapes/operator-budget-${BUDGET}/shard-${SHARD}-of-${SHARDS}"
fi
if [[ -n "${HIBANA_PROTOCOL_SHAPE_OUTPUT_ROOT:-}" ]]; then
  if [[ "${HIBANA_PROTOCOL_SHAPE_OUTPUT_ROOT}" != /* ]]; then
    echo 'HIBANA_PROTOCOL_SHAPE_OUTPUT_ROOT must be an absolute path' >&2
    exit 2
  fi
  OUTPUT="${HIBANA_PROTOCOL_SHAPE_OUTPUT_ROOT}/${OUTPUT#"${ROOT_DIR}/target/protocol-shapes/"}"
fi
source_hashes() {
  (
    cd "${ROOT_DIR}"
    git ls-files --cached --others --exclude-standard -z |
      perl -0ne 'print "$_\0" for grep { -f $_ } split /\0/' |
      xargs -0 shasum -a 256 --
  )
}
if [[ "$check" == true ]]; then
  temporary="$(mktemp)"
  trap 'rm -f -- "$temporary"' EXIT
  source_hashes > "$temporary"
  cmp "$temporary" "${OUTPUT}/source.sha256"
  git -C "${ROOT_DIR}" rev-parse HEAD > "$temporary"
  cmp "$temporary" "${OUTPUT}/revision.txt"
  (
    cd "${OUTPUT}"
    shasum -a 256 --check passed.sha256
    shasum -a 256 --check inputs.sha256
    shasum -a 256 --check dependencies.sha256
  )
  exit 0
fi
mkdir -p "$(dirname "${OUTPUT}")"
mkdir "${OUTPUT}"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
git -C "${ROOT_DIR}" rev-parse HEAD > "${OUTPUT}/revision.txt"
git -C "${ROOT_DIR}" diff HEAD --binary > "${OUTPUT}/source.patch"
source_hashes > "${OUTPUT}/source.sha256"
# Generator unit tests run once in the required workspace test gate. Each
# population still runs its complete compiler, Endpoint and oracle checks here.
cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shapes -- "${ROOT_DIR}" "${OUTPUT}" "${GENERATOR_ARGS[@]}" \
  | tee "${OUTPUT}/catalog-inventory.txt"
(
  cd "${OUTPUT}"
  shasum -a 256 src/bin/diagnose.rs > diagnosis-source.sha256
)
/usr/bin/time -p -o "${OUTPUT}/diagnose.time" \
  cargo +"${TOOLCHAIN:-stable}" run --manifest-path "${OUTPUT}/Cargo.toml" --target-dir "${OUTPUT}/target" --bin diagnose \
    > "${OUTPUT}/diagnostics.tsv"
cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shapes -- "${ROOT_DIR}" "${OUTPUT}" "${GENERATOR_ARGS[@]}" \
    "${OUTPUT}/diagnostics.tsv" | tee "${OUTPUT}/inventory.txt"
(
  cd "${OUTPUT}"
  shasum -a 256 --check diagnosis-source.sha256
)
(
  cd "${OUTPUT}"
  shopt -s nullglob
  shasum -a 256 Cargo.toml catalog.json catalog-scope.txt case-identities.tsv contract-assignments.tsv case-depths.tsv source-coverage*.tsv inputs.json scope-inputs.json src/*.rs src/bin/diagnose.rs \
    diagnostics.tsv unclassified-rejections.txt traces/*.txt scope-plans/*.jsonl scripts/*.jsonl \
    rejected/Cargo.toml rejected/inventory.tsv rejected/src/bin/*.rs > inputs.sha256
)
cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shape-rejections -- "${OUTPUT}" "${TOOLCHAIN:-stable}"
if [[ "$(cat "${OUTPUT}/inputs.json")" == '[]' ]]; then
  source_hashes > "${OUTPUT}/source-after.sha256"
  cmp "${OUTPUT}/source.sha256" "${OUTPUT}/source-after.sha256"
  (cd "${OUTPUT}" && shasum -a 256 --check inputs.sha256 > inputs-check.log)
  if [[ "$(cat "${OUTPUT}/unclassified-rejections.txt")" != 0 ]]; then
    echo "Unclassified projection rejections remain; see ${OUTPUT}/diagnostics.tsv" >&2
    exit 1
  fi
  echo "This shard contains expected compile-fail cases only; no runtime coverage is claimed."
  if [[ -d "${OUTPUT}/rejected/target" ]]; then rm -r -- "${OUTPUT}/rejected/target"; fi
  exit 0
fi
rm -rf "${OUTPUT}/observations" "${OUTPUT}/scope-observations"
/usr/bin/time -p -o "${OUTPUT}/compile.time" \
  cargo +"${TOOLCHAIN:-stable}" test --manifest-path "${OUTPUT}/Cargo.toml" --target-dir "${OUTPUT}/target" --no-run \
    --message-format=json > "${OUTPUT}/test-binaries.jsonl"
(cd "${OUTPUT}" && shasum -a 256 Cargo.lock > dependencies.sha256)
/usr/bin/time -p -o "${OUTPUT}/execute.time" \
  cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
    --bin protocol-shape-runs -- "${OUTPUT}"
CHECKER="$(bash "${ROOT_DIR}/.github/scripts/build_protocol_checker.sh")"
/usr/bin/time -p -o "${OUTPUT}/oracle.time" \
  "${CHECKER}" \
  "${OUTPUT}/inputs.json" "${OUTPUT}/observations" | tee "${OUTPUT}/oracle.log"
cat "${OUTPUT}/diagnose.time" "${OUTPUT}/compile.time" "${OUTPUT}/execute.time" "${OUTPUT}/oracle.time"
cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shape-controls -- "${CHECKER}" "${OUTPUT}" | tee "${OUTPUT}/controls.log"
if [[ "$admission_controls" == true ]]; then
  cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
    --bin protocol-shape-controls -- --admission-controls "${CHECKER}" "${OUTPUT}" \
    | tee "${OUTPUT}/admission-controls.log"
fi
source_hashes > "${OUTPUT}/source-after.sha256"
cmp "${OUTPUT}/source.sha256" "${OUTPUT}/source-after.sha256"
(cd "${OUTPUT}" && shasum -a 256 --check inputs.sha256 > inputs-check.log)
(cd "${OUTPUT}" && shasum -a 256 --check dependencies.sha256 > dependencies-check.log)
if [[ "$(cat "${OUTPUT}/unclassified-rejections.txt")" != 0 ]]; then
  echo "Unclassified projection rejections remain; see ${OUTPUT}/diagnostics.tsv" >&2
  exit 1
fi
cp "${OUTPUT}/source-coverage.tsv" "${OUTPUT}/executed-coverage.tsv"
cp "${OUTPUT}/source-coverage-counts.tsv" "${OUTPUT}/executed-coverage-counts.tsv"
# Admission arrays and partition copies have been consumed by the source
# checker and its controls. Keep their hashes and all completed execution
# records; failed runs never reach this retirement boundary.
(
  cd "${OUTPUT}"
  temporary=(observations controls)
  if [[ -d admission-controls ]]; then temporary+=(admission-controls); fi
  find "${temporary[@]}" -type f -print0 | xargs -0 shasum -a 256 -- \
    > admission-observations.sha256
)
rm -r -- "${OUTPUT}/observations" "${OUTPUT}/scope-parts"
for controls in controls admission-controls; do
  if [[ -d "${OUTPUT}/$controls" ]]; then
    while IFS= read -r -d '' directory; do
      rm -r -- "$directory"
    done < <(find "${OUTPUT}/$controls" -type d \( -name observations -o -name scope-observations -o -name scripts \) -prune -print0)
  fi
done
# The sources, compiler transcript, binary hashes, completed execution records
# and control rejection logs remain; private Cargo targets are no longer used.
rm -r -- "${OUTPUT}/target"
if [[ -d "${OUTPUT}/rejected/target" ]]; then rm -r -- "${OUTPUT}/rejected/target"; fi
(
  cd "${OUTPUT}"
  shopt -s nullglob
  files=(revision.txt compiler.txt source.patch source.sha256 source-after.sha256
    inputs.sha256 dependencies.sha256 oracle.log controls.log admission-observations.sha256)
  if [[ -f admission-controls.log ]]; then files+=(admission-controls.log); fi
  shasum -a 256 "${files[@]}" scope-observations/*.jsonl controls/*/rejection.log \
    admission-controls/*/rejection.log > passed.sha256
)
