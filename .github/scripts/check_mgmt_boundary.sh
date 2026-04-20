#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

# Core must not expose the old in-crate management surface.
if rg -n "pub mod mgmt\\b|substrate::mgmt|crate::runtime::mgmt" src/lib.rs src/substrate.rs src/runtime.rs; then
  echo "mgmt boundary violation: hibana core must not expose an in-crate mgmt bucket" >&2
  exit 1
fi

# Core must not keep the old EPF bucket either.
if rg -n "mod epf;|pub mod epf\\b|substrate::policy::epf" src/lib.rs src/substrate.rs; then
  echo "mgmt boundary violation: hibana core must not expose an in-crate epf bucket" >&2
  exit 1
fi

# The surviving core policy surface is the generic slot boundary only.
if ! rg -n "pub use crate::policy_runtime::PolicySlot;" src/substrate.rs; then
  echo "mgmt boundary violation: substrate::policy must re-export PolicySlot" >&2
  exit 1
fi

# Core must not remain the public owner for management/policy lifecycle kinds.
if rg -n \
  "pub struct (PolicyLoadKind|PolicyActivateKind|PolicyRevertKind|PolicyAnnotateKind|LoadBeginKind|LoadCommitKind)" \
  src/control/cap/resource_kinds.rs; then
  echo "mgmt boundary violation: lifecycle control kinds must stay internal-only in core" >&2
  exit 1
fi

echo "mgmt boundary check passed"
