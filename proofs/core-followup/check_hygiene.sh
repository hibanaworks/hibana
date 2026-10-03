#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
for check in \
  check_source_file_budget \
  check_maintainability_budgets \
  check_source_lowering_hygiene \
  check_lowering_hygiene \
  check_compiled_descriptor_authority \
  check_no_nightly_features \
  check_no_generic_const_exprs \
  check_no_underscore_discards \
  check_surface_hygiene \
  check_surface_test_alias_hygiene \
  check_endpoint_surface_owner \
  check_text_integrity \
  check_no_split_guard_literals
do
  bash ".github/scripts/${check}.sh"
done
git diff --check
