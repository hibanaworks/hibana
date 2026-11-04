#!/usr/bin/env bash
set -euo pipefail

case "$#:${1:-}" in
  0:) selection=all ;;
  1:--tools) selection=tools ;;
  1:--rust) selection=rust ;;
  *) echo 'usage: run_ci_gates.sh [--tools|--rust]' >&2; exit 2 ;;
esac

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"
export TOOLCHAIN="${TOOLCHAIN:-stable}"
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_PROFILE_TEST_DEBUG=line-tables-only
source .github/scripts/repo_rustflags.sh

if [[ "$selection" != rust ]]; then
  cargo +"$TOOLCHAIN" fmt --all -- --check
  cargo +"$TOOLCHAIN" test --locked -p hibana-ci-tests --all-targets -- --test-threads=1
  cargo +"$TOOLCHAIN" clippy --locked -p hibana-ci-tests --all-targets -- -D warnings
fi
if [[ "$selection" == tools ]]; then exit 0; fi
hibana_enable_repo_tests_cfg

source .github/scripts/configure_ui_diagnostics.sh
hibana_pin_ui_diagnostic_width
trap hibana_restore_ui_diagnostic_width EXIT
# UI diagnostics include source snippets from the selected compiler's core.
rustup component add rust-src --toolchain "$TOOLCHAIN"

cargo +"$TOOLCHAIN" test --locked --workspace --exclude hibana-ci-tests --all-targets -- --test-threads=1
cargo +"$TOOLCHAIN" clippy --locked --workspace --exclude hibana-ci-tests --all-targets -- -D warnings
# --all-targets excludes doctests; check the public examples exactly once.
RUSTDOCFLAGS=-Dwarnings cargo +"$TOOLCHAIN" test --locked -p hibana --doc
RUSTDOCFLAGS=-Dwarnings cargo +"$TOOLCHAIN" doc --locked -p hibana --no-deps --document-private-items
bash .github/scripts/check_no_std_build.sh
if [[ "$selection" == all ]]; then
  bash .github/scripts/check_lean_proofs.sh --models
fi
bash .github/scripts/check_miri.sh --contract
