#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"
export TOOLCHAIN="${TOOLCHAIN:-stable}"
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_PROFILE_TEST_DEBUG=line-tables-only
source .github/scripts/repo_rustflags.sh
source .github/scripts/configure_ui_diagnostics.sh
hibana_enable_repo_tests_cfg
hibana_pin_ui_diagnostic_width
trap hibana_restore_ui_diagnostic_width EXIT

# UI diagnostics include source snippets from the selected compiler's core.
# Install this input explicitly rather than depending on a warmed host image.
rustup component add rust-src --toolchain "$TOOLCHAIN"

cargo +"$TOOLCHAIN" fmt --all -- --check
cargo +"$TOOLCHAIN" test --locked --workspace --all-targets -- --test-threads=1
cargo +"$TOOLCHAIN" clippy --locked --workspace --all-targets -- -D warnings
RUSTDOCFLAGS=-Dwarnings cargo +"$TOOLCHAIN" doc --locked -p hibana --no-deps --document-private-items
bash .github/scripts/check_no_std_build.sh
bash .github/scripts/check_lean_proofs.sh --models
bash .github/scripts/check_miri.sh --contract
