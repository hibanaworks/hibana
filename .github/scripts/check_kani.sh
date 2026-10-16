#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec bash "${ROOT_DIR}/.github/scripts/lib/resource_guard.sh" \
  bash "${ROOT_DIR}/.github/scripts/lib/kani_source_tool.sh" --gate "${ROOT_DIR}" "$@"
