#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if [[ "${HIBANA_FINAL_FORM_COMPILE_PRESSURE_GUARD:-1}" != "0" \
  && "${HIBANA_FINAL_FORM_COMPILE_PRESSURE_GUARD_ACTIVE:-0}" != "1" ]]; then
  source "${ROOT_DIR}/.github/scripts/lib/compile_pressure_guard.sh"
  HIBANA_FINAL_FORM_COMPILE_PRESSURE_GUARD_ACTIVE=1 \
    HIBANA_COMPILE_PRESSURE_LABEL=final_form_gate \
    run_with_compile_pressure_guard \
      "final-form gate" \
      env HIBANA_FINAL_FORM_COMPILE_PRESSURE_GUARD_ACTIVE=1 bash "$0" "$@"
  exit "$?"
fi

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/configure_ui_diagnostics.sh"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg

bash ./.github/scripts/check_tracked_worktree_closure.sh
bash ./.github/scripts/check_text_integrity.sh
bash ./.github/scripts/check_source_file_budget.sh
bash ./.github/scripts/check_maintainability_budgets.sh
bash ./.github/scripts/check_surface_test_alias_hygiene.sh
bash ./.github/scripts/check_no_split_guard_literals.sh
python3 .github/scripts/check_public_api_allowlists.py --self-test
bash ./.github/scripts/check_no_underscore_discards.sh
bash ./.github/scripts/check_rust_1_95_stable.sh
bash ./.github/scripts/check_no_nightly_features.sh
bash ./.github/scripts/check_no_generic_const_exprs.sh
bash ./.github/scripts/check_hygiene_roots_fail_closed.sh
bash ./.github/scripts/check_no_custom_target_json.sh
bash ./.github/scripts/check_no_std_build.sh
bash ./.github/scripts/check_warning_free.sh
cargo +"${TOOLCHAIN}" doc -p hibana --no-deps --document-private-items
bash ./.github/scripts/check_hibana_public_api.sh --surface-only
bash ./.github/scripts/check_resolver_context_surface.sh
bash ./.github/scripts/check_unsafe_contract_hygiene.sh
bash ./.github/scripts/check_boundary_contracts.sh --local-only
bash ./.github/scripts/check_plane_boundaries.sh
bash ./.github/scripts/check_mgmt_boundary.sh
bash ./.github/scripts/check_resolver_surface_hygiene.sh
bash ./.github/scripts/check_lowering_hygiene.sh
bash ./.github/scripts/check_summary_authority_hygiene.sh
bash ./.github/scripts/check_segmented_lowering_hygiene.sh
bash ./.github/scripts/check_descriptor_streaming_hygiene.sh
bash ./.github/scripts/check_frozen_image_hygiene.sh
bash ./.github/scripts/check_exact_layout_hygiene.sh
bash ./.github/scripts/check_raw_future_hygiene.sh
bash ./.github/scripts/check_kernel_monomorphization_quarantine.sh
bash ./.github/scripts/check_message_monomorphization_hygiene.sh
bash ./.github/scripts/check_final_form_measurements.sh
bash ./.github/scripts/check_runtime_performance_hygiene.sh
bash ./.github/scripts/check_route_authority_taxonomy.sh
bash ./.github/scripts/check_compiled_descriptor_authority.sh
bash ./.github/scripts/check_route_frontier_owner.sh
bash ./.github/scripts/check_direct_projection_binary.sh
bash ./.github/scripts/check_subsystem_budget_gates.sh
bash ./.github/scripts/check_package_artifact.sh
