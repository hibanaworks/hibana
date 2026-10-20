#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SOURCE="${ROOT_DIR}/.github/ci-tests/compile_pressure.rs"
BUILD_DIR="${ROOT_DIR}/target/ci-tools"
BINARY="${BUILD_DIR}/compile-pressure"
if [[ ! -x "${BINARY}" || "${SOURCE}" -nt "${BINARY}" ]]; then
  mkdir -p "${BUILD_DIR}"
  temporary="$(mktemp "${BUILD_DIR}/compile-pressure.XXXXXX")"
  trap 'rm -f "${temporary}"' EXIT
  rustc +"${TOOLCHAIN:-stable}" --edition=2024 -O "${SOURCE}" -o "${temporary}"
  mv -f "${temporary}" "${BINARY}"
fi
exec "${BINARY}" "$@"
