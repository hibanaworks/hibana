#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-stable}"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

run_stable_gate() {
  local label="$1"
  shift
  echo "==> ${label}"
  "$@"
}

run_stable_gate "public surface budget" \
  bash "${ROOT_DIR}/.github/scripts/check_public_surface_budget.sh"
run_stable_gate "surface hygiene" \
  bash "${ROOT_DIR}/.github/scripts/check_surface_hygiene.sh"
run_stable_gate "root surface" \
  cargo +"${TOOLCHAIN}" test -p hibana --test root_surface --features std
run_stable_gate "substrate surface" \
  cargo +"${TOOLCHAIN}" test -p hibana --test substrate_surface --features std
run_stable_gate "public surface guards" \
  cargo +"${TOOLCHAIN}" test -p hibana --test public_surface_guards --features std
run_stable_gate "docs surface" \
  cargo +"${TOOLCHAIN}" test -p hibana --test docs_surface --features std

echo "stable public API check passed"
