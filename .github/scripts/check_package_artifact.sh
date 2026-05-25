#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

TOOLCHAIN="${TOOLCHAIN:-1.95.0}"

PACKAGE_LIST="$(cargo +"${TOOLCHAIN}" package --list --allow-dirty)"

if rg -n 'tests/support/' src; then
  echo "package artifact check failed: src must not depend on tests/support fixtures" >&2
  exit 1
fi

if grep -qE '^src/(test_support|endpoint/kernel/test_support)/|^src/.*/tests\.rs$|^src/.*_tests\.rs$' <<<"${PACKAGE_LIST}"; then
  echo "package artifact check failed: source-tree test fixtures must not ship in the production crate package" >&2
  grep -E '^src/(test_support|endpoint/kernel/test_support)/|^src/.*/tests\.rs$|^src/.*_tests\.rs$' <<<"${PACKAGE_LIST}" >&2
  exit 1
fi

cargo +"${TOOLCHAIN}" package --allow-dirty

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

CRATE_FILE="$(ls -t target/package/hibana-*.crate | head -n 1)"
tar -xf "${CRATE_FILE}" -C "${TMP_DIR}"
PKG_DIR="$(find "${TMP_DIR}" -maxdepth 1 -type d -name 'hibana-*' | head -n 1)"

cargo +"${TOOLCHAIN}" check --manifest-path "${PKG_DIR}/Cargo.toml" --features std --lib
cargo +"${TOOLCHAIN}" check --manifest-path "${PKG_DIR}/Cargo.toml" --no-default-features --lib
cargo +"${TOOLCHAIN}" doc --manifest-path "${PKG_DIR}/Cargo.toml" --no-deps --no-default-features
