#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUSTDOC_JSON="${ROOT_DIR}/target/doc/hibana.json"

cd "${ROOT_DIR}"

cargo +nightly rustdoc --lib --features std -- -Z unstable-options --output-format json

if [[ ! -f "${RUSTDOC_JSON}" ]]; then
  echo "missing rustdoc JSON output: ${RUSTDOC_JSON}" >&2
  exit 1
fi

HIBANA_RUSTDOC_JSON="${RUSTDOC_JSON}" \
  cargo +nightly test --test semantic_surface --features std

echo "semantic public API check passed"
