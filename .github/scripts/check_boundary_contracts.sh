#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

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

if rg -n "MAX_COMPILED_PROGRAMS|MAX_COMPILED_ROLES|compiled_programs|compiled_roles|CompiledCacheLease|ProgramCacheEntry|RoleCacheEntry|acquire_compiled_cache|with_pinned_compiled_program|with_pinned_compiled_role|release_compiled_cache_lease" \
  "${ROOT_DIR}/src/session/cluster/core.rs" "${ROOT_DIR}/src/endpoint/kernel/core.rs"; then
  echo "boundary deny pattern detected: runtime compiled cache owner" >&2
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
