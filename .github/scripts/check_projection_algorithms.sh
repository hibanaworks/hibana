#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/hibana-projection-checks.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
rustc +"${TOOLCHAIN:-stable}" --edition=2024 -D warnings -O \
  "$ROOT/.github/ci-tests/projection_checks.rs" -o "$scratch/run"
"$scratch/run" "$ROOT" "$@"
