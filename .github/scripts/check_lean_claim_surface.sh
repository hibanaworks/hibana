#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF_DIR="${ROOT_DIR}/proofs/lean"
CLAIMS="${PROOF_DIR}/ClaimSurface.lean"
SNAPSHOT="${PROOF_DIR}/claim-surface.txt"
EXPECTED_CLAIMS=15

if [[ "$(grep -c '^#check @Hibana\.' "${CLAIMS}")" != "${EXPECTED_CLAIMS}" ]]; then
  echo "Lean claim surface must contain exactly ${EXPECTED_CLAIMS} reviewed claims" >&2
  exit 1
fi

actual="$(mktemp "${TMPDIR:-/tmp}/hibana-lean-claim-surface.XXXXXX")"
cleanup() {
  rm -f "${actual}"
}
trap cleanup EXIT

(
  cd "${PROOF_DIR}"
  lake env lean ClaimSurface.lean >"${actual}"
)

if ! diff -u "${SNAPSHOT}" "${actual}"; then
  echo "Lean public claim type surface changed" >&2
  exit 1
fi

echo "Lean claim surface passed claims=${EXPECTED_CLAIMS}"
