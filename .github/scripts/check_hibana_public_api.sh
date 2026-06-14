#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

RUN_SURFACE_TESTS=1
case "${1:-}" in
  "")
    ;;
  "--surface-only")
    RUN_SURFACE_TESTS=0
    ;;
  *)
    echo "usage: $0 [--surface-only]" >&2
    exit 2
    ;;
esac

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

if [[ "${RUN_SURFACE_TESTS}" -eq 1 ]]; then
  run_stable_gate "root surface" \
    cargo +"${TOOLCHAIN}" test -p hibana --test root_surface --features std
  run_stable_gate "runtime surface" \
    cargo +"${TOOLCHAIN}" test -p hibana --test runtime_surface --features std
  run_stable_gate "public surface guards" \
    cargo +"${TOOLCHAIN}" test -p hibana --test public_surface_guards --features std
  run_stable_gate "docs surface" \
    cargo +"${TOOLCHAIN}" test -p hibana --test docs_surface --features std
fi

echo "stable public API check passed"
