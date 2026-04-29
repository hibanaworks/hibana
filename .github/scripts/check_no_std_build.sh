#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"
export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh" thumbv6m-none-eabi

if ! rg -q '^#!\[cfg_attr\(not\(feature = "std"\), no_std\)\]' src/lib.rs; then
  echo "missing #![cfg_attr(not(feature = \"std\"), no_std)] in src/lib.rs" >&2
  exit 1
fi

cargo +"${TOOLCHAIN}" check --quiet --no-default-features --lib -p hibana

echo "no_std build gate passed"
