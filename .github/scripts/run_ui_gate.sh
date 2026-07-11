#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/configure_ui_diagnostics.sh"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_disable_repo_tests_cfg
hibana_pin_ui_diagnostic_width
trap hibana_restore_ui_diagnostic_width EXIT

cargo +"${TOOLCHAIN}" test --test ui "$@"
