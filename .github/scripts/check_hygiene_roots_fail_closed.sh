#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

tmp="$(mktemp -d "${TMPDIR:-/tmp}/hibana-hygiene-roots.XXXXXX")"
trap 'rm -rf "${tmp}"' EXIT

printf 'pub fn fixture_violation() {}\n' >"${tmp}/violation.rs"
printf 'RUSTC_''BOOTSTRAP=1\n' >"${tmp}/Cargo.toml"

FAILED=0
check_absent "fixture_violation" \
  "missing root fixture" \
  "${tmp}/missing-root" "${tmp}/violation.rs"
if [[ "${FAILED}" -eq 0 ]]; then
  echo "hygiene root self-test violation: missing root was treated as success" >&2
  exit 1
fi

FAILED=0
check_absent "RUSTC_""BOOTSTRAP" \
  "optional root fixture" \
  "${tmp}/Cargo.toml" --optional "${tmp}/.cargo"
if [[ "${FAILED}" -eq 0 ]]; then
  echo "hygiene root self-test violation: absent optional root hid an existing violation" >&2
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

FAILED=0
check_absent "[" \
  "rg exit 2 fixture" \
  "${tmp}/violation.rs"
if [[ "${FAILED}" -eq 0 ]]; then
  echo "hygiene root self-test violation: rg exit 2 was treated as success" >&2
  exit 1
fi

FAILED=0
naked_rg_suppression='rg .*2>/dev/''null|2>/dev/''null.*rg|rg .*[|][|][[:space:]]*true'
capture_rg NAKED_RG_SUPPRESSION_MATCHES \
  "naked rg suppression in hygiene scripts" \
  -n "${naked_rg_suppression}" .github/scripts
if [[ -n "${NAKED_RG_SUPPRESSION_MATCHES}" ]]; then
  echo "${NAKED_RG_SUPPRESSION_MATCHES}" >&2
  echo "hygiene root self-test violation: naked rg suppression detected" >&2
  exit 1
fi
bare_true_suppression='[|][|][[:space:]]*true'
capture_rg BARE_TRUE_SUPPRESSION_MATCHES \
  "bare true suppression in hygiene scripts" \
  -n "${bare_true_suppression}" .github/scripts
if [[ -n "${BARE_TRUE_SUPPRESSION_MATCHES}" ]]; then
  echo "${BARE_TRUE_SUPPRESSION_MATCHES}" >&2
  echo "hygiene root self-test violation: bare pipe-true suppression detected" >&2
  exit 1
fi
if [[ "${FAILED}" -ne 0 ]]; then
  echo "hygiene root self-test violation: suppression scan failed" >&2
  exit 1
fi

echo "hygiene root fail-closed self-test passed"
