#!/usr/bin/env bash
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/hibana-elastic-runner.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
rustc +"${TOOLCHAIN:-1.95.0}" --edition=2024 -O "$REPO/.github/ci-tests/elastic_proof_runner.rs" -o "$scratch/run"
"$scratch/run" "$REPO" "$@"
