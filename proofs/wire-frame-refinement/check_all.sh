#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/hibana-wire-runner.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
rustc +"${TOOLCHAIN:-1.95.0}" --edition=2024 -O "$ROOT/.github/ci-tests/wire_proof_runner.rs" -o "$scratch/run"
"$scratch/run" "$ROOT" "$@"
