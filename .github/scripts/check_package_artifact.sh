#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

TOOLCHAIN="${TOOLCHAIN:-1.95.0}"

PACKAGE_LIST="$(cargo +"${TOOLCHAIN}" package --list --allow-dirty)"
mapfile -t REQUIRED_FIXTURES < <(
  rg --no-filename -No '"/tests/support/[^"]+"' src \
    | tr -d '"' \
    | sed 's#^/##' \
    | sort -u
)

for required in "${REQUIRED_FIXTURES[@]}"; do
  if ! grep -qx "${required}" <<<"${PACKAGE_LIST}"; then
    echo "package artifact missing required source unit-test fixture: ${required}" >&2
    exit 1
  fi
done

cargo +"${TOOLCHAIN}" package --allow-dirty

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

CRATE_FILE="$(ls -t target/package/hibana-*.crate | head -n 1)"
tar -xf "${CRATE_FILE}" -C "${TMP_DIR}"
PKG_DIR="$(find "${TMP_DIR}" -maxdepth 1 -type d -name 'hibana-*' | head -n 1)"

cargo +"${TOOLCHAIN}" test --manifest-path "${PKG_DIR}/Cargo.toml" --features std --lib
cargo +"${TOOLCHAIN}" check --manifest-path "${PKG_DIR}/Cargo.toml" --no-default-features --lib
cargo +"${TOOLCHAIN}" doc --manifest-path "${PKG_DIR}/Cargo.toml" --no-deps --no-default-features
