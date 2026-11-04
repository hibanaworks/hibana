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

# Cargo omits unversioned Git dev-dependencies from a published manifest.
# Run the shipped source as an external consumer of the untouched archive,
# selecting the same public runtime revision as the repository manifest.
runtime_source="$(cargo +"${TOOLCHAIN}" metadata --locked --no-deps --format-version 1 |
  jq -er '[.packages[] | select(.name == "hibana") | .dependencies[] |
    select(.name == "hibana-runtime" and .kind == "dev" and .target == null)] |
    if length == 1 then .[0].source else error("expected one runtime dependency") end')"
case "$runtime_source" in
  git+https://github.com/hibanaworks/hibana-runtime\?rev=*) ;;
  *) echo "expected a pinned public Hibana runtime source: $runtime_source" >&2; exit 1 ;;
esac
runtime_url="${runtime_source#git+}"
runtime_revision="${runtime_url#*\?rev=}"
runtime_url="${runtime_url%%\?*}"
if [[ ! "$runtime_revision" =~ ^[0-9a-f]{40}$ ]]; then
  echo "expected a full runtime revision: $runtime_revision" >&2
  exit 1
fi
CONSUMER_DIR="${PACKAGE_WORK}/consumer"
mkdir "$CONSUMER_DIR"
cat > "$CONSUMER_DIR/Cargo.toml" <<EOF
[workspace]

[package]
name = "hibana-package-consumer"
version = "0.0.0"
edition = "2024"

[dependencies]
hibana = { path = "${PKG_DIR}" }
hibana-runtime = { git = "${runtime_url}", rev = "${runtime_revision}" }

[lints.rust]
long_running_const_eval = "forbid"
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(hibana_repo_tests)', 'cfg(hibana_profile)'] }

[[bin]]
name = "ping_pong"
path = "${PKG_DIR}/examples/ping_pong.rs"

[[bin]]
name = "endpoint_delegation"
path = "${PKG_DIR}/examples/endpoint_delegation.rs"
EOF
test "$(cargo +"${TOOLCHAIN}" run --quiet --manifest-path "${CONSUMER_DIR}/Cargo.toml" --bin ping_pong)" = 'ping=7, pong=8'
test "$(cargo +"${TOOLCHAIN}" run --quiet --manifest-path "${CONSUMER_DIR}/Cargo.toml" --bin endpoint_delegation)" = 'worker=8, owner=42, audit=7'
if cargo +"${TOOLCHAIN}" test --manifest-path "${CONSUMER_DIR}/Cargo.toml" --bin endpoint_delegation \
  > "${PACKAGE_WORK}/endpoint-tests.log" 2>&1; then
  test_status=0
else
  test_status=$?
fi
cat "${PACKAGE_WORK}/endpoint-tests.log"
bash "${ROOT_DIR}/.github/scripts/lib/test_result_tool.sh" check \
  "${PACKAGE_WORK}/endpoint-tests.log" package-endpoint-delegation "$test_status"
cargo +"${TOOLCHAIN}" doc --manifest-path "${PKG_DIR}/Cargo.toml" --no-deps --no-default-features
