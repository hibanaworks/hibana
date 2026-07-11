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

for harness in \
  endpoint_generation_advances_or_exhausts \
  endpoint_gap_placement_is_aligned_and_bounded \
  endpoint_lease_storage_layout_is_bounded_and_exact \
  association_storage_layout_is_bounded_and_exact \
  route_storage_layout_is_bounded_and_exact \
  packed_sidecar_range_is_aligned_and_monotonic \
  packed_sidecar_pair_is_aligned_and_disjoint \
  packed_sidecar_pair_compacts_before_source_ranges \
  sidecar_overlap_is_symmetric_and_exact \
  resolver_storage_layout_is_bounded_and_exact; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/rendezvous/core/storage_layout/capacity/kani.rs"; then
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
  resident_route_arm_index_decoding_accepts_exact_binary_domain \
  resident_route_scope_decoding_accepts_exact_domain \
  resident_roll_scope_decoding_accepts_exact_domain \
  resident_event_header_decoding_accepts_exact_domain \
  resident_local_step_lane_decoding_accepts_exact_domain \
  resident_route_commit_decision_match_is_exact \
  resident_route_arm_lane_step_decoding_accepts_exact_domain; do
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

for harness in program_atom_row_decoding_accepts_exact_domain; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/global/compiled/images/image/program_ref/kani.rs"; then
    echo "Kani gate missing proof harness: ${harness}" >&2
    exit 1
  fi
done

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
  --harness packed_sidecar_pair_compacts_before_source_ranges \
  --harness sidecar_overlap_is_symmetric_and_exact \
  --harness resolver_storage_layout_is_bounded_and_exact \
  --harness route_arm_decoding_accepts_exact_binary_domain \
  --harness single_ready_mask_decoding_is_exact \
  --harness packed_event_conflict_decoding_accepts_exact_domain \
  --harness optional_route_arm_decoding_accepts_exact_domain \
  --harness packed_local_dependency_decoding_accepts_exact_domain \
  --harness packed_local_dependency_event_bounds_are_exact \
  --harness packed_lane_range_encoding_avoids_reserved_sentinel \
  --harness resident_route_arm_index_decoding_accepts_exact_binary_domain \
  --harness resident_route_scope_decoding_accepts_exact_domain \
  --harness resident_roll_scope_decoding_accepts_exact_domain \
  --harness resident_event_header_decoding_accepts_exact_domain \
  --harness resident_local_step_lane_decoding_accepts_exact_domain \
  --harness resident_route_commit_decision_match_is_exact \
  --harness resident_route_arm_lane_step_decoding_accepts_exact_domain \
  --harness scope_id_decoding_accepts_exact_compact_domain \
  --harness route_resolver_row_decoding_accepts_exact_domain \
  --harness dynamic_route_resolver_identity_is_scope_and_id \
  --harness program_atom_row_decoding_accepts_exact_domain

echo "Kani gate passed version=${EXPECTED_VERSION} harnesses=28 backend=CBMC"
