#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

bash "${ROOT_DIR}/.github/scripts/check_mgmt_boundary.sh"
bash "${ROOT_DIR}/.github/scripts/check_plane_boundaries.sh"
bash "${ROOT_DIR}/.github/scripts/check_resolver_context_surface.sh"
bash "${ROOT_DIR}/.github/scripts/check_lowering_hygiene.sh"
bash "${ROOT_DIR}/.github/scripts/check_surface_hygiene.sh"
