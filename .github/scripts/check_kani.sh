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
  packed_sidecar_range_is_aligned_and_monotonic \
  packed_sidecar_pair_is_aligned_and_disjoint \
  sidecar_overlap_is_symmetric_and_exact \
  resolver_storage_layout_is_bounded_and_exact; do
  if ! grep -Eq "^fn ${harness}\(\)" \
    "${ROOT_DIR}/src/rendezvous/core/storage_layout/capacity/kani.rs"; then
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
  --harness packed_sidecar_range_is_aligned_and_monotonic \
  --harness packed_sidecar_pair_is_aligned_and_disjoint \
  --harness sidecar_overlap_is_symmetric_and_exact \
  --harness resolver_storage_layout_is_bounded_and_exact

echo "Kani gate passed version=${EXPECTED_VERSION} harnesses=7 backend=CBMC"
