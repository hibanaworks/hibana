#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
source "${ROOT_DIR}/.github/scripts/configure_ui_diagnostics.sh"
hibana_enable_repo_tests_cfg
hibana_pin_ui_diagnostic_width
trap hibana_restore_ui_diagnostic_width EXIT
TOOLCHAIN=1.95.0 bash ./.github/scripts/ensure_rust_toolchain.sh thumbv6m-none-eabi
cargo +1.95.0 check --no-default-features --lib -p hibana
cargo +1.95.0 check --target thumbv6m-none-eabi --no-default-features --lib -p hibana
cargo +1.95.0 test --workspace
echo "Rust 1.95 stable check passed"
