#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/configure_ui_diagnostics.sh"
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

run_warning_free "cargo check --all-targets -p hibana" \
  cargo +"${TOOLCHAIN}" check --all-targets -p hibana
run_warning_free "cargo check --no-default-features --lib -p hibana" \
  cargo +"${TOOLCHAIN}" check --no-default-features --lib -p hibana
run_warning_free "cargo test -p hibana --features std" \
  cargo +"${TOOLCHAIN}" test -p hibana --features std
run_warning_free "cargo test -p hibana --test ui --features std" \
  cargo +"${TOOLCHAIN}" test -p hibana --test ui --features std
run_warning_free "cargo test -p hibana --test policy_replay --features std" \
  cargo +"${TOOLCHAIN}" test -p hibana --test policy_replay --features std
