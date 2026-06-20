#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/configure_ui_diagnostics.sh"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

run_warning_free() {
  local label="$1"
  shift

  local log
  log="$(mktemp)"
  trap 'rm -f "${log}"' RETURN

  echo "==> ${label}"
  if ! "$@" >"${log}" 2>&1; then
    cat "${log}" >&2
    echo "warning-free gate failed while running: ${label}" >&2
    exit 1
  fi

  if rg -n "warning:" "${log}" >/dev/null; then
    cat "${log}" >&2
    echo "warning-free gate detected warnings in: ${label}" >&2
    exit 1
  fi

  rm -f "${log}"
  trap - RETURN
}

run_warning_free "cargo +${TOOLCHAIN} check --lib -p hibana" \
  cargo +"${TOOLCHAIN}" check --lib -p hibana
run_warning_free "cargo +${TOOLCHAIN} check --manifest-path .github/repo-tests/Cargo.toml --test semantic_surface" \
  cargo +"${TOOLCHAIN}" check --manifest-path "${ROOT_DIR}/.github/repo-tests/Cargo.toml" --test semantic_surface
run_warning_free "cargo +${TOOLCHAIN} check --no-default-features --lib -p hibana" \
  cargo +"${TOOLCHAIN}" check --no-default-features --lib -p hibana
