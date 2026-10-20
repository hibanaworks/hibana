#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SOURCE="$ROOT/.github/ci-tests/final_measurements.rs"
SHARED="$ROOT/.github/ci-tests/kani_json.rs"
BUILD="$ROOT/target/ci-tools"
BINARY="$BUILD/final-measurements"
if [[ ! -x "$BINARY" || "$SOURCE" -nt "$BINARY" || "$SHARED" -nt "$BINARY" ]]; then
  mkdir -p "$BUILD"
  temporary="$(mktemp "$BUILD/final-measurements.XXXXXX")"
  trap 'rm -f "$temporary"' EXIT
  rustc +"${TOOLCHAIN:-stable}" --edition=2024 -O "$SOURCE" -o "$temporary"
  mv -f "$temporary" "$BINARY"
fi
exec "$BINARY" "$@"
