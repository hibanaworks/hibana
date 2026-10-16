#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SOURCE="${ROOT_DIR}/.github/ci-tests/check_kani_source.rs"
JSON_SOURCE="${ROOT_DIR}/.github/ci-tests/kani_json.rs"
GATE_SOURCE="${ROOT_DIR}/.github/ci-tests/kani_gate.rs"
RECURSION_SOURCE="${ROOT_DIR}/.github/ci-tests/kani_recursion.rs"
BUILD_DIR="${ROOT_DIR}/target/ci-tools"
BINARY="${BUILD_DIR}/check_kani_source"
if [[ ! -x "${BINARY}" || "${SOURCE}" -nt "${BINARY}" || "${JSON_SOURCE}" -nt "${BINARY}" || "${GATE_SOURCE}" -nt "${BINARY}" || "${RECURSION_SOURCE}" -nt "${BINARY}" ]]; then
  mkdir -p "${BUILD_DIR}"
  temporary="$(mktemp "${BUILD_DIR}/check_kani_source.XXXXXX")"
  trap 'rm -f "${temporary}"' EXIT
  rustc +"${TOOLCHAIN:-1.95.0}" --edition=2024 -D warnings -O "${SOURCE}" -o "${temporary}"
  mv -f "${temporary}" "${BINARY}"
fi
exec "${BINARY}" "$@"
