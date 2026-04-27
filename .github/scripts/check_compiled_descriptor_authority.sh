#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

python3 - <<'PY'
import pathlib
import re
import sys

root = pathlib.Path.cwd()


def read(path: str) -> str:
    return (root / path).read_text()


def fail(message: str) -> None:
    print(f"compiled descriptor authority violation: {message}", file=sys.stderr)
    sys.exit(1)


def strip_cfg_test_modules(source: str) -> str:
    marker = re.compile(r"#\[cfg\(test\)\]\s*(?:#\[[^\n]*\]\s*)*mod\s+tests\s*\{")
    out = []
    cursor = 0
    while True:
        match = marker.search(source, cursor)
        if match is None:
            out.append(source[cursor:])
            return "".join(out)

        out.append(source[cursor:match.start()])
        close = source.find("\n}\n", match.end())
        if close < 0:
            fail("unterminated cfg(test) module")
        cursor = close + len("\n}\n")


runtime_paths = [
    "src/control/cluster/core.rs",
    "src/rendezvous/core.rs",
    "src/rendezvous/port.rs",
    "src/endpoint/kernel/core.rs",
    "src/endpoint/kernel/endpoint_init.rs",
    "src/endpoint/kernel/decode.rs",
    "src/endpoint/kernel/recv.rs",
    "src/endpoint/kernel/route_frontier/offer.rs",
    "src/endpoint/kernel/runtime/frontier.rs",
    "src/endpoint/kernel/runtime/layout.rs",
    "src/endpoint.rs",
    "src/endpoint/flow.rs",
]

for path in runtime_paths:
    source = strip_cfg_test_modules(read(path))
    for forbidden in [
        r"\bEffList\b",
        r"\bStepCons\b",
        r"\bStepNil\b",
        r"\bSeqSteps\b",
        r"\bRouteSteps\b",
        r"\bParSteps\b",
        r"CompiledRole::compile\(",
        r"CompiledProgram::compile\(",
        r"interpret_eff_list\(",
    ]:
        if re.search(forbidden, source):
            fail(f"{path} reads typed/raw choreography instead of compiled descriptor facts: {forbidden}")

for path in [
    "src/endpoint/kernel/core.rs",
    "src/endpoint/kernel/route_frontier/offer.rs",
    "src/global/typestate/cursor.rs",
]:
    source = strip_cfg_test_modules(read(path))
    if "MAX_EFF_NODES" in source:
            fail(f"{path} uses whole-program effect bounds in route/flow/cursor hot path instead of compiled descriptor bounds")

core = read("src/endpoint/kernel/core.rs")
offer = read("src/endpoint/kernel/route_frontier/offer.rs")
role_image_source = read("src/global/compiled/images/role.rs")
for required in [
    "pub(crate) struct RoleRuntimeTableView",
    "route_record_by_dense_route",
    "route_dense_by_scope_slot",
    "route_offer_lane_words_by_dense_route",
    "phase_headers",
    "control_by_eff: &'a [ControlDesc]",
    "pub(crate) fn control_by_eff(&self) -> &[ControlDesc]",
    "runtime_tables(",
]:
    if required not in role_image_source:
        fail(f"role image missing precomputed runtime table view: {required}")

if "par_join_by_scope" in role_image_source:
    fail("role runtime table view must not label phase headers as par join by scope")

if re.search(r"control_by_eff:\s*self\.eff_index_to_step\(\)", role_image_source):
    fail("control_by_eff must be a ControlDesc row table, not an eff-to-step map")

cluster = read("src/control/cluster/core.rs")
for required in [
    "CompiledProgramRef",
    "RoleImageSlice",
    "CompiledRoleImage::persistent_bytes_for_program",
    "RoleImageSlice::from_raw",
    "CompiledProgramRef::from_raw",
]:
    if required not in cluster:
        fail(f"SessionKit/cluster attach path missing compiled descriptor authority: {required}")

endpoint_init = read("src/endpoint/kernel/endpoint_init.rs")
for required in [
    "CompiledProgramRef",
    "CompiledRoleImage",
    "KernelEndpointHeader::new",
]:
    if required not in endpoint_init:
        fail(f"endpoint init path missing compiled image/header authority: {required}")

