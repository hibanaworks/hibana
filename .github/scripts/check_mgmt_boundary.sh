#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

# Core must not expose the forbidden in-crate management surface.
if rg -n "pub mod mgmt\\b|runtime::mgmt|crate::runtime::mgmt" src/lib.rs src/runtime.rs src/runtime src/runtime.rs; then
  echo "mgmt boundary violation: hibana core must not expose an in-crate mgmt bucket" >&2
  exit 1
fi

# Core must not keep the forbidden EPF bucket either.
if rg -n "mod epf;|pub mod epf\\b|runtime::resolver::epf" src/lib.rs src/runtime.rs src/runtime; then
  echo "mgmt boundary violation: hibana core must not expose an in-crate epf bucket" >&2
  exit 1
fi

# The surviving core resolver surface keeps resolver ownership at the root.
# Replay metadata is an internal TapEvent wire shape, not a public resolver API.
RESOLVER_BLOCK="$(sed -n '/^pub mod resolver {/,/^\/\/\/ Wire payload codec surface\\./p' src/runtime/buckets.rs)"
if printf "%s\n" "${RESOLVER_BLOCK}" | rg -n "pub mod advanced \\{" >/dev/null; then
  echo "mgmt boundary violation: runtime::resolver must not keep an advanced extra bucket" >&2
  exit 1
fi
for required in \
  "ResolverRef"
do
  if ! printf "%s\n" "${RESOLVER_BLOCK}" | rg -n -F "${required}" >/dev/null; then
    echo "mgmt boundary violation: runtime::resolver missing resolver surface: ${required}" >&2
    exit 1
  fi
done
for forbidden in \
  "ResolverContext" \
  "ContextId" \
  "ContextValue" \
  "ResolverAttrs" \
  "ResolverSignals," \
  "ResolverSlot" \
  "pub mod replay {"
do
  if printf "%s\n" "${RESOLVER_BLOCK}" | rg -n -F "${forbidden}" >/dev/null; then
    echo "mgmt boundary violation: runtime::resolver root leaks replay metadata: ${forbidden}" >&2
    exit 1
  fi
done
for forbidden in \
  "ResolverInput" \
  "ResolverAttrs" \
  "ResolverSignals," \
  "ResolverContext" \
  "ContextId" \
  "ContextValue" \
  "pub mod core {" \
  "pub mod replay {" \
  "advanced::resolver"
do
  if rg -n -F "${forbidden}" src/runtime/buckets.rs >/dev/null; then
    echo "mgmt boundary violation: runtime leaks resolver replay internals: ${forbidden}" >&2
    exit 1
  fi
done

echo "mgmt boundary check passed"
