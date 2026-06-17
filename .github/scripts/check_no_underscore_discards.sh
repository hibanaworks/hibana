#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

NAMED_UNDERSCORE_PATTERN='let[[:space:]]+_[A-Za-z0-9_]+([[:space:]]*:[^=]+)?[[:space:]]*='

check_absent "${NAMED_UNDERSCORE_PATTERN}" \
  "production source must not hide live values behind named underscores" \
  src

check_absent 'let[[:space:]]+_[[:space:]]*=[[:space:]]*_[A-Za-z][A-Za-z0-9_]*' \
  "unconsumed named values must be absent or consumed explicitly" \
  src tests

check_absent "${NAMED_UNDERSCORE_PATTERN}" \
  "docs and tests must consume values explicitly" \
  README.md tests

check_absent 'let[[:space:]]+_[[:space:]]*=' \
  "README must not teach wildcard discards" \
  README.md

check_absent '^[[:space:]]*_[A-Za-z0-9_]*storage[[:space:]]*:' \
  "storage owners must be named and read explicitly" \
  src/endpoint/kernel/decision_state.rs

check_absent '^[[:space:]]*where[[:space:]]*\{' \
  "empty where clause detected" \
  src

OLD_WORD='legacy'
MODE_WORD='compat'
ALT_WORD='fallback'
RECOVERY_WORD='rescue'
GUESS_WORD='heuristic'
LAYER_WORD='shim'
DISALLOWED_OLD_VOCAB_PATTERN="\\b(_${OLD_WORD}|_${MODE_WORD}|_${ALT_WORD}|_${RECOVERY_WORD}|_${GUESS_WORD}|_${LAYER_WORD}|${OLD_WORD}_|${MODE_WORD}_|${ALT_WORD}_|${RECOVERY_WORD}_|${GUESS_WORD}_|${LAYER_WORD}_)\\b"
check_absent "${DISALLOWED_OLD_VOCAB_PATTERN}" \
  "forbidden old vocabulary detected" \
  src tests README.md

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "no underscore discard check passed"
