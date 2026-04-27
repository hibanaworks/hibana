#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if rg -n 'allow\(dead_code\)|cfg_attr\([^)]*allow\(dead_code\)' src tests internal integration 2>/dev/null; then
  echo "descriptor streaming hygiene violation: dead_code allow is forbidden" >&2
  exit 1
fi

if rg -n 'Box<|alloc::|std::boxed' src/global/compiled --glob '!**/*tests.rs'; then
  echo "descriptor streaming hygiene violation: heap-backed descriptor storage in core init path" >&2
  exit 1
fi

if ! rg -n 'with_lowering_lease|RoleLoweringScratchLayout|from_storage' src/global/compiled/materialize >/dev/null; then
  echo "descriptor streaming hygiene violation: lowering must attach through caller-provided lease storage" >&2
  exit 1
fi

if rg -n 'write_clone_to|init_lowering: unsafe fn|source\.init_lowering|MaybeUninit::<LoweringSummary>' src/global/compiled src/global/role_program.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: lowering summary must be borrowed from the typed projection source, not cloned into attach storage" >&2
  exit 1
fi

if ! rg -n 'source\.summary\(\)|summary_only_required, 0' src/global/compiled/materialize/lease.rs src/global/role_program.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: lowering lease must borrow static summaries and reserve caller storage only for role scratch" >&2
  exit 1
fi

if rg -n 'eff_list\.as_slice\(\)' src/global/compiled >/dev/null; then
  echo "descriptor streaming hygiene violation: lowering summary scan must stream through EffList segments" >&2
  exit 1
fi

if rg -n 'view\.as_slice\(\)|LoweringView::as_slice|fn as_slice\(&self\) -> .*\[EffStruct\]' src/global/compiled src/global/typestate >/dev/null; then
  echo "descriptor streaming hygiene violation: compiled descriptor init must stream through segment views, not flat lowering slices" >&2
  exit 1
fi

if rg -n 'pub const fn as_slice\(&self\)' src/global/const_dsl.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: EffList flat view must not be public canonical API" >&2
  exit 1
fi

if rg -n 'impl (core::ops::Deref|AsRef<\[EffStruct\]>) for EffList' src/global/const_dsl.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: EffList must not expose flat slice traits as canonical path" >&2
  exit 1
fi

if ! rg -n 'segment_count\(\)|segment_len\(|node_at\(' src/global/compiled/lowering/driver.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: lowering summary scan must be segment-aware" >&2
  exit 1
fi

if ! rg -n 'while segment_idx < view\.segment_count\(\)' src/global/compiled/lowering/program_image_builder.rs src/global/compiled/lowering/program_owner.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: compiled program image init must stream segment-by-segment" >&2
  exit 1
fi

if ! rg -n 'CompiledRoleSegmentHeader|init_compiled_role_image_segments|segment_headers' src/global/compiled/lowering/role_image_builder.rs src/global/compiled/images/role.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: compiled role image must persist segment descriptor rows" >&2
  exit 1
fi

if ! rg -n 'try_init_compiled_role_image_from_summary|stream_compiled_role_descriptor_rows|rollback_compiled_role_descriptor_stream|validate_compiled_role_descriptor_row_capacity' src/global/compiled/lowering/role_image_builder.rs src/global/compiled/materialize/lease.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: role image descriptor init must expose a rollback-capable streaming transaction" >&2
  exit 1
fi

if rg -n 'init_compiled_role_image_typestate|finalize_compiled_role_image_from_typestate|stream_typestate_and_route_descriptor_rows|stream_typestate_scope_and_route_rows' src/global/compiled/lowering/role_image_builder.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: role image init must be split into descriptor row streaming helpers" >&2
  exit 1
fi

if rg -n 'RoleTypestateInitStorage|init_value_from_summary_for_role|init_role_typestate_value|stream_value_from_summary_for_role' src/global/typestate src/global/compiled/lowering/role_image_builder.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: typestate descriptor streaming must not keep the old bulk init storage/function path" >&2
  exit 1
fi

for required in \
  'stream_typestate_header' \
  'stream_typestate_nodes' \
  'stream_scope_rows' \
  'stream_route_records' \
  'stream_route_slot_by_scope_ordinal' \
  'stream_lane_mask_by_scope' \
  'stream_phase_descriptor_rows_from_steps' \
  'stream_eff_index_to_step_rows' \
  'stream_step_index_to_state_rows' \
  'stream_control_by_eff_rows' \
  'publish_compiled_role_image_offsets'
do
  if ! rg -n "${required}" src/global/compiled/lowering/role_image_builder.rs >/dev/null; then
    echo "descriptor streaming hygiene violation: missing row streaming helper ${required}" >&2
    exit 1
  fi
