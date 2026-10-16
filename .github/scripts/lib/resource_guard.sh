#!/usr/bin/env bash
set -euo pipefail
RESOURCE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
RESOURCE_SOURCE="$RESOURCE_ROOT/.github/ci-tests/resource_guard.rs"
RESOURCE_BINARY="$RESOURCE_ROOT/target/ci-tools/resource-guard"
if [[ ! -x "$RESOURCE_BINARY" || "$RESOURCE_SOURCE" -nt "$RESOURCE_BINARY" ]]; then
  mkdir -p "$(dirname "$RESOURCE_BINARY")"
  resource_temporary="$(mktemp "$RESOURCE_BINARY.XXXXXX")"
  trap 'rm -f "$resource_temporary"' EXIT
  rustc +1.95.0 --edition=2024 -D warnings -O -C codegen-units=1 \
    "$RESOURCE_SOURCE" -o "$resource_temporary"
  mv -f "$resource_temporary" "$RESOURCE_BINARY"
fi
exec "$RESOURCE_BINARY" -- "$@"
