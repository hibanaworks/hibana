#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
export TOOLCHAIN="${TOOLCHAIN:-stable}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
source "${ROOT_DIR}/.github/scripts/configure_ui_diagnostics.sh"
hibana_enable_repo_tests_cfg
hibana_pin_ui_diagnostic_width
trap hibana_restore_ui_diagnostic_width EXIT
bash ./.github/scripts/ensure_rust_toolchain.sh thumbv6m-none-eabi
cargo +"${TOOLCHAIN}" check --no-default-features --lib -p hibana
cargo +"${TOOLCHAIN}" check --target thumbv6m-none-eabi --no-default-features --lib -p hibana
cargo +"${TOOLCHAIN}" test --workspace
echo "Rust stable check passed toolchain=${TOOLCHAIN}"
