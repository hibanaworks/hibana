#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SOURCE="$ROOT/.github/ci-tests/pressure_source.rs"
BUILD="$ROOT/target/ci-tools"
BINARY="$BUILD/pressure-source"
rebuild=0
for source in "$SOURCE" "$ROOT/.github/ci-tests/pressure_templates.rs"; do
  if [[ ! -x "$BINARY" || "$source" -nt "$BINARY" ]]; then rebuild=1; fi
done
if [[ "$rebuild" == 1 ]]; then
  mkdir -p "$BUILD"
  temporary="$(mktemp "$BUILD/pressure-source.XXXXXX")"
  trap 'rm -f "$temporary"' EXIT
  rustc +"${TOOLCHAIN:-stable}" --edition=2024 -O "$SOURCE" -o "$temporary"
  mv -f "$temporary" "$BINARY"
fi
exec "$BINARY" "$@"
