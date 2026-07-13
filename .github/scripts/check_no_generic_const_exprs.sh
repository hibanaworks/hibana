#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0
source ./.github/scripts/lib/hygiene_common.sh

TYPE_LEVEL_ARITHMETIC_PATTERN='generic_const_exprs|where\s*\[\s*\(\)\s*;|\[[^;\n]+;\s*[A-Za-z0-9_:]+::[A-Za-z0-9_]+::<[^>]+>\(\)\s*\]'

if printf '%s\n' 'let values = [symbolic::any::<u8>(), symbolic::any::<u8>()];' \
  | rg -q "${TYPE_LEVEL_ARITHMETIC_PATTERN}"
then
  echo "generic const expression gate rejected an ordinary value array" >&2
  exit 1
fi
if ! printf '%s\n' 'let bytes: [u8; size::of::<T>()];' \
  | rg -q "${TYPE_LEVEL_ARITHMETIC_PATTERN}"
then
  echo "generic const expression gate missed type-level array arithmetic" >&2
  exit 1
fi

check_absent "${TYPE_LEVEL_ARITHMETIC_PATTERN}" \
  "use fixed-capacity + len instead of type-level arithmetic" \
  src tests

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "no generic const expressions check passed"
