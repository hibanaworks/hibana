#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_max_lines() {
  local path="$1"
  local max="$2"
  local count
  count="$(wc -l < "${path}" | tr -d ' ')"
  if (( count > max )); then
    echo "public surface budget violation: ${path} has ${count} lines, budget is ${max}" >&2
    FAILED=1
  fi
}

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n -U "${pattern}" "$@"; then
    echo "public surface budget violation: ${label}" >&2
    FAILED=1
  fi
}

check_max_lines ".github/allowlists/lib-public-api.txt" 3
check_max_lines ".github/allowlists/g-public-api.txt" 15
check_max_lines ".github/allowlists/endpoint-public-api.txt" 11
check_max_lines ".github/allowlists/runtime-public-api.txt" 92

python3 "${ROOT_DIR}/.github/scripts/check_public_api_allowlists.py" || FAILED=1

OLD_WORD='leg''acy'
MODE_WORD='comp''at'
ALT_WORD='fall''back'
RECOVERY_WORD='res''cue'
GUESS_WORD='heur''istic'

check_absent \
  "g::advanced|binding::advanced|FlowSendArg|SendOutcomeKind|CapFlow|FlowInner|DynamicResolution|IncomingClassification|from_fn|from_state|${ALT_WORD}|${OLD_WORD}|${MODE_WORD}|${GUESS_WORD}|${RECOVERY_WORD}|state machine|TransportSnapshotParts|ConfigParts|RegisteredTokenParts|ProjectionMessageSpec|ProjectionTypeFingerprint|TransportOpsError|has_fin" \
  "forbidden final-form names in public API allowlists" \
  .github/allowlists

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "public surface budget check passed"
