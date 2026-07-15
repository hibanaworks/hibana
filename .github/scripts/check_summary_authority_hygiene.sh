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
  "ProgramProjection must bind the resident program image in one owner" \
  src/g.rs

check_required \
  "const VALIDATION: () = {" \
  "ProgramProjection must keep choreography validation as program-level authority" \
  src/g.rs

check_required \
  "const SOURCE: ProgramSourceData<CAPACITY> = ProgramSourceData::lower::<Steps>();" \
  "ProgramProjection must lower and validate source shape before role image construction" \
  src/g.rs

check_required \
  "pub(super) const SOURCE_EFF_LIST:" \
  "ProgramProjection must remain the only raw EffList owner for resident image generation" \
  src/g.rs

check_required \
  "Self::SOURCE.eff_list();" \
  "ProgramProjection must borrow its sole EffList view from the validated source owner" \
  src/g.rs

check_required \
  "panic!(\"type tree and lowered source disagree\")" \
  "source lowering must fail closed when type-tree counts disagree with emitted rows" \
  src/g/source.rs

check_required \
  "let source = Self::SOURCE_EFF_LIST;" \
  "ProgramProjection must lower resident images from its owned EffList view" \
  src/g.rs

check_required \
  "crate::global::compiled::lowering::CompiledProgramImage::scan_const(source)" \
  "ProgramProjection must remain the const validation image-generation owner" \
  src/g.rs

check_required \
  "Self::IMAGE.validate_projection_program();" \
  "ProgramProjection validation must validate resident projection invariants" \
  src/g.rs

check_required \
  "crate::global::compiled::lowering::projection_error_all_roles(&Self::IMAGE, source)" \
  "ProgramProjection validation must validate every role through the program owner" \
  src/g.rs

check_required \
  "let () = Self::VALIDATION;" \
  "ProgramProjection compiled ref must be the validated program-ref boundary" \
  src/g/role_projection.rs

check_absent_multiline \
  "const fn validate_choreography<Steps>|validate_choreography::<Steps>\\(\\)" \
  "projection validation must not exist as a second project-boundary authority" \
  src/g.rs

check_required \
  "ProgramImageBytes" \
  "Program resident image must be compacted through private bucket storage" \
  src/g/role_projection.rs

ROLE_IMAGE_SOURCE_PATTERN='RoleImageSource'
ROLE_DEBUG_FACTS_PATTERN='RoleDebugFacts'
ROLE_DEBUG_FOOTPRINT_PATTERN='RoleDebugFootprint'
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
