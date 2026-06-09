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
role_projection_surface = read("src/g/role_projection.rs")
projection_owner = role_program + "\n" + g_surface + "\n" + role_projection_surface
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
    "role: u8",
    "image: &'static RoleImageRef",
    "image: &'static RoleImageRef,",
    "pub(crate) const fn program(&self) -> CompiledProgramRef",
    "self.image.program",
    "pub(crate) const fn role_image(&self) -> RoleImageRef",
]:
    if required not in role_image_owner:
        fail(f"CompiledRoleImage is not the resident role descriptor owner: {required}")

for required in [
    "pub struct RoleProgram<const ROLE: u8>",
    "image: &'static crate::global::compiled::images::CompiledRoleImage",
    "pub(crate) const fn role_program_from_image<const ROLE: u8>",
    "role_program_from_image(image)",
]:
    if required not in projection_owner:
        fail(f"RoleProgram does not consume the resident g projection boundary before attach: {required}")

if "ProjectionWitness" in projection_owner:
    fail("RoleProgram must not keep a wrapper around the resident CompiledRoleImage")

for required in [
    "struct RoleProjection<const ROLE: u8, Steps>",
    "impl<const ROLE: u8, Steps> RoleProjection<ROLE, Steps>",
    "const IMAGE: crate::global::compiled::images::CompiledRoleImage",
    "CompiledRoleImage::new(",
    "ProgramImageBlobStorage",
    "CompiledProgramRef::compact(",
]:
    if required not in role_projection_surface:
        fail(f"g projection boundary does not own a resident CompiledRoleImage before attach: {required}")

project_match = re.search(
    r"pub\(crate\) fn project<const ROLE: u8, Steps>\([^{}]*\)\s*->\s*crate::global::role_program::RoleProgram<ROLE>\s*where\s*Steps:\s*ProgramTerm,\s*\{(?P<body>.*?)\n\}",
    g_surface,
    re.S,
)
if project_match is None:
    fail("g project entry is not a recognizable resident projection boundary")

project_body = project_match.group("body")
role_validation = project_body.find("if ROLE >= ROLE_DOMAIN_SIZE")
role_dispatch = project_body.find("match ROLE {")
role_program_publication = project_body.find("role_program_from_image(image)")
if role_validation < 0 or role_dispatch < 0 or role_program_publication < 0:
    fail("g project entry must validate, dispatch to resident descriptors, and publish from the selected image")
if not (role_validation < role_dispatch < role_program_publication):
    fail("g project entry must validate the public role before selecting a resident descriptor image")
if '_ => panic!("{}", ROLE_INDEX_ERROR)' not in project_body:
    fail("g project entry must fail closed for unreachable out-of-domain descriptor arms")
for role in range(16):
    required = f"role_projection_image_for::<{role}, Steps>()"
    if required not in project_body:
        fail(f"g project entry must dispatch role {role} through a resident role descriptor arm")
for forbidden in [
    "RoleProjection::<ROLE, Steps>",
    "role_projection_image_for::<ROLE, Steps>()",
    "role_projection_image_for::<16",
    "_ => role_projection_image_for::<0, Steps>()",
]:
    if forbidden in project_body:
        fail(f"g project entry regressed to generic or out-of-domain projection instantiation: {forbidden}")

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
    "Self { resident: compiled }",
    "self.resident.program()",
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