role_image = read("src/global/compiled/images/role.rs")
role_program = read("src/global/role_program.rs")
for required in [
    "pub(crate) struct CompiledRoleImage",
    "typestate_offset: u16",
    "phase_headers_offset: u16",
    "eff_index_to_step_offset: u16",
    "step_index_to_state_offset: u16",
    "pub(in crate::global::compiled) struct RoleResidentFacts",
]:
    if required not in role_image:
        fail(f"compiled role image is not a compact offset/facts owner: {required}")

for required in [
    "fn next_set_from(",
    "lane_set_view_iterates_set_bits_without_empty_lane_scan",
]:
    if required not in role_program:
        fail(f"LaneSetView must iterate compiled lane masks by set bits: {required}")

for required in [
    "offer_lanes.next_set_from(",
    "next_preferred_lane_in_lane_set(",
]:
    if required not in core:
        fail(f"offer hot path must walk compiled lane masks without empty-lane scans: {required}")

offer_tests = read("src/endpoint/kernel/core_offer_tests.rs")
for required in [
    "preview_offer_entry_evidence_defers_binding_poll_until_selected_scope",
    "poll_binding_for_offer_polls_only_selected_lane_for_unbuffered_generic_mask",
    "poll_binding_for_offer_polls_authoritative_demux_lane_when_current_lane_is_excluded",
]:
    if required not in offer_tests:
        fail(f"offer hot path missing behavior proof for compiled lane/evidence use: {required}")

for required in [
    "role_runtime_table_view_route_dense_by_scope_slot_maps_to_expected_row",
    "role_runtime_table_view_control_by_eff_contains_control_descriptors",
]:
    if required not in role_image_source:
        fail(f"compiled runtime table view missing behavior proof: {required}")

for required in [
    "try_poll_route_decision_immediate(",
    "offer_lanes.next_set_from(",
    "ingest_scope_evidence_for_offer(",
]:
    if required not in offer:
        fail(f"offer frontier must use compiled lane-set hot-path helpers: {required}")

if re.search(
    r"while\s+lane_idx\s+<\s+(?:logical_lane_count|lane_limit)\s*\{[^{}]*(?:!\s*)?(?<!active_)offer_lanes\.contains",
    offer,
    re.S,
):
    fail("offer frontier must not scan empty lanes looking for offer_lanes membership")

for forbidden in [
    "fallback route",
    "repair route",
    "absorb mismatch",
    "guess route",
    "infer route",
]:
    if forbidden in core or forbidden in offer:
        fail(f"offer/route hot path retained forbidden repair or inference vocabulary: {forbidden}")

for required in [
    "struct RoleImage",
    "pub(crate) struct RoleImageRef",
    "pub(crate) struct RoleImageSource",
    "pub(crate) struct RoleFacts",
    "image: RoleImageRef",
    "image: &'static RoleImage",
    "stamp: ProgramStamp",
    "facts: RoleFacts",
    "source: RoleImageSource",
    "const fn source(&self) -> RoleImageSource",
    "pub(crate) const fn source(&self) -> RoleImageSource",
    "fn footprint(self) -> RoleFootprint",
    "RoleImage::new(",
    "RoleImageSource::new(Self::summary)",
    "&ValidatedRoleImage::<Steps, ROLE>::IMAGE",
]:
    if required not in role_program:
        fail(f"RoleProgram is not a compact verified descriptor handle: {required}")
for forbidden in [
    "summary: &'static LoweringSummary",
    "const fn summary(",
    "pub(crate) const fn summary(",
    "program.image.summary()",
    "\n    counts: RoleLoweringCounts,\n",
    "program.summary.role_lowering_counts::<ROLE>()",
    "let counts = program.image.summary().role_lowering_counts::<ROLE>();",
    "footprint: program.facts.footprint(counts)",
    "RoleImage::new::<ROLE>(validated_program_summary::<Steps>())",
]:
    if forbidden in role_program:
        fail(f"RoleProgram retained old lowering-summary witness shape: {forbidden}")

program_image = read("src/global/compiled/images/program.rs")
for required in [
    "pub(crate) struct RouteControlRecord",
    "pub(crate) struct CompiledProgramFacts",
    "route_controller_role",
    "route_controller(",
    "ControlSemanticsTable",
]:
    if required not in program_image:
        fail(f"compiled program image is not the route/control facts authority: {required}")

print("compiled descriptor authority check passed")
PY
