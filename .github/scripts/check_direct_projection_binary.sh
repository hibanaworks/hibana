#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST_PATH="${ROOT_DIR}/Cargo.toml"
export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --test substrate_surface \
  --features std \
  substrate_facade_projects_before_enter \
  -- \
  --exact \
  --nocapture

TEST_BINARY="$(
  cargo +"${TOOLCHAIN}" test \
    --manifest-path "${MANIFEST_PATH}" \
    --test substrate_surface \
    --features std \
    --no-run \
    --message-format=json |
    awk -F'"' '
      /"reason":"compiler-artifact"/ && /"name":"substrate_surface"/ && /"executable":"/ {
        for (i = 1; i <= NF; i++) {
          if ($i == "executable") {
            print $(i + 2)
          }
        }
      }
    ' | tail -n 1
)"

if [[ -z "${TEST_BINARY}" ]]; then
  echo "failed to locate substrate_surface test binary" >&2
  exit 1
fi

timeout 30s "${TEST_BINARY}" substrate_facade_projects_before_enter --exact --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --test public_surface_guards \
  --features std \
  route_projection_regression_fixtures_keep_canonical_inputs_live \
  -- \
  --exact \
  --nocapture

cargo +"${TOOLCHAIN}" test \
  --manifest-path "${MANIFEST_PATH}" \
  --test public_surface_guards \
  --features std \
  ui_diagnostics_stay_semantic \
  -- \
  --exact \
  --nocapture
