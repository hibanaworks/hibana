#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

NAMED_UNDERSCORE_PATTERN='let[[:space:]]+_[A-Za-z0-9_]+([[:space:]]*:[^=]+)?[[:space:]]*='

if rg -n "${NAMED_UNDERSCORE_PATTERN}" src >/dev/null; then
  echo "underscore discard violation: production source must not hide live values behind named underscores" >&2
  rg -n "${NAMED_UNDERSCORE_PATTERN}" src >&2
  exit 1
fi

if rg -n 'let[[:space:]]+_[[:space:]]*=[[:space:]]*_[A-Za-z][A-Za-z0-9_]*' src tests >/dev/null; then
  echo "underscore discard violation: unconsumed named values must be absent or consumed explicitly" >&2
  rg -n 'let[[:space:]]+_[[:space:]]*=[[:space:]]*_[A-Za-z][A-Za-z0-9_]*' src tests >&2
  exit 1
fi

if rg -n "${NAMED_UNDERSCORE_PATTERN}" README.md tests >/dev/null; then
  echo "underscore discard violation: docs and tests must consume values explicitly" >&2
  rg -n "${NAMED_UNDERSCORE_PATTERN}" README.md tests >&2
  exit 1
fi

if rg -n 'let[[:space:]]+_[[:space:]]*=' README.md >/dev/null; then
  echo "underscore discard violation: README must not teach wildcard discards" >&2
  rg -n 'let[[:space:]]+_[[:space:]]*=' README.md >&2
  exit 1
fi

if rg -n '^[[:space:]]*_[A-Za-z0-9_]*storage[[:space:]]*:' src/endpoint/kernel/decision_state.rs >/dev/null; then
  echo "underscore discard violation: storage owners must be named and read explicitly" >&2
  rg -n '^[[:space:]]*_[A-Za-z0-9_]*storage[[:space:]]*:' src/endpoint/kernel/decision_state.rs >&2
  exit 1
fi

if rg -n '^[[:space:]]*where[[:space:]]*\{' src >/dev/null; then
  echo "underscore discard violation: empty where clause detected" >&2
  rg -n '^[[:space:]]*where[[:space:]]*\{' src >&2
  exit 1
fi

OLD_WORD='leg''acy'
MODE_WORD='comp''at'
ALT_WORD='fall''back'
RECOVERY_WORD='res''cue'
GUESS_WORD='heur''istic'
LAYER_WORD='sh''im'
DISALLOWED_OLD_VOCAB_PATTERN="\\b(_${OLD_WORD}|_${MODE_WORD}|_${ALT_WORD}|_${RECOVERY_WORD}|_${GUESS_WORD}|_${LAYER_WORD}|${OLD_WORD}_|${MODE_WORD}_|${ALT_WORD}_|${RECOVERY_WORD}_|${GUESS_WORD}_|${LAYER_WORD}_)\\b"
if rg -n "${DISALLOWED_OLD_VOCAB_PATTERN}" \
  src tests README.md >/dev/null; then
  echo "underscore discard violation: forbidden old vocabulary detected" >&2
  rg -n "${DISALLOWED_OLD_VOCAB_PATTERN}" \
    src tests README.md >&2
  exit 1
fi
