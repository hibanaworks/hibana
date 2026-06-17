#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

# Core must not expose the forbidden in-crate management surface.
check_absent "pub mod mgmt\\b|runtime::mgmt|crate::runtime::mgmt" \
  "hibana core must not expose an in-crate mgmt bucket" \
  src/lib.rs src/runtime.rs src/runtime

# Core must not keep the forbidden EPF bucket either.
check_absent "mod epf;|pub mod epf\\b|runtime::resolver::epf" \
  "hibana core must not expose an in-crate epf bucket" \
  src/lib.rs src/runtime.rs src/runtime

# The surviving core resolver surface keeps resolver ownership at the root.
# Replay metadata is an internal TapEvent wire shape, not a public resolver API.
RESOLVER_BLOCK="$(sed -n '/^pub mod resolver {/,/^\/\/\/ Wire payload codec surface\\./p' src/runtime/buckets.rs)"
check_pipe_absent "pub mod advanced \\{" \
  "runtime::resolver must not keep an advanced extra bucket" \
  "${RESOLVER_BLOCK}"
for required in \
  "ResolverRef"
do
  check_pipe_required "${required}" \
    "runtime::resolver missing resolver surface: ${required}" \
    "${RESOLVER_BLOCK}"
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
  check_pipe_absent "${forbidden}" \
    "runtime::resolver root leaks replay metadata: ${forbidden}" \
    "${RESOLVER_BLOCK}" \
    -F
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
  check_absent_literal "${forbidden}" \
    "runtime leaks resolver replay internals: ${forbidden}" \
    src/runtime/buckets.rs
done

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "mgmt boundary check passed"
