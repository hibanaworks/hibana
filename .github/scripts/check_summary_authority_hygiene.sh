#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

for forbidden_path in \
  src/global/typestate/emit.rs \
  src/global/typestate/emit_walk.rs \
  src/global/typestate/emit_scope.rs \
  src/global/typestate/emit_route.rs \
  src/global/typestate/builder.rs \
  src/global/typestate/registry.rs \
  src/global/typestate/route_facts.rs
do
  if [[ -e "${forbidden_path}" ]]; then
    echo "summary authority hygiene violation: forbidden typestate lowering owner still present -> ${forbidden_path}" >&2
    FAILED=1
  fi
done

check_absent_outside_tests \
  "CompiledProgramImage::scan_const\\(" \
  "raw summary scans outside Program compile layer" \
  "src/g.rs" \
  "src/global/program/source.rs"

check_absent_outside_tests \
  "SOURCE\\.eff_list\\(" \
  "raw EffList lowering source outside Program compile layer" \
  "src/g.rs" \
  "src/global/program/source.rs" \
  "src/global/const_dsl.rs"

check_required \
  "const IMAGE: crate::global::compiled::lowering::CompiledProgramImage = {" \
  "Program must bind the resident program image in one owner" \
  src/g.rs

check_required \
  "const fn validate_choreography<Steps>()" \
  "Program must keep projection validation as the public project boundary proof" \
  src/g.rs

check_required \
  "const { validate_choreography::<Steps>() };" \
  "project must force projection validation before role image construction" \
  src/g.rs

check_required \
  "if let Some(error) = source_data.error() {" \
  "Program must reject invalid choreography terms before role image construction" \
  src/g.rs

check_required \
  "let source = source_data.eff_list();" \
  "Program must remain the only raw EffList owner for resident image generation" \
  src/g.rs

check_required \
  "crate::global::compiled::lowering::CompiledProgramImage::scan_const(source)" \
  "Program must remain the const validation image-generation owner" \
  src/g.rs

check_required \
  "ProgramImageBytes" \
  "Program resident image must be compacted through private bucket storage" \
  src/g/role_projection.rs

ROLE_IMAGE_SOURCE_PATTERN='Role''Image''Source'
ROLE_DEBUG_FACTS_PATTERN='Role''Debug''Facts'
ROLE_DEBUG_FOOTPRINT_PATTERN='Role''Debug''Footprint'
check_absent_multiline \
  "\\b${ROLE_IMAGE_SOURCE_PATTERN}\\b|\\b${ROLE_DEBUG_FACTS_PATTERN}\\b|\\b${ROLE_DEBUG_FOOTPRINT_PATTERN}\\b|compiled_program_image\\(|program_image\\(|compact_blob_len\\(|largest_section_bytes\\(|write_lane_indices\\(" \
  "test/debug-only role source metadata, lowering-image backpointer, or measurement helper detected" \
  src/g src/global/role_program src/global/compiled/images/image/role_descriptor_ref.rs

check_required \
  "CompiledProgramRef::compact(" \
  "Program image bytes must construct a compact compiled program reference before attach" \
  src/global/compiled/images/image/blob_storage.rs

check_absent_multiline \
  "write_clone_to|MaybeUninit::<CompiledProgramImage>|: &'static CompiledProgramImage|pub\\(crate\\) const fn summary\\(&self\\)" \
  "resident compiled program images must not be cloned or exposed through RoleProgram as secondary handles" \
  src/global/role_program.rs src/global/role_program

check_absent_multiline \
  "write_clone_to|MaybeUninit::<CompiledProgramImage>" \
  "compiled descriptor owners must borrow resident images, not clone them into attach storage" \
  src/global/compiled

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "summary authority hygiene check passed"
