#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
TOOLCHAIN="${TOOLCHAIN:-stable}"
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
tar -tf "${PACKAGE_ARCHIVES[0]}" > "${PACKAGE_WORK}/members.txt"
while IFS= read -r member; do
  case "$member" in
    "$PACKAGE_NAME/tests/"*)
      echo "repository test files must not be shipped in the crate: $member" >&2
      exit 1
      ;;
  esac
done < "${PACKAGE_WORK}/members.txt"
export CARGO_TARGET_DIR="${PACKAGE_TARGET}" RUSTFLAGS=-Dwarnings RUSTDOCFLAGS=-Dwarnings
unset CARGO_ENCODED_RUSTFLAGS
cargo +"${TOOLCHAIN}" check --manifest-path "${PKG_DIR}/Cargo.toml" --lib
cargo +"${TOOLCHAIN}" check --manifest-path "${PKG_DIR}/Cargo.toml" --no-default-features \
  --target thumbv6m-none-eabi --lib
cargo +"${TOOLCHAIN}" test --manifest-path "${PKG_DIR}/Cargo.toml" --examples --no-run
test "$(cargo +"${TOOLCHAIN}" run --quiet --manifest-path "${PKG_DIR}/Cargo.toml" --example ping_pong)" = 'ping=7, pong=8'
test "$(cargo +"${TOOLCHAIN}" run --quiet --manifest-path "${PKG_DIR}/Cargo.toml" --example endpoint_delegation)" = 'worker=8, owner=42, audit=7'
if cargo +"${TOOLCHAIN}" test --manifest-path "${PKG_DIR}/Cargo.toml" \
  --example endpoint_delegation > "${PACKAGE_WORK}/endpoint-tests.log" 2>&1; then
  test_status=0
else
  test_status=$?
fi
cat "${PACKAGE_WORK}/endpoint-tests.log"
bash "${ROOT_DIR}/.github/scripts/lib/test_result_tool.sh" check \
  "${PACKAGE_WORK}/endpoint-tests.log" package-endpoint-delegation "$test_status"
cargo +"${TOOLCHAIN}" doc --manifest-path "${PKG_DIR}/Cargo.toml" --no-deps --no-default-features
