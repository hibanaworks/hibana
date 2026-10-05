#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
PACKAGE_WORK="$(mktemp -d "${TMPDIR:-/tmp}/hibana-package.XXXXXX")"
PACKAGE_TARGET="${PACKAGE_WORK}/target"
cleanup_package() {
  local status=$?
  trap - EXIT
  cargo +"${TOOLCHAIN}" clean --target-dir "${PACKAGE_TARGET}" >/dev/null 2>&1 || status=1
  rm -r "${PACKAGE_WORK}" || status=1
  exit "${status}"
}
trap cleanup_package EXIT
CARGO_TARGET_DIR="${PACKAGE_TARGET}" env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS \
  cargo +"${TOOLCHAIN}" package -p hibana --locked --allow-dirty --no-verify

shopt -s nullglob
PACKAGE_ARCHIVES=("${PACKAGE_TARGET}"/package/*.crate)
shopt -u nullglob
if [[ "${#PACKAGE_ARCHIVES[@]}" != 1 ]]; then
  echo "Cargo did not produce one crate archive" >&2
  exit 1
fi
PACKAGE_NAME="${PACKAGE_ARCHIVES[0]##*/}"
PACKAGE_NAME="${PACKAGE_NAME%.crate}"
tar -xf "${PACKAGE_ARCHIVES[0]}" -C "${PACKAGE_WORK}"
PKG_DIR="${PACKAGE_WORK}/${PACKAGE_NAME}"
export CARGO_TARGET_DIR="${PACKAGE_TARGET}" RUSTFLAGS=-Dwarnings RUSTDOCFLAGS=-Dwarnings
unset CARGO_ENCODED_RUSTFLAGS
cargo +"${TOOLCHAIN}" check --manifest-path "${PKG_DIR}/Cargo.toml" --lib
cargo +"${TOOLCHAIN}" check --manifest-path "${PKG_DIR}/Cargo.toml" --no-default-features \
  --target thumbv6m-none-eabi --lib
cargo +"${TOOLCHAIN}" test --manifest-path "${PKG_DIR}/Cargo.toml" --tests --no-run
cargo +"${TOOLCHAIN}" test --manifest-path "${PKG_DIR}/Cargo.toml" --test lane_lifecycle_tap
cargo +"${TOOLCHAIN}" doc --manifest-path "${PKG_DIR}/Cargo.toml" --no-deps --no-default-features
