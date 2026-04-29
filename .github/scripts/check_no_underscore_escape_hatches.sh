#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

NAMED_UNDERSCORE_PATTERN='let[[:space:]]+_[A-Za-z0-9_]+([[:space:]]*:[^=]+)?[[:space:]]*='

if rg -n "${NAMED_UNDERSCORE_PATTERN}" src >/dev/null; then
  echo "underscore escape hatch violation: production source must not hide live values behind named underscores" >&2
  rg -n "${NAMED_UNDERSCORE_PATTERN}" src >&2
  exit 1
fi

if rg -n 'let[[:space:]]+_[[:space:]]*=[[:space:]]*_[A-Za-z][A-Za-z0-9_]*' src tests internal >/dev/null; then
  echo "underscore escape hatch violation: unused named values must be deleted or consumed explicitly" >&2
  rg -n 'let[[:space:]]+_[[:space:]]*=[[:space:]]*_[A-Za-z][A-Za-z0-9_]*' src tests internal >&2
  exit 1
fi

if rg -n "${NAMED_UNDERSCORE_PATTERN}" README.md tests internal >/dev/null; then
  echo "underscore escape hatch violation: docs, tests, and internal fixtures must consume values explicitly" >&2
  rg -n "${NAMED_UNDERSCORE_PATTERN}" README.md tests internal >&2
  exit 1
fi

if rg -n 'let[[:space:]]+_[[:space:]]*=' README.md docs >/dev/null; then
  echo "underscore escape hatch violation: docs must not teach wildcard discards" >&2
  rg -n 'let[[:space:]]+_[[:space:]]*=' README.md docs >&2
  exit 1
fi

if rg -n '^[[:space:]]*_[A-Za-z0-9_]*storage[[:space:]]*:' src/endpoint/kernel/runtime/route_state.rs >/dev/null; then
  echo "underscore escape hatch violation: storage owners must be named and read explicitly" >&2
  rg -n '^[[:space:]]*_[A-Za-z0-9_]*storage[[:space:]]*:' src/endpoint/kernel/runtime/route_state.rs >&2
  exit 1
fi

if rg -n '\b(_legacy|_compat|_fallback|_rescue|_heuristic|_shim|legacy_|compat_|fallback_|rescue_|heuristic_|shim_)\b' \
  src tests internal README.md docs >/dev/null; then
  echo "underscore escape hatch violation: compatibility or fallback escape vocabulary reintroduced" >&2
  rg -n '\b(_legacy|_compat|_fallback|_rescue|_heuristic|_shim|legacy_|compat_|fallback_|rescue_|heuristic_|shim_)\b' \
    src tests internal README.md docs >&2
  exit 1
fi
