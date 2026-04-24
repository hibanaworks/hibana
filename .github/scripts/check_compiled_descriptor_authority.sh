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

role_program = read("src/global/role_program.rs")
for required in [
    "struct ProjectedRoleImage",
    "pub(crate) struct RoleImageRef",
    "pub(crate) struct RoleFacts",
    "image: RoleImageRef",
    "image: &'static ProjectedRoleImage",
    "facts: RoleFacts",
    "fn footprint(self) -> RoleFootprint",
    "ProjectedRoleImage::new::<ROLE>(validated_program_summary::<Steps>())",
    "&ValidatedRoleImage::<Steps, ROLE>::IMAGE",
]:
    if required not in role_program:
        fail(f"RoleProgram is not a compact verified descriptor handle: {required}")
for forbidden in [
    "\n    counts: RoleLoweringCounts,\n",
    "program.summary.role_lowering_counts::<ROLE>()",
    "let counts = program.image.summary().role_lowering_counts::<ROLE>();",
    "footprint: program.facts.footprint(counts)",
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
