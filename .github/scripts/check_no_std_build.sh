#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh" thumbv6m-none-eabi

cargo +"${TOOLCHAIN}" check \
  --quiet \
  --locked \
  --no-default-features \
  --lib \
  -p hibana \
  --target thumbv6m-none-eabi

cargo +"${TOOLCHAIN}" check \
  --quiet \
  --manifest-path examples/pico/Cargo.toml \
  --no-default-features \
  --lib \
  --target thumbv6m-none-eabi

echo "no_std build gate passed target=thumbv6m-none-eabi projection-example=1"
