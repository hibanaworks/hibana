#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEPTH="${1:-2}"
SHARD="${2:-0}"
SHARDS="${3:-1}"
OUTPUT="${ROOT_DIR}/target/protocol-shapes/depth-${DEPTH}/shard-${SHARD}-of-${SHARDS}"
mkdir -p "${OUTPUT}"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
git -C "${ROOT_DIR}" rev-parse HEAD > "${OUTPUT}/revision.txt"
git -C "${ROOT_DIR}" diff --binary > "${OUTPUT}/source.patch"
cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shapes -- "${ROOT_DIR}" "${OUTPUT}" "${DEPTH}" "${SHARD}" "${SHARDS}" \
  | tee "${OUTPUT}/inventory.txt"
rm -rf "${OUTPUT}/observations"
/usr/bin/time -p -o "${OUTPUT}/compile.time" \
  cargo +"${TOOLCHAIN:-stable}" test --manifest-path "${OUTPUT}/Cargo.toml" --no-run
/usr/bin/time -p -o "${OUTPUT}/execute.time" \
  cargo +"${TOOLCHAIN:-stable}" test --manifest-path "${OUTPUT}/Cargo.toml" -- --test-threads=1
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
cat "${OUTPUT}/compile.time" "${OUTPUT}/execute.time" "${OUTPUT}/oracle.time"
cargo +"${TOOLCHAIN:-stable}" run --locked --manifest-path "${ROOT_DIR}/.github/ci-tests/Cargo.toml" \
  --bin protocol-shape-controls -- "${ROOT_DIR}/proofs/lean/.lake/build/bin/protocol-shapes" "${OUTPUT}"
