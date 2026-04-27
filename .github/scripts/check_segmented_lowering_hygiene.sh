#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if ! rg -n 'MAX_SEGMENT_EFFS' src/eff.rs >/dev/null; then
  echo "segmented lowering hygiene violation: missing segment-local effect capacity" >&2
  exit 1
fi

if ! rg -n 'MAX_SEGMENTS:\s*usize\s*=\s*32\b' src/eff.rs >/dev/null; then
  echo "segmented lowering hygiene violation: MAX_SEGMENTS must match final-form plan capacity 32" >&2
  exit 1
fi

if ! rg -n 'MAX_EFF_NODES:\s*usize\s*=\s*MAX_SEGMENTS\s*\*\s*MAX_SEGMENT_EFFS' src/eff.rs >/dev/null; then
  echo "segmented lowering hygiene violation: total effect capacity must derive from segment capacity" >&2
  exit 1
fi

if rg -n 'MAX_EFF_NODES:\s*usize\s*=\s*256\b|data:\s*\[EffStruct;\s*MAX_CAPACITY\]' src/eff.rs src/global/const_dsl.rs; then
  echo "segmented lowering hygiene violation: flat single-cap EffList storage reintroduced" >&2
  exit 1
fi

if rg -n 'pub[[:space:]]+const[[:space:]]+fn[[:space:]]+from_usize' src/eff.rs >/dev/null; then
  echo "segmented lowering hygiene violation: EffIndex must not expose a public flat ordinal constructor" >&2
  exit 1
fi

if rg -n 'pub[[:space:]]+const[[:space:]]+fn[[:space:]]+new\(segment:[[:space:]]*u16,[[:space:]]*offset:[[:space:]]*u16\)' src/eff.rs >/dev/null; then
  echo "segmented lowering hygiene violation: EffIndex must not expose segment storage as a public constructor" >&2
  exit 1
fi

if ! rg -n 'pub\(crate\)[[:space:]]+const[[:space:]]+fn[[:space:]]+from_segment_offset\(segment:[[:space:]]*u16,[[:space:]]*offset:[[:space:]]*u16\)' src/eff.rs >/dev/null; then
  echo "segmented lowering hygiene violation: segment/offset construction must remain crate-private and honestly named" >&2
  exit 1
fi

if ! rg -n 'pub\(crate\)[[:space:]]+const[[:space:]]+fn[[:space:]]+from_dense_ordinal' src/eff.rs >/dev/null; then
  echo "segmented lowering hygiene violation: flat EffIndex construction must remain crate-private and honestly named" >&2
  exit 1
fi

if ! rg -n 'segments:\s*\[\[EffStruct;\s*MAX_SEGMENT_EFFS\];\s*MAX_SEGMENTS\]' src/global/const_dsl.rs >/dev/null; then
  echo "segmented lowering hygiene violation: EffList must use fixed segment storage" >&2
  exit 1
fi

if ! rg -n 'struct SegmentSummary' src/global/const_dsl.rs >/dev/null; then
  echo "segmented lowering hygiene violation: segment-local summaries must be explicit" >&2
  exit 1
fi

if ! rg -n 'segment_summaries:\s*\[SegmentSummary;\s*MAX_SEGMENTS\]' src/global/const_dsl.rs >/dev/null; then
  echo "segmented lowering hygiene violation: EffList must carry segment-local summaries" >&2
  exit 1
fi

if ! rg -n 'struct LoweringSegmentData' src/global/compiled/lowering/driver.rs >/dev/null; then
  echo "segmented lowering hygiene violation: LoweringSummary must use segment-local lowering rows" >&2
  exit 1
fi

if rg -n 'nodes:\s*\[EffStruct;\s*MAX_LOWERING_NODES\]|policies:\s*\[PolicyMode;\s*MAX_LOWERING_NODES\]|control_descs:\s*\[Option<ControlDesc>;\s*MAX_LOWERING_NODES\]' src/global/compiled/lowering/driver.rs >/dev/null; then
  echo "segmented lowering hygiene violation: flat lowering validation rows reintroduced" >&2
  exit 1
fi

if ! rg -n 'segment_at\(|node_at\(|policy_at_local|control_desc_at_local' src/global/compiled/lowering/driver.rs >/dev/null; then
  echo "segmented lowering hygiene violation: segment-local lowering view accessors are missing" >&2
  exit 1
fi

echo "segmented lowering hygiene check passed"
