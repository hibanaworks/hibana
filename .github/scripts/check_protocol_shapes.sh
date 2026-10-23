#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
DEPTH="${1:-2}"
SHARD="${2:-0}"
SHARDS="${3:-1}"
OUTPUT="${ROOT_DIR}/target/protocol-shapes/depth-${DEPTH}/shard-${SHARD}-of-${SHARDS}"
mkdir -p "${OUTPUT}"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
git -C "${ROOT_DIR}" rev-parse HEAD > "${OUTPUT}/revision.txt"
git -C "${ROOT_DIR}" diff HEAD --binary > "${OUTPUT}/source.patch"
source_hashes() {
  (
    cd "${ROOT_DIR}"
    git ls-files --cached --others --exclude-standard -z -- \
      src tests examples proofs/lean .github Cargo.toml Cargo.lock |
      perl -0ne 'print "$_\0" for grep { -f $_ } split /\0/' |
      xargs -0 shasum -a 256 --
  )
}
source_hashes > "${OUTPUT}/source.sha256"
cargo +"${TOOLCHAIN:-stable}" test --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shapes
cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shapes -- "${ROOT_DIR}" "${OUTPUT}" "${DEPTH}" "${SHARD}" "${SHARDS}" \
  | tee "${OUTPUT}/catalog-inventory.txt"
(
  cd "${OUTPUT}"
  shasum -a 256 src/bin/diagnose.rs > diagnosis-source.sha256
)
/usr/bin/time -p -o "${OUTPUT}/diagnose.time" \
  cargo +"${TOOLCHAIN:-stable}" run --manifest-path "${OUTPUT}/Cargo.toml" --bin diagnose \
    > "${OUTPUT}/diagnostics.tsv"
cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shapes -- "${ROOT_DIR}" "${OUTPUT}" "${DEPTH}" "${SHARD}" "${SHARDS}" \
    "${OUTPUT}/diagnostics.tsv" | tee "${OUTPUT}/inventory.txt"
(
  cd "${OUTPUT}"
  shasum -a 256 --check diagnosis-source.sha256
)
(
  cd "${OUTPUT}"
  shopt -s nullglob
  shasum -a 256 Cargo.toml catalog.json inputs.json scope-inputs.json src/*.rs src/bin/diagnose.rs \
    diagnostics.tsv unclassified-rejections.txt traces/*.txt scope-plans/*.json scripts/*.jsonl \
    rejected/Cargo.toml rejected/inventory.tsv rejected/src/bin/*.rs > inputs.sha256
)
cargo +"${TOOLCHAIN:-stable}" test --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shape-rejections
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
  exit 0
fi
rm -rf "${OUTPUT}/observations" "${OUTPUT}/scope-observations"
/usr/bin/time -p -o "${OUTPUT}/compile.time" \
  cargo +"${TOOLCHAIN:-stable}" test --manifest-path "${OUTPUT}/Cargo.toml" --no-run
(cd "${OUTPUT}" && shasum -a 256 Cargo.lock > dependencies.sha256)
/usr/bin/time -p -o "${OUTPUT}/execute.time" \
  timeout --kill-after=10s "${HIBANA_PROTOCOL_SHAPE_TIMEOUT_SECONDS:-3600}s" \
    cargo +"${TOOLCHAIN:-stable}" test --locked --manifest-path "${OUTPUT}/Cargo.toml" -- --test-threads=1
GENERATED="${ROOT_DIR}/target/lean-proof/ProtocolShapesGenerated.lean"
mkdir -p "$(dirname "${GENERATED}")"
cat "${ROOT_DIR}/.github/ci-tests/endpoint_conformance.lean" \
  "${ROOT_DIR}/.github/ci-tests/protocol_shapes.lean" > "${GENERATED}.new"
if ! cmp -s "${GENERATED}.new" "${GENERATED}"; then
  mv "${GENERATED}.new" "${GENERATED}"
else
  rm "${GENERATED}.new"
fi
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" build-serial \
  "${ROOT_DIR}/proofs/lean" --native Hibana.SourceOccurrenceTrace
(cd "${ROOT_DIR}/proofs/lean" && lake build protocolShapes)
/usr/bin/time -p -o "${OUTPUT}/oracle.time" \
  "${ROOT_DIR}/proofs/lean/.lake/build/bin/protocol-shapes" \
  "${OUTPUT}/inputs.json" "${OUTPUT}/observations" | tee "${OUTPUT}/oracle.log"
cat "${OUTPUT}/diagnose.time" "${OUTPUT}/compile.time" "${OUTPUT}/execute.time" "${OUTPUT}/oracle.time"
cargo +"${TOOLCHAIN:-stable}" test --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shape-controls
cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shape-controls -- "${ROOT_DIR}/proofs/lean/.lake/build/bin/protocol-shapes" "${OUTPUT}"
source_hashes > "${OUTPUT}/source-after.sha256"
cmp "${OUTPUT}/source.sha256" "${OUTPUT}/source-after.sha256"
(cd "${OUTPUT}" && shasum -a 256 --check inputs.sha256 > inputs-check.log)
(cd "${OUTPUT}" && shasum -a 256 --check dependencies.sha256 > dependencies-check.log)
if [[ "$(cat "${OUTPUT}/unclassified-rejections.txt")" != 0 ]]; then
  echo "Unclassified projection rejections remain; see ${OUTPUT}/diagnostics.tsv" >&2
  exit 1
fi
