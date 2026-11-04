#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "$ROOT_DIR"
case "$#:${1:-}" in
  0:) build=true ;;
  1:--check) build=false ;;
  *) echo 'usage: build_protocol_checker.sh [--check]' >&2; exit 2 ;;
esac
out="$ROOT_DIR/target/protocol-checker"
mkdir -p "$ROOT_DIR/target"
work="$(mktemp -d "$ROOT_DIR/target/protocol-checker-build.XXXXXX")"
trap 'rm -r -- "$work"' EXIT
inputs() {
  git ls-files --cached --others --exclude-standard -z -- \
    proofs/lean .github/ci-tests/endpoint_conformance.lean .github/ci-tests/protocol_shapes.lean \
    .github/ci-tests/build_lean_serial.rs .github/ci-tests/lean_audit.rs \
    .github/ci-tests/lean_module.rs .github/ci-tests/supplemental_proofs.rs \
    .github/ci-tests/check_lean_proof_guard.rs \
    .github/scripts/lib/lean_audit_tool.sh .github/scripts/lib/supplemental_proofs_tool.sh \
    .github/scripts/check_shape_resumption_sensitivity.sh \
    .github/scripts/check_prefix_input_sensitivity.sh .github/scripts/build_protocol_checker.sh |
    xargs -0 shasum -a 256 --
}
inputs > "$work/source.sha256"
git rev-parse HEAD > "$work/revision.txt"
uname -sm > "$work/platform.txt"
check() {
  cmp "$work/source.sha256" "$out/source.sha256"
  cmp "$work/revision.txt" "$out/revision.txt"
  cmp "$work/platform.txt" "$out/platform.txt"
  (cd "$out" && shasum -a 256 --check binary.sha256 >&2)
}
if [[ "$build" == false ]]; then
  check
elif [[ -f "$out/source.sha256" && -f "$out/revision.txt" && -f "$out/platform.txt" ]] &&
    cmp -s "$work/source.sha256" "$out/source.sha256" &&
    cmp -s "$work/revision.txt" "$out/revision.txt" &&
    cmp -s "$work/platform.txt" "$out/platform.txt"; then
  check
else
  generated="$ROOT_DIR/target/lean-proof/ProtocolShapesGenerated.lean"
  mkdir -p "$(dirname "$generated")"
  cat .github/ci-tests/endpoint_conformance.lean .github/ci-tests/protocol_shapes.lean > "$generated"
  bash .github/scripts/lib/lean_audit_tool.sh build-serial \
    "$ROOT_DIR/proofs/lean" --native Hibana.SourceOccurrenceTrace >&2
  (cd proofs/lean && lake build protocolShapes) >&2
  cp proofs/lean/.lake/build/bin/protocol-shapes "$work/protocol-shapes"
  "$work/protocol-shapes" --check-guarded-reentry > "$work/guarded-reentry.log"
  cat "$work/guarded-reentry.log" >&2
  # Model mutations belong to this oracle's build, before any population runs.
  bash .github/scripts/check_shape_resumption_sensitivity.sh >&2
  bash .github/scripts/check_prefix_input_sensitivity.sh >&2
  cp target/shape-resumption-sensitivity/mutants.tsv "$work/resumption-controls.tsv"
  cp target/prefix-input-sensitivity/results.tsv "$work/input-controls.tsv"
  inputs > "$work/source-after.sha256"
  cmp "$work/source.sha256" "$work/source-after.sha256"
  rm "$work/source-after.sha256"
  (cd "$work" && shasum -a 256 protocol-shapes guarded-reentry.log \
    resumption-controls.tsv input-controls.tsv > binary.sha256)
  if [[ -e "$out" ]]; then rm -r -- "$out"; fi
  mv "$work" "$out"
  trap - EXIT
fi
printf '%s\n' "$out/protocol-shapes"
