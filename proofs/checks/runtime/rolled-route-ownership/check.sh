#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
PROOFS="$ROOT/proofs/checks/runtime/rolled-route-ownership"
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-rolled-route-proofs.XXXXXX)}
mkdir -p "$EVIDENCE"
EVIDENCE=$(cd "$EVIDENCE" && pwd -P)
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources "rolled-route trusted proof declarations" "$PROOFS"/*.lean
if [[ "$FAILED" -ne 0 ]]; then
  echo 'Untrusted declaration or proof bypass in supplemental proofs' >&2
  exit 1
fi
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
(cd "$ROOT/proofs/lean" && lake build Hibana.GlobalSemantics) > "$EVIDENCE/build.log" 2>&1
for proof in TraceValidity PhaseOwnership NestedReentry EligibleIngress ReentryAdmission ResetAlignment SendContinuation SendEntry NestedVisit SharedSuffix; do
  LEAN_PATH="$ROOT/proofs/lean/.lake/build/lib/lean" \
    lean -o "$EVIDENCE/$proof.olean" "$PROOFS/$proof.lean" > "$EVIDENCE/$proof.log" 2>&1
done
HIBANA_ROLL_COMPLETION_OBSERVATIONS="$EVIDENCE/completion.jsonl" \
  RUSTFLAGS="${RUSTFLAGS:-} --cfg hibana_repo_tests" \
  cargo +"${TOOLCHAIN:-stable}" test --locked --manifest-path "$ROOT/Cargo.toml" --lib \
  parallel_completion_observations_cover_every_bit_credit_and_entry \
  > "$EVIDENCE/completion-rust.log" 2>&1
LEAN_PATH="$EVIDENCE:$ROOT/proofs/lean/.lake/build/lib/lean" \
  lean --run "$PROOFS/CheckCompletion.lean" "$EVIDENCE/completion.jsonl" \
  "$EVIDENCE/completion.smt2" > "$EVIDENCE/completion-lean.log" 2>&1
z3 "$EVIDENCE/completion.smt2" > "$EVIDENCE/completion-z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-sequence \
  "$EVIDENCE/completion-z3.log" unsat unsat unsat sat sat
z3 "$PROOFS/Admission.smt2" > "$EVIDENCE/z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-admission "$EVIDENCE/z3.log"
z3 "$PROOFS/SendContinuation.smt2" > "$EVIDENCE/send-continuation-z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-counts "$EVIDENCE/send-continuation-z3.log" 5 3
z3 "$PROOFS/SendEntry.smt2" > "$EVIDENCE/send-entry-z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-sequence "$EVIDENCE/send-entry-z3.log" sat unsat sat unsat sat
z3 "$PROOFS/NestedVisit.smt2" > "$EVIDENCE/nested-visit-z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-sequence "$EVIDENCE/nested-visit-z3.log" sat unsat sat unsat sat unsat sat unsat
z3 "$PROOFS/SharedSuffix.smt2" > "$EVIDENCE/shared-suffix-z3.log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" smt-sequence "$EVIDENCE/shared-suffix-z3.log" sat unsat sat unsat sat unsat sat unsat sat sat sat sat unsat sat unsat
printf 'Rolled-route proofs passed: Lean 4.30.0, 10 proof files; 384 Rust/model comparisons; Z3 27 UNSAT obligations, 37 SAT premises/witnesses. Logs: %s\n' "$EVIDENCE"
