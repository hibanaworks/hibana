#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="${ROOT_DIR}/proofs/kani/Cargo.toml"
EXPECTED_VERSION="$(< "${ROOT_DIR}/.github/kani-version")"
ACTUAL_VERSION="$(cargo kani --version)"

if [[ "${ACTUAL_VERSION}" != "cargo-kani ${EXPECTED_VERSION}" ]]; then
  echo "Kani gate requires cargo-kani ${EXPECTED_VERSION}, found: ${ACTUAL_VERSION}" >&2
  exit 1
fi

python3 - "${ROOT_DIR}/src" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
marker = re.compile(
    r"#\[kani::should_panic\]\s*(?:#\[[^\n]+\]\s*)*"
    r"(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?fn\s+([A-Za-z0-9_]+)\s*\(\s*\)\s*\{"
)
for path in sorted(root.rglob("*.rs")):
    source = path.read_text()
    for match in marker.finditer(source):
        depth = 1
        cursor = match.end()
        while cursor < len(source) and depth:
            if source[cursor] == "{":
                depth += 1
            elif source[cursor] == "}":
                depth -= 1
            cursor += 1
        if depth:
            print(f"unterminated Kani should-panic harness: {path}:{match.group(1)}", file=sys.stderr)
            sys.exit(1)
        body = source[match.end():cursor - 1]
        direct_panic = re.search(
            r"\b(?:(?:debug_)?assert(?:_eq|_ne)?|panic|unreachable|todo|unimplemented)!\s*\("
            r"|\b(?:crate::)?invariant\s*\(",
            body,
        )
        if direct_panic:
            print(
                f"Kani should-panic harness may panic before its production call: {path}:{match.group(1)}",
                file=sys.stderr,
            )
            sys.exit(1)
PY

for harness in \
  endpoint_generation_advances_or_exhausts \
  endpoint_gap_placement_is_aligned_and_bounded \
  endpoint_lease_storage_layout_is_bounded_and_exact \
  association_storage_layout_is_bounded_and_exact \
  route_storage_layout_is_bounded_and_exact \
  packed_sidecar_range_is_aligned_and_monotonic \
  packed_sidecar_pair_is_aligned_and_disjoint \
  four_resident_sidecars_compact_before_all_source_ranges \
  sidecar_overlap_is_symmetric_and_exact \
  resolver_storage_layout_is_bounded_and_exact; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/rendezvous/core/storage_layout/capacity/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

for harness in \
  resolver_registration_key_is_program_and_id \
  resolver_registration_key_rejects_intrinsic_id \
  resolver_initial_storage_is_initialized_and_dispatchable \
  resolver_replacement_compacts_entries_and_preserves_dispatch; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/session/cluster/core/dynamic_resolvers/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

for harness in \
  route_arm_decoding_accepts_exact_binary_domain \
  single_ready_mask_decoding_is_exact; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/endpoint/kernel/authority/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

for harness in \
  frame_header_roundtrip_preserves_every_field \
  frame_header_identity_is_exact_and_injective; do
  if ! grep -Eq "^fn ${harness}\\(\\)" \
    "${ROOT_DIR}/src/transport/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

for harness in \
  packed_event_conflict_decoding_accepts_exact_domain \
  optional_route_arm_decoding_accepts_exact_domain \
  packed_local_dependency_decoding_accepts_exact_domain \
  packed_local_dependency_event_bounds_are_exact; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/global/typestate/facts/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

for harness in \
  packed_lane_range_encoding_avoids_reserved_sentinel \
  packed_lane_range_reserved_sentinel_is_rejected \
  resident_route_arm_index_decoding_accepts_exact_binary_domain \
  resident_route_scope_decoding_accepts_exact_domain \
  resident_roll_scope_decoding_accepts_exact_domain \
  resident_event_header_decoding_accepts_exact_domain \
  resident_local_step_lane_decoding_accepts_exact_domain \
  resident_route_commit_decision_match_is_exact \
  resident_route_arm_lane_step_decoding_accepts_exact_domain \
  role_image_fit_probe_rejects_undersized_storage \
  role_image_fit_probe_rejects_plan_mismatch; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/global/role_program/image_impl/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

for harness in scope_id_decoding_accepts_exact_compact_domain; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/global/const_dsl/scope/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

for harness in \
  route_resolver_row_decoding_accepts_exact_domain \
  dynamic_route_resolver_identity_is_scope_and_id; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/global/compiled/images/image/route_resolvers/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

for harness in \
  program_image_columns_are_canonical_for_exact_count_domain \
  program_image_columns_reject_total_byte_overflow \
  program_image_fit_probe_rejects_undersized_storage \
  program_image_constructor_rejects_undersized_storage \
  scope_marker_identity_tag_is_exact_and_injective \
  packed_column_range_construction_is_exact_for_resident_stride_domain \
  compiled_program_column_range_rejects_stride_multiplication_overflow \
  role_image_column_range_rejects_stride_multiplication_overflow \
  compiled_program_blob_comparison_matches_array_equality \
  compiled_program_image_identity_is_exact_over_facts_columns_and_blob \
  program_atom_row_decoding_accepts_exact_domain \
  compiled_program_atom_blob_decoding_preserves_every_schema_bit; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/global/compiled/images/image/program_ref/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

