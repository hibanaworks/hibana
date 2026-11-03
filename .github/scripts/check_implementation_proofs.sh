#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/hibana-implementation-proofs.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
rustc +"${TOOLCHAIN:-stable}" --edition=2024 -D warnings -O \
  "$ROOT/.github/ci-tests/implementation_proofs.rs" -o "$scratch/run"
"$scratch/run" "$ROOT" "$@"
