#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

LOCAL_ONLY=0
case "${1:-}" in
  "")
    ;;
  "--local-only")
    LOCAL_ONLY=1
    ;;
  *)
    echo "usage: $0 [--local-only]" >&2
    exit 2
    ;;
esac

check_absent \
  "MAX_COMPILED_PROGRAMS|MAX_COMPILED_ROLES|compiled_programs|compiled_roles|CompiledCacheLease|ProgramCacheEntry|RoleCacheEntry|acquire_compiled_cache|with_pinned_compiled_program|with_pinned_compiled_role|release_compiled_cache_lease" \
  "runtime compiled cache owner" \
  src/session/cluster/core.rs src/endpoint/kernel/core.rs
if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

if [[ "${LOCAL_ONLY}" -eq 1 ]]; then
  echo "boundary contract local check passed"
  exit 0
fi

bash "${ROOT_DIR}/.github/scripts/check_mgmt_boundary.sh"
bash "${ROOT_DIR}/.github/scripts/check_plane_boundaries.sh"
bash "${ROOT_DIR}/.github/scripts/check_resolver_context_surface.sh"
bash "${ROOT_DIR}/.github/scripts/check_lowering_hygiene.sh"
bash "${ROOT_DIR}/.github/scripts/check_surface_hygiene.sh"
