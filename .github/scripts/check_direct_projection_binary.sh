#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST_PATH="${ROOT_DIR}/Cargo.toml"
export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

timeout 30s cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --test runtime_surface \
  runtime_facade_projects_before_enter \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --test public_surface_guards \
  dynamic_resolver_surface_uses_one_decision_resolver \
  -- \
  --exact \
  --nocapture
