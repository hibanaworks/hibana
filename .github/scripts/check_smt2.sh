#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOURCE="$ROOT/.github/ci-tests/smt_model.rs"
BUILD="$ROOT/target/ci-tools"
BINARY="$BUILD/smt-model"
if [[ ! -x "$BINARY" || "$SOURCE" -nt "$BINARY" ]]; then
  mkdir -p "$BUILD"
  temporary="$(mktemp "$BUILD/smt-model.XXXXXX")"
  trap 'rm -f "$temporary"' EXIT
  rustc +"${TOOLCHAIN:-stable}" --edition=2024 -O "$SOURCE" -o "$temporary"
  mv -f "$temporary" "$BINARY"
fi
exec "$BINARY" "$@"
