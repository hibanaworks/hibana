#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PROOFS="$ROOT/proofs/rolled-route-ownership"
TOOLCHAIN=leanprover/lean4:v4.30.0
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-rolled-route-proofs.XXXXXX)}
mkdir -p "$EVIDENCE"
source "$ROOT/.github/scripts/lib/hygiene_common.sh"
FAILED=0
check_absent '\b(sorry|admit|axiom|native_decide|unsafe)\b' \
  "rolled-route trusted proof declarations" "$PROOFS"/*.lean
if [[ "$FAILED" -ne 0 ]]; then
  echo 'Untrusted declaration or proof bypass in supplemental proofs' >&2
  exit 1
fi
python3 - "$ROOT" <<'PY'
from pathlib import Path
import hashlib, json, sys
root = Path(sys.argv[1])
record = json.loads((root / 'proofs/rolled-route-ownership/send-continuation-source.json').read_text())
expected_sources = {
    'src/global/typestate/cursor/scope_route/send_preview.rs',
    'tests/security_report_regressions/send_continuations.rs',
    'proofs/rolled-route-ownership/SendContinuation.lean',
    'proofs/rolled-route-ownership/SendContinuation.smt2',
}
if set(record['files']) != expected_sources:
    raise SystemExit('Send continuation proof source inventory changed')
for source, expected in record['files'].items():
    actual = hashlib.sha256((root / source).read_bytes()).hexdigest()
    if actual != expected:
        raise SystemExit('Send continuation proof source identity changed: ' + source)
expected_pre_edit_logs = {
    'proofs/rolled-route-ownership/validation/send-continuation-pre-edit-lean.log',
    'proofs/rolled-route-ownership/validation/send-continuation-pre-edit-z3.log',
}
if set(record['pre_edit_checks']) != expected_pre_edit_logs:
    raise SystemExit('Send continuation pre-edit evidence inventory changed')
for source, expected in record['pre_edit_checks'].items():
    if hashlib.sha256((root / source).read_bytes()).hexdigest() != expected:
        raise SystemExit('Send continuation pre-edit evidence changed: ' + source)
PY
(cd "$ROOT/proofs/lean" && lake +"$TOOLCHAIN" build Hibana.GlobalSemantics) > "$EVIDENCE/build.log" 2>&1
for proof in TraceValidity PhaseOwnership NestedReentry EligibleIngress ReentryAdmission ResetAlignment SendContinuation; do
  LEAN_PATH="$ROOT/proofs/lean/.lake/build/lib/lean" \
    lean +"$TOOLCHAIN" "$PROOFS/$proof.lean" > "$EVIDENCE/$proof.log" 2>&1
done
z3 "$PROOFS/Admission.smt2" > "$EVIDENCE/z3.log"
awk '
  /^sat$/ { sat++ }
  /^unsat$/ { unsat++ }
  /^unknown$|\(error/ { failed=1 }
  END { if (failed || sat != 16 || unsat != 10) exit 1 }
' "$EVIDENCE/z3.log"
z3 "$PROOFS/SendContinuation.smt2" > "$EVIDENCE/send-continuation-z3.log"
awk '
  /^sat$/ { sat++ }
  /^unsat$/ { unsat++ }
  /^unknown$|\(error/ { failed=1 }
  END { if (failed || sat != 3 || unsat != 2) exit 1 }
' "$EVIDENCE/send-continuation-z3.log"
printf 'Rolled-route proofs passed: Lean 4.30.0, 7 files; Z3 12 UNSAT obligations, 19 SAT premises/witnesses. Logs: %s\n' "$EVIDENCE"