done

python3 - <<'PY'
import pathlib
import re
import sys

source = pathlib.Path("src/global/compiled/lowering/role_image_builder.rs").read_text()
noop = re.compile(
    r"unsafe fn stream_(typestate_header|scope_rows|route_records|route_slot_by_scope_ordinal|lane_mask_by_scope)"
    r"\s*\([^)]*\)\s*->\s*Result<\(\),[^>]+>\s*\{\s*"
    r"(?:let _ = storage;\s*)?Ok\(\(\)\)\s*\}",
    re.S,
)
if noop.search(source):
    print(
        "descriptor streaming hygiene violation: row-family helpers must own real row writes, not no-op capacity labels",
        file=sys.stderr,
    )
    sys.exit(1)
for helper in [
    "stream_typestate_header",
    "stream_typestate_nodes",
    "stream_scope_rows",
    "stream_route_slot_by_scope_ordinal",
    "stream_route_records",
    "stream_lane_mask_by_scope",
    "stream_phase_descriptor_rows_from_steps",
    "stream_eff_index_to_step_rows",
    "stream_step_index_to_state_rows",
    "stream_control_by_eff_rows",
]:
    match = re.search(
        rf"unsafe fn {helper}\s*\([^{{]*\{{(?P<body>[\s\S]*?)\n\}}",
        source,
    )
    if not match:
        print(f"descriptor streaming hygiene violation: missing row writer body {helper}", file=sys.stderr)
        sys.exit(1)
    body = match.group("body")
    if re.fullmatch(r"\s*(?:let _ = [A-Za-z_][A-Za-z0-9_]*;\s*)*(?:Ok\(\(\)\)\s*)?", body):
        print(f"descriptor streaming hygiene violation: {helper} is a no-op label, not a row writer", file=sys.stderr)
        sys.exit(1)
    if helper == "stream_typestate_header" and "stream_value_header" not in body:
        print("descriptor streaming hygiene violation: typestate header writer must publish the typestate header row", file=sys.stderr)
        sys.exit(1)
    if helper.startswith("stream_route") or helper in {"stream_scope_rows", "stream_lane_mask_by_scope"}:
        expected = {
            "stream_scope_rows": "stream_value_scope_rows_from_walk",
            "stream_route_slot_by_scope_ordinal": "stream_value_route_slot_rows_from_walk",
            "stream_route_records": "stream_value_route_record_rows_from_walk",
            "stream_lane_mask_by_scope": "stream_value_lane_mask_rows_from_walk",
        }.get(helper)
        if expected and expected not in body:
            print(f"descriptor streaming hygiene violation: {helper} must call its concrete typestate row writer {expected}", file=sys.stderr)
            sys.exit(1)
if "RoleImageStreamFault" not in source or "try_init_compiled_role_image_from_summary_with_fault" not in source:
    print(
        "descriptor streaming hygiene violation: row writers must expose test-only writer fault injection",
        file=sys.stderr,
    )
    sys.exit(1)
PY

python3 - <<'PY'
import pathlib
import re
import sys

source = pathlib.Path("src/global/compiled/lowering/role_image_builder.rs").read_text()
match = re.search(r"unsafe fn stream_compiled_role_descriptor_rows[\s\S]*?\n}\n\n#\[inline", source)
if not match:
    print("descriptor streaming hygiene violation: missing stream_compiled_role_descriptor_rows body", file=sys.stderr)
    sys.exit(1)
body = match.group(0)
publish = body.find("publish_compiled_role_image_offsets")
step = body.find("stream_step_index_to_state_rows")
eff = body.find("stream_eff_index_to_step_rows")
control = body.find("stream_control_by_eff_rows")
if publish < 0 or step < 0 or eff < 0 or control < 0 or not (eff < step < control < publish):
    print("descriptor streaming hygiene violation: role image offsets must be published only after all fallible index/control row writers", file=sys.stderr)
    sys.exit(1)
PY

if ! rg -n '\) -> Result<\(\), CompiledRoleImageInitError> \{' src/global/compiled/lowering/role_image_builder.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: typestate/route row streaming must remain fallible" >&2
  exit 1
fi

if ! rg -n 'descriptor_row_writer_faults_roll_back_rows|role_image_phase_row_streaming_failure_rolls_back_rows|role_image_descriptor_row_capacity_failures_roll_back_rows|PhaseHeaderCapacity|RouteRowCapacity|TypestateNodeCapacity' src/global/compiled/images/role.rs src/global/compiled/lowering/role_image_builder.rs >/dev/null; then
  echo "descriptor streaming hygiene violation: descriptor row overflow must fail with a preserved reason and rollback" >&2
  exit 1
fi

echo "descriptor streaming hygiene check passed"
