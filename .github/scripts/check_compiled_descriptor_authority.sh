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


def read_rs_tree(path: str) -> str:
    base = root / path
    if base.is_file():
        return read(path)
    chunks = []
    for child in sorted(base.rglob("*.rs")):
        rel = child.relative_to(root)
        if "tests" in rel.parts or child.name == "tests.rs" or child.name.endswith("_tests.rs"):
            continue
        chunks.append(child.read_text())
    return "\n".join(chunks)


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
        depth = 1
        idx = match.end()
        while idx < len(source) and depth:
            ch = source[idx]
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
            idx += 1
        if depth:
            fail("unterminated cfg(test) module")
        cursor = idx


for path in [
    "src/global/compiled/layout.rs",
    "src/global/compiled/materialize",
    "src/global/compiled/lowering/program_image_builder.rs",
    "src/global/compiled/lowering/program_tail_storage.rs",
    "src/global/compiled/lowering/role_image_builder.rs",
    "src/global/compiled/lowering/role_image_lowering.rs",
    "src/global/compiled/lowering/role_scope_storage.rs",
    "src/global/typestate/builder.rs",
    "src/global/typestate/emit.rs",
    "src/global/typestate/emit_route.rs",
    "src/global/typestate/emit_scope.rs",
    "src/global/typestate/emit_walk.rs",
    "src/global/typestate/registry.rs",
    "src/global/typestate/route_facts.rs",
]:
    if (root / path).exists():
        fail(f"legacy lowering/materialization owner still present: {path}")

cluster = strip_cfg_test_modules(
    read("src/control/cluster/core.rs") + "\n" + read_rs_tree("src/control/cluster/core")
)
rendezvous = strip_cfg_test_modules(
    read("src/rendezvous/core.rs") + "\n" + read_rs_tree("src/rendezvous/core")
)
port = strip_cfg_test_modules(read("src/rendezvous/port.rs"))
role_program = read("src/global/role_program.rs") + "\n" + read_rs_tree("src/global/role_program")
g_surface = read("src/g.rs")
projection_owner = role_program + "\n" + g_surface
role_image_owner = read("src/global/compiled/images/role.rs")
role_image = read("src/global/compiled/images/image.rs") + "\n" + read_rs_tree("src/global/compiled/images/image")
compiled_mod = read("src/global/compiled/mod.rs")
lowering_mod = read("src/global/compiled/lowering/mod.rs")

for forbidden in [
    "RoleLoweringScratch",
    "LoweringLeaseMode",
    "with_lowering_lease",
    "MaterializedRoleImage",
    "materialize_program_image_",
    "materialize_role_image_",
    "RoleImageSlice::from_raw(",
    "CompiledProgramRef::from_raw(",
    "CompiledProgramRef::from_",
    "scratch_reserved_bytes",
    "program_images",
    "role_images",
]:
    for path, source in [
        ("src/control/cluster/core.rs", cluster),
        ("src/rendezvous/core.rs", rendezvous),
        ("src/rendezvous/port.rs", port),
        ("src/global/compiled/images/image.rs", role_image),
        ("src/global/compiled/mod.rs", compiled_mod),
        ("src/global/compiled/lowering/mod.rs", lowering_mod),
    ]:
        if forbidden in source:
            fail(f"{path} retains transient attach/materialization primitive: {forbidden}")

for required in [
    "pub(crate) struct CompiledRoleImage",
    "program: CompiledProgramRef",
    "role: u8",
    "image: RoleImageRef",
    "pub(crate) const fn program(&self) -> CompiledProgramRef",
    "pub(crate) fn program_image(&self) -> &'static CompiledProgramImage",
]:
    if required not in role_image_owner:
        fail(f"CompiledRoleImage is not the resident role descriptor owner: {required}")

for required in [
    "pub struct RoleProgram<const ROLE: u8>",
    "struct ProjectionWitness(&'static crate::global::compiled::images::CompiledRoleImage)",
    "image: ProjectionWitness",
    "pub(crate) const fn role_program_from_image<const ROLE: u8>",
    "image: ProjectionWitness::new(image)",
    "role_program_from_image(role_projection_image::<ROLE, Steps>())",
]:
    if required not in projection_owner:
        fail(f"RoleProgram does not consume the resident g projection boundary before attach: {required}")

for required in [
    "struct RoleProjection<const ROLE: u8, Steps>",
    "impl<const ROLE: u8, Steps> RoleProjection<ROLE, Steps>",
    "const IMAGE: crate::global::compiled::images::CompiledRoleImage",
    "CompiledRoleImage::new(",
    "CompiledProgramRef::resident(",
]:
    if required not in g_surface:
        fail(f"g projection boundary does not own a resident CompiledRoleImage before attach: {required}")

for forbidden in [
    ": &'static CompiledProgramImage",
    "stamp: ProgramStamp,\n}",
    "pub(crate) const fn summary",
    "RoleProgram::new(validated_program_image",
]:
    if forbidden in role_program:
        fail(f"RoleProgram regressed to a summary/stamp-backed handle: {forbidden}")

for required in [
    "pub(crate) const fn from_resident(compiled: &'static CompiledRoleImage)",
    "program: compiled.program()",
    "resident: compiled",
    "RoleImageSlice::from_resident(compiled)",
]:
    haystack = role_image + "\n" + cluster
    if required not in haystack:
        fail(f"attach path must consume resident descriptor references only: {required}")

if "RoleDescriptorSource" in role_image:
    fail("RoleDescriptorSource must not exist; resident descriptors are the only attach input")

for required in [
    "let compiled = program.compiled_role_image();",
    "RoleImageSlice::from_resident(compiled)",
    "program.compiled_role_image().program()",
]:
    if required not in cluster:
        fail(f"SessionKit attach path is not resident-descriptor-first: {required}")

for path in [
    "src/control/cluster/core.rs",
    "src/control/cluster/core",
    "src/rendezvous/core.rs",
    "src/rendezvous/core",
    "src/rendezvous/port.rs",
    "src/endpoint/kernel/endpoint_init.rs",
    "src/endpoint/kernel/core.rs",
    "src/endpoint/kernel/core",
    "src/endpoint/kernel/offer.rs",
    "src/endpoint/kernel/offer",
]:
    source = strip_cfg_test_modules(read_rs_tree(path))
    for forbidden in [
        r"\bEffList\b",
        r"\bStepCons\b",
        r"\bStepNil\b",
        r"\bSeqSteps\b",
        r"\bRouteSteps\b",
        r"\bParSteps\b",
        r"interpret_eff_list\(",
    ]:
        if re.search(forbidden, source):
            fail(f"{path} reads raw choreography in runtime attach/hot path: {forbidden}")

print("compiled descriptor authority check passed")
PY
