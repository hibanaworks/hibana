#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}"
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
bash ./.github/scripts/check_huge_choreography_budget.sh
bash ./.github/scripts/check_package_artifact.sh
