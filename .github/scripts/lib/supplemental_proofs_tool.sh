#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
SOURCE="$ROOT/.github/ci-tests/supplemental_proofs.rs"
SHARED="$ROOT/.github/ci-tests/lean_audit.rs"
BUILD="$ROOT/target/ci-tools"
BINARY="$BUILD/supplemental-proofs"
if [[ ! -x "$BINARY" || "$SOURCE" -nt "$BINARY" || "$SHARED" -nt "$BINARY" ]]; then
  mkdir -p "$BUILD"
  temporary="$(mktemp "$BUILD/supplemental-proofs.XXXXXX")"
  trap 'rm -f "$temporary"' EXIT
  rustc +"${TOOLCHAIN:-1.95.0}" --edition=2024 -O "$SOURCE" -o "$temporary"
  mv -f "$temporary" "$BINARY"
fi
exec "$BINARY" "$@"
