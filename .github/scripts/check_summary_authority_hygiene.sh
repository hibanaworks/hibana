#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n -U "${pattern}" "$@"; then
    echo "summary authority hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

check_absent_outside() {
  local pattern="$1"
  local label="$2"
  shift 2
  local globs=("-g" "!**/tests.rs" "-g" "!**/*_tests.rs" "-g" "!**/test_support/**")
  local exclude
  for exclude in "$@"; do
    globs+=("-g" "!${exclude}")
  done
  if rg -n -U "${pattern}" src "${globs[@]}"; then
    echo "summary authority hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

check_required() {
  local pattern="$1"
  local label="$2"
  local path="$3"
  if ! rg -n -F "${pattern}" "${path}" >/dev/null; then
    echo "summary authority hygiene violation: ${label}" >&2
    FAILED=1
  fi
}

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
    echo "summary authority hygiene violation: legacy typestate lowering owner still present -> ${forbidden_path}" >&2
    FAILED=1
  fi
done

check_absent_outside \
  "CompiledProgramImage::scan_const\\(" \
  "raw summary scans escaped Program compile layer" \
  "src/g.rs" \
  "src/global/program/source.rs"

check_absent_outside \
  "SOURCE\\.eff_list\\(" \
  "raw EffList lowering source escaped Program compile layer" \
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
  "let _ = const { validate_choreography::<Steps>() };" \
  "project must force projection validation before role image escape" \
  src/g.rs

check_required \
  "if let Some(error) = source_data.error() {" \
  "Program must reject invalid choreography terms before role image escape" \
  src/g.rs

check_required \
  "let source = source_data.eff_list();" \
  "Program must remain the only raw EffList owner for resident image generation" \
  src/g.rs

check_required \
  "crate::global::compiled::lowering::CompiledProgramImage::scan_const_with_lookup(" \
  "Program must remain the resident program image-generation owner" \
  src/g.rs

check_required \
  "crate::global::compiled::lowering::ProgramSourceLookup::new(" \
  "Program-owned overflow lookup must stay tied to the validated Program source" \
  src/g.rs

check_required \
  "RoleImageSource::new(" \
  "RoleProgram must bind resident role image source to the validated compiled program image" \
  src/g.rs

check_required \
  "RoleImageSource::new(Self::program_image)" \
  "resident role image source must resolve through the Program-owned role image owner" \
  src/g.rs

check_required \
  "CompiledProgramRef::resident(" \
  "RoleProgram must construct a resident compiled program reference before attach" \
  src/g.rs

check_absent \
  "write_clone_to|MaybeUninit::<CompiledProgramImage>|: &'static CompiledProgramImage|pub\\(crate\\) const fn summary\\(&self\\)" \
  "resident compiled program images must not be cloned or exposed through RoleProgram as secondary handles" \
  src/global/role_program.rs src/global/role_program

check_absent \
  "write_clone_to|MaybeUninit::<CompiledProgramImage>" \
  "compiled descriptor owners must borrow resident images, not clone them into attach storage" \
  src/global/compiled

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "summary authority hygiene check passed"
