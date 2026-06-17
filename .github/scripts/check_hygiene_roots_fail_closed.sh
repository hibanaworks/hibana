#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

tmp="$(mktemp -d "${TMPDIR:-/tmp}/hibana-hygiene-roots.XXXXXX")"
trap 'rm -rf "${tmp}"' EXIT

printf 'pub fn fixture_violation() {}\n' >"${tmp}/violation.rs"

FAILED=0
check_absent "fixture_violation" \
  "missing root fixture" \
  "${tmp}/missing-root" "${tmp}/violation.rs"
if [[ "${FAILED}" -eq 0 ]]; then
  echo "hygiene root self-test violation: missing root was treated as success" >&2
  exit 1
fi

FAILED=0
check_absent "fixture_violation" \
  "violation fixture" \
  "${tmp}/violation.rs"
if [[ "${FAILED}" -eq 0 ]]; then
  echo "hygiene root self-test violation: existing violation was not detected" >&2
  exit 1
fi

FAILED=0
check_absent "fixture_clean" \
  "clean fixture" \
  "${tmp}/violation.rs"
if [[ "${FAILED}" -ne 0 ]]; then
  echo "hygiene root self-test violation: clean search failed" >&2
  exit 1
fi

echo "hygiene root fail-closed self-test passed"
