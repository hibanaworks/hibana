#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if ! rg -n '^rust-version\s*=\s*"1\.95"' Cargo.toml >/dev/null; then
  echo "Rust 1.95 stable gate violation: Cargo.toml must set rust-version = \"1.95\"" >&2
  exit 1
fi

TOOLCHAIN=1.95.0 ./.github/scripts/ensure_rust_toolchain.sh thumbv6m-none-eabi

cargo +1.95.0 check --no-default-features --lib -p hibana
cargo +1.95.0 check --target thumbv6m-none-eabi --no-default-features --lib -p hibana
cargo +1.95.0 test -p hibana --features std

echo "Rust 1.95 stable check passed"
