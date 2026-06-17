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
check_required "fixture_violation" \
  "missing required file fixture" \
  "${tmp}/missing-file.rs"
if [[ "${FAILED}" -eq 0 ]]; then
  echo "hygiene root self-test violation: missing required file was treated as success" >&2
  exit 1
fi

representative_script="${tmp}/representative-migrated.sh"
cat >"${representative_script}" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
root_dir="$1"
fixture="$2"
cd "${root_dir}"
source ./.github/scripts/lib/hygiene_common.sh
FAILED=0
check_absent "[" "representative migrated invalid regex" "${fixture}"
if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi
SH
if bash "${representative_script}" "${ROOT_DIR}" "${tmp}/violation.rs"; then
  echo "hygiene root self-test violation: representative migrated script accepted rg exit 2" >&2
  exit 1
fi

FAILED=0
naked_rg_suppression='r''g .*2>/dev/''null|2>/dev/''null.*r''g|r''g .*[|][|][[:space:]]*tr''ue'
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

migrated_deny_scripts=(
  .github/scripts/check_boundary_contracts.sh
  .github/scripts/check_endpoint_surface_owner.sh
  .github/scripts/check_exact_layout_hygiene.sh
  .github/scripts/check_hygiene_roots_fail_closed.sh
  .github/scripts/check_lowering_hygiene.sh
  .github/scripts/check_mgmt_boundary.sh
  .github/scripts/check_no_custom_target_json.sh
  .github/scripts/check_no_nightly_features.sh
  .github/scripts/check_no_underscore_discards.sh
  .github/scripts/check_plane_boundaries.sh
  .github/scripts/check_public_surface_budget.sh
  .github/scripts/check_raw_future_hygiene.sh
  .github/scripts/check_resolver_context_surface.sh
  .github/scripts/check_resolver_surface_hygiene.sh
  .github/scripts/check_route_frontier_owner.sh
  .github/scripts/check_runtime_performance_hygiene.sh
  .github/scripts/check_segmented_lowering_hygiene.sh
  .github/scripts/check_summary_authority_hygiene.sh
  .github/scripts/check_surface_hygiene.sh
  .github/scripts/check_surface_test_alias_hygiene.sh
  .github/scripts/check_unsafe_contract_hygiene.sh
)
bare_rg_branch='(^|[;&|({[:space:]])(elif|if)[[:space:]]+![[:space:]]*rg\b|(^|[;&|({[:space:]])(elif|if)[[:space:]]+rg\b'
bare_rg_pipe='[|][[:space:]]*rg\b'
bare_rg_redirect='rg[[:space:]][^\n]*(>/dev/''null|2>/dev/''null)'
bare_rg_quiet='rg[[:space:]][^\n]*-q\b'
bare_rg_true='[|][|][[:space:]]*true'
capture_rg BARE_RG_MATCHES \
  "bare rg execution in migrated deny scripts" \
  -n "${bare_rg_branch}|${bare_rg_pipe}|${bare_rg_redirect}|${bare_rg_quiet}|${bare_rg_true}" \
  "${migrated_deny_scripts[@]}"
if [[ -n "${BARE_RG_MATCHES}" ]]; then
  echo "${BARE_RG_MATCHES}" >&2
  echo "hygiene root self-test violation: migrated deny scripts must use hygiene_common.sh for rg execution" >&2
  exit 1
fi
if [[ "${FAILED}" -ne 0 ]]; then
  echo "hygiene root self-test violation: migrated deny script source scan failed" >&2
  exit 1
fi

echo "hygiene root fail-closed self-test passed"
