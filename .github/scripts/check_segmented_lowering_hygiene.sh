#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

check_required_regex 'MAX_SEGMENT_EFFS' \
  "segmented lowering hygiene violation: missing segment-local effect capacity" \
  src/eff.rs
check_required_regex 'MAX_SEGMENTS:\s*usize\s*=\s*32\b' \
  "segmented lowering hygiene violation: MAX_SEGMENTS must match final-form plan capacity 32" \
  src/eff.rs
check_required_regex 'MAX_EFF_NODES:\s*usize\s*=\s*MAX_SEGMENTS\s*\*\s*MAX_SEGMENT_EFFS' \
  "segmented lowering hygiene violation: total effect capacity must derive from segment capacity" \
  src/eff.rs
check_absent 'MAX_EFF_NODES:\s*usize\s*=\s*256\b|data:\s*\[EffStruct;\s*MAX_CAPACITY\]' \
  "segmented lowering hygiene violation: flat single-cap EffList storage detected" \
  src/eff.rs src/global/const_dsl.rs
check_absent 'pub[[:space:]]+const[[:space:]]+fn[[:space:]]+from_usize' \
  "segmented lowering hygiene violation: EffIndex must not expose a public flat ordinal constructor" \
  src/eff.rs
check_absent 'pub[[:space:]]+const[[:space:]]+fn[[:space:]]+as_usize' \
  "segmented lowering hygiene violation: EffIndex must not expose a public flat ordinal accessor" \
  src/eff.rs
check_absent 'pub[[:space:]]+const[[:space:]]+fn[[:space:]]+raw' \
  "segmented lowering hygiene violation: EffIndex must not expose a public raw accessor" \
  src/eff.rs
check_absent 'pub[[:space:]]+const[[:space:]]+(ZERO|MAX):' \
  "segmented lowering hygiene violation: EffIndex must not expose public absence constructors" \
  src/eff.rs
check_required_regex 'pub\(crate\)[[:space:]]+const[[:space:]]+fn[[:space:]]+segment\(self\)[[:space:]]*->[[:space:]]*u16' \
  "segmented lowering hygiene violation: EffIndex segment accessor must remain crate-private" \
  src/eff.rs
check_required_regex 'pub\(crate\)[[:space:]]+const[[:space:]]+fn[[:space:]]+offset\(self\)[[:space:]]*->[[:space:]]*u16' \
  "segmented lowering hygiene violation: EffIndex segment-local offset accessor must remain crate-private" \
  src/eff.rs
check_absent 'pub[[:space:]]+const[[:space:]]+fn[[:space:]]+new\(segment:[[:space:]]*u16,[[:space:]]*offset:[[:space:]]*u16\)' \
  "segmented lowering hygiene violation: EffIndex must not expose segment storage as a public constructor" \
  src/eff.rs
check_required_regex 'pub\(crate\)[[:space:]]+const[[:space:]]+fn[[:space:]]+from_segment_offset\(segment:[[:space:]]*u16,[[:space:]]*offset:[[:space:]]*u16\)' \
  "segmented lowering hygiene violation: segment/offset construction must remain crate-private and honestly named" \
  src/eff.rs
check_required_regex 'pub\(crate\)[[:space:]]+const[[:space:]]+fn[[:space:]]+from_dense_ordinal' \
  "segmented lowering hygiene violation: flat EffIndex construction must remain crate-private and honestly named" \
  src/eff.rs
check_required_regex 'pub\(crate\)[[:space:]]+const[[:space:]]+fn[[:space:]]+dense_ordinal\(self\)[[:space:]]*->[[:space:]]*usize' \
  "segmented lowering hygiene violation: flat EffIndex accessor must remain crate-private and honestly named" \
  src/eff.rs
check_absent '\b(as_eff_index|eff_ordinal)\b' \
  "segmented lowering hygiene violation: do not keep renamed flat EffIndex helper aliases" \
  src/global src/endpoint src/runtime.rs
check_absent 'if[[:space:]]+self\.segment\(\)[[:space:]]*==[[:space:]]*0|write!\(f,[[:space:]]*"\{\}"[[:space:]]*,[[:space:]]*self\.offset\(\)\)' \
  "segmented lowering hygiene violation: EffIndex Display must not render segment-zero as a flat ordinal" \
  src/eff.rs
check_absent '\b[Mm]onolithic lowering\b' \
  "segmented lowering hygiene violation: final-form source must not describe lowering as monolithic" \
  src
check_required_regex 'pub\(crate\)[[:space:]]+const[[:space:]]+ZERO:' \
  "segmented lowering hygiene violation: EffIndex zero value must remain crate-private" \
  src/eff.rs
check_absent 'pub\(crate\)[[:space:]]+const[[:space:]]+MAX:' \
  "segmented lowering hygiene violation: EffIndex must not keep an unused max value" \
  src/eff.rs
check_required_regex 'segments:\s*\[\[EffStruct;\s*MAX_SEGMENT_EFFS\];\s*MAX_SEGMENTS\]' \
  "segmented lowering hygiene violation: EffList must use fixed segment storage" \
  src/global/const_dsl.rs
check_required_regex 'struct SegmentSummary' \
  "segmented lowering hygiene violation: segment-local summaries must be explicit" \
  src/global/const_dsl.rs
check_required_regex 'segment_summaries:\s*\[SegmentSummary;\s*MAX_SEGMENTS\]' \
  "segmented lowering hygiene violation: EffList must carry segment-local summaries" \
  src/global/const_dsl.rs
check_required_regex 'struct ProgramImageSegmentData' \
  "segmented lowering hygiene violation: CompiledProgramImage must use segment-local lowering rows" \
  src/global/compiled/lowering/driver.rs
check_absent 'nodes:\s*\[EffStruct;\s*MAX_COMPILED_IMAGE_NODES\]|policies:\s*\[RouteResolver;\s*MAX_COMPILED_IMAGE_NODES\]' \
  "segmented lowering hygiene violation: flat lowering validation rows detected" \
  src/global/compiled/lowering/driver.rs src/global/compiled/lowering/driver
check_required_regex 'segment_at\(|node_at\(|resolver_at_local' \
  "segmented lowering hygiene violation: segment-local lowering view accessors are missing" \
  src/global/compiled/lowering/driver.rs src/global/compiled/lowering/driver

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "segmented lowering hygiene check passed"
