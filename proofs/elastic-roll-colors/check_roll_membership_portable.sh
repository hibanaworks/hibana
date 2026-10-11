#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/hibana-membership.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
rustc +"${TOOLCHAIN:-1.95.0}" --edition=2024 -O "$REPO/.github/ci-tests/check_roll_membership.rs" -o "$scratch/check"
"$scratch/check" "$HERE" "$@"
