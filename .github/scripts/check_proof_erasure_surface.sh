#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

readonly PROOF_METADATA_PATTERN='ProductionKernelArtifact|ProtocolCapability|CarrierProfile|ElasticOccurrenceKey|iteration_epoch|protocol_image_digest|carrier_profile'

set +e
proof_metadata_matches="$({
  rg -n "${PROOF_METADATA_PATTERN}" src \
    --glob '*.rs' \
    --glob '!src/test_support/**' \
    --glob '!src/**/tests/**' \
    --glob '!src/**/tests.rs' \
    --glob '!src/**/kani.rs'
} 2>&1)"
proof_metadata_status=$?
set -e

if [[ "${proof_metadata_status}" -eq 0 ]]; then
  printf '%s\n' "${proof_metadata_matches}" >&2
  echo "proof-erasure gate found proof-only metadata in production Rust" >&2
  exit 1
fi
if [[ "${proof_metadata_status}" -ne 1 ]]; then
  printf '%s\n' "${proof_metadata_matches}" >&2
  echo "proof-erasure gate could not inspect production Rust" >&2
  exit 1
fi

if ! grep -Fqx 'pub struct FrameHeader([u8; 8]);' src/transport.rs; then
  echo "proof-erasure gate requires the exact eight-byte core frame header" >&2
  exit 1
fi
if ! grep -Fqx '#[cfg(all(test, hibana_repo_tests))]' src/lib.rs \
  || ! grep -Fqx 'mod test_support;' src/lib.rs; then
  echo "proof-erasure gate requires host proof exporters to remain test-only" >&2
  exit 1
fi

echo "proof-erasure gate passed runtime-proof-metadata=0 wire-proof-fields=0 endpoint-proof-types=0 core-header-bytes=8"
