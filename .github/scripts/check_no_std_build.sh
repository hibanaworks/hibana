#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
export TOOLCHAIN="${TOOLCHAIN:-stable}"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh" \
  thumbv6m-none-eabi thumbv8m.main-none-eabihf

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

# Profiling must retain the same no_std/no-allocator contract. Cross-compile
# it once here rather than repeating identical target checks on both hosts.
for target in thumbv6m-none-eabi thumbv8m.main-none-eabihf; do
  RUSTFLAGS='--cfg hibana_profile' cargo +"${TOOLCHAIN}" check --locked \
    --lib --no-default-features --target "$target"
done

echo "no_std build gate passed target=thumbv6m-none-eabi projection-example=1"
