#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

# Core must not expose the old in-crate management surface.
if rg -n "pub mod mgmt\\b|integration::mgmt|crate::runtime::mgmt" src/lib.rs src/integration.rs src/integration src/runtime.rs; then
  echo "mgmt boundary violation: hibana core must not expose an in-crate mgmt bucket" >&2
  exit 1
fi

# Core must not keep the old EPF bucket either.
if rg -n "mod epf;|pub mod epf\\b|integration::resolver::epf" src/lib.rs src/integration.rs src/integration; then
  echo "mgmt boundary violation: hibana core must not expose an in-crate epf bucket" >&2
  exit 1
fi

# The surviving core policy surface keeps resolver ownership at the root.
# Replay metadata is an internal TapEvent wire shape, not a public policy API.
RESOLVER_BLOCK="$(sed -n '/^pub mod resolver {/,/^\/\/\/ Canonical capability-token surface/p' src/integration/buckets.rs)"
if printf "%s\n" "${RESOLVER_BLOCK}" | rg -n "pub mod advanced \\{" >/dev/null; then
  echo "mgmt boundary violation: integration::resolver must not keep an advanced compatibility bucket" >&2
  exit 1
fi
for required in \
  "ResolverRef"
do
  if ! printf "%s\n" "${RESOLVER_BLOCK}" | rg -n -F "${required}" >/dev/null; then
    echo "mgmt boundary violation: integration::resolver missing resolver surface: ${required}" >&2
    exit 1
  fi
done
for forbidden in \
  "ResolverContext" \
  "ContextId" \
  "ContextValue" \
  "PolicyAttrs" \
  "PolicySignals," \
  "PolicySlot" \
  "pub mod replay {"
do
  if printf "%s\n" "${RESOLVER_BLOCK}" | rg -n -F "${forbidden}" >/dev/null; then
    echo "mgmt boundary violation: integration::resolver root leaks replay metadata: ${forbidden}" >&2
    exit 1
  fi
done
for forbidden in \
  "PolicyInput" \
  "PolicyAttrs" \
  "PolicySignals," \
  "ResolverContext" \
  "ContextId" \
  "ContextValue" \
  "pub mod core {" \
  "pub mod replay {" \
  "advanced::policy"
do
  if rg -n -F "${forbidden}" src/integration/buckets.rs >/dev/null; then
    echo "mgmt boundary violation: integration leaks resolver replay internals: ${forbidden}" >&2
    exit 1
  fi
done

# Core must not remain the public owner for management/policy lifecycle kinds.
if rg -n \
  "pub struct (PolicyLoadKind|PolicyActivateKind|PolicyRevertKind|PolicyRestoreKind|PolicyAnnotateKind|LoadBeginKind|LoadCommitKind)" \
  src/control/cap/resource_kinds.rs; then
  echo "mgmt boundary violation: lifecycle control kinds must stay internal-only in core" >&2
  exit 1
fi

echo "mgmt boundary check passed"
