#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/hibana-elastic-tests.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
rustc +"${TOOLCHAIN:-1.95.0}" --edition=2024 --test "$ROOT/.github/ci-tests/check_roll_membership.rs" -o "$scratch/tests"
"$scratch/tests" "$@"
