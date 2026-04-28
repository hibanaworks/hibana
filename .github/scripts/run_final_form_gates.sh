#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/configure_ui_diagnostics.sh"

bash ./.github/scripts/check_text_integrity.sh
bash ./.github/scripts/check_rust_1_95_stable.sh
bash ./.github/scripts/check_no_nightly_features.sh
bash ./.github/scripts/check_no_generic_const_exprs.sh
bash ./.github/scripts/check_no_custom_target_json.sh
bash ./.github/scripts/check_no_std_build.sh
bash ./.github/scripts/check_warning_free.sh
bash ./.github/scripts/check_hibana_public_api.sh
bash ./.github/scripts/check_public_surface_budget.sh
bash ./.github/scripts/run_ui_gate.sh
cargo +"${TOOLCHAIN}" test -p hibana --test public_surface_guards --features std
cargo +"${TOOLCHAIN}" test -p hibana --test substrate_surface --features std
bash ./.github/scripts/check_segmented_lowering_hygiene.sh
bash ./.github/scripts/check_descriptor_streaming_hygiene.sh
bash ./.github/scripts/check_kernel_monomorphization_quarantine.sh
bash ./.github/scripts/check_route_authority_taxonomy.sh
bash ./.github/scripts/check_compiled_descriptor_authority.sh
bash ./.github/scripts/check_topology_hygiene.sh
bash ./.github/scripts/check_final_form_measurements.sh
