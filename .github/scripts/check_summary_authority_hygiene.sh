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
  local globs=()
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

check_absent \
  "impl[[:space:]]+TypestateProgramView[[:space:]]+for[[:space:]]+&EffList" \
  "emit_walk must not keep a raw EffList typestate view" \
  src/global/typestate/emit_walk.rs

check_absent \
  "EffList::(as_slice|scope_markers|policy_at|control_spec_at)\\(" \
  "emit_walk must not read raw EffList after summary generation" \
  src/global/typestate/emit_walk.rs

check_absent_outside \
  "LoweringSummary::scan_const\\(" \
  "raw summary scans escaped Program compile layer" \
  "src/global/program.rs"

check_absent_outside \
  "SOURCE\\.eff_list\\(" \
  "raw EffList lowering source escaped Program compile layer" \
  "src/global/program.rs" \
  "src/global/const_dsl.rs"

check_required \
  "let summary = LoweringSummary::scan_const(<Steps as BuildProgramSource>::SOURCE.eff_list());" \
  "Program must remain the summary-generation owner" \
  src/global/program.rs

check_required \
  "impl TypestateProgramView for LoweringView<'_> {" \
  "emit_walk must keep LoweringView as the summary-backed walker authority" \
  src/global/typestate/emit_walk.rs

check_required \
  "summary.view()," \
  "typestate emit must feed summary-backed views into emit_walk" \
  src/global/typestate/emit.rs

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "summary authority hygiene check passed"