proof_inventory="$(mktemp "${TMPDIR:-/tmp}/hibana-kani-proofs.XXXXXX")"
gate_inventory="$(mktemp "${TMPDIR:-/tmp}/hibana-kani-gate.XXXXXX")"
trap 'rm -f "${proof_inventory}" "${gate_inventory}"' EXIT
while IFS= read -r proof_file; do
  awk '
    /#\[kani::proof\]/ { proof = 1; next }
    proof && /(^|[[:space:]])fn [A-Za-z0-9_]+\(\)/ {
      name = $0
      sub(/^.*fn /, "", name)
      sub(/\(.*/, "", name)
      print name
      proof = 0
    }
  ' "${proof_file}"
done < <(
  find "${ROOT_DIR}/src" -type f -name '*.rs' \
    -exec grep -lF '#[kani::proof]' {} + | LC_ALL=C sort
) | LC_ALL=C sort >"${proof_inventory}"
sed -n 's/^[[:space:]]*--harness \([A-Za-z0-9_]*\)[[:space:]]*\\*$/\1/p' \
  "${BASH_SOURCE[0]}" | LC_ALL=C sort >"${gate_inventory}"
if [[ -n "$(uniq -d "${proof_inventory}")" ]] \
  || [[ -n "$(uniq -d "${gate_inventory}")" ]]; then
  echo "Kani proof and gate harness names must each be unique" >&2
  exit 1
fi
if ! diff -u "${proof_inventory}" "${gate_inventory}"; then
  echo "Kani gate harness inventory does not match production proof inventory" >&2
  exit 1
fi

RUSTFLAGS="-D warnings" CARGO_BUILD_JOBS=1 cargo kani \
  --manifest-path "${MANIFEST}" \
  --target-dir "${ROOT_DIR}/target/kani" \
  -Z unstable-options \
  --run-sanity-checks \
  --output-format terse \
  --harness endpoint_generation_advances_or_exhausts \
  --harness endpoint_gap_placement_is_aligned_and_bounded \
  --harness endpoint_lease_storage_layout_is_bounded_and_exact \
  --harness association_storage_layout_is_bounded_and_exact \
  --harness route_storage_layout_is_bounded_and_exact \
  --harness packed_sidecar_range_is_aligned_and_monotonic \
  --harness packed_sidecar_pair_is_aligned_and_disjoint \
  --harness four_resident_sidecars_compact_before_all_source_ranges \
  --harness sidecar_overlap_is_symmetric_and_exact \
  --harness resolver_storage_layout_is_bounded_and_exact \
  --harness route_arm_decoding_accepts_exact_binary_domain \
  --harness single_ready_mask_decoding_is_exact \
  --harness frame_header_roundtrip_preserves_every_field \
  --harness frame_header_identity_is_exact_and_injective \
  --harness packed_event_conflict_decoding_accepts_exact_domain \
  --harness optional_route_arm_decoding_accepts_exact_domain \
  --harness packed_local_dependency_decoding_accepts_exact_domain \
  --harness packed_local_dependency_event_bounds_are_exact \
  --harness packed_lane_range_encoding_avoids_reserved_sentinel \
  --harness packed_lane_range_reserved_sentinel_is_rejected \
  --harness resident_route_arm_index_decoding_accepts_exact_binary_domain \
  --harness resident_route_scope_decoding_accepts_exact_domain \
  --harness resident_roll_scope_decoding_accepts_exact_domain \
  --harness resident_event_header_decoding_accepts_exact_domain \
  --harness resident_local_step_lane_decoding_accepts_exact_domain \
  --harness resident_route_commit_decision_match_is_exact \
  --harness resident_route_arm_lane_step_decoding_accepts_exact_domain \
  --harness role_image_fit_probe_rejects_undersized_storage \
  --harness role_image_fit_probe_rejects_plan_mismatch \
  --harness scope_id_decoding_accepts_exact_compact_domain \
  --harness route_resolver_row_decoding_accepts_exact_domain \
  --harness dynamic_route_resolver_identity_is_scope_and_id \
  --harness resolver_registration_key_is_program_and_id \
  --harness resolver_registration_key_rejects_intrinsic_id \
  --harness resolver_initial_storage_is_initialized_and_dispatchable \
  --harness resolver_replacement_compacts_entries_and_preserves_dispatch \
  --harness program_image_columns_are_canonical_for_exact_count_domain \
  --harness program_image_columns_reject_total_byte_overflow \
  --harness program_image_fit_probe_rejects_undersized_storage \
  --harness program_image_constructor_rejects_undersized_storage \
  --harness scope_marker_identity_tag_is_exact_and_injective \
  --harness packed_column_range_construction_is_exact_for_resident_stride_domain \
  --harness compiled_program_column_range_rejects_stride_multiplication_overflow \
  --harness role_image_column_range_rejects_stride_multiplication_overflow \
  --harness compiled_program_blob_comparison_matches_array_equality \
  --harness compiled_program_image_identity_is_exact_over_facts_columns_and_blob \
  --harness program_atom_row_decoding_accepts_exact_domain \
  --harness compiled_program_atom_blob_decoding_preserves_every_schema_bit

echo "Kani gate passed version=${EXPECTED_VERSION} harnesses=48 backend=CBMC"
