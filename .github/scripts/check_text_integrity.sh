#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

for script in .github/scripts/*.sh; do
  first="$(head -n 1 "${script}")"
  if [[ "${first}" != '#!/usr/bin/env bash' ]]; then
    echo "script integrity violation: ${script} has invalid shebang" >&2
    exit 1
  fi
  if [[ "$(wc -l < "${script}")" -lt 2 ]]; then
    echo "script integrity violation: ${script} collapsed into one line" >&2
    exit 1
  fi
  bash -n "${script}"
done

for workflow in .github/workflows/*.yml .github/workflows/*.yaml; do
  [[ -e "${workflow}" ]] || continue
  if [[ "$(wc -l < "${workflow}")" -lt 5 ]]; then
    echo "workflow integrity violation: ${workflow} collapsed into one line" >&2
    exit 1
  fi
done

echo "text integrity check passed"
