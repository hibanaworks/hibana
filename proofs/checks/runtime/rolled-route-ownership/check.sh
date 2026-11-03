#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
PROOFS="$ROOT/proofs/checks/runtime/rolled-route-ownership"
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-rolled-route-proofs.XXXXXX)}
mkdir -p "$EVIDENCE"
EVIDENCE=$(cd "$EVIDENCE" && pwd -P)
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
(cd "$ROOT/proofs/lean" && lake build Hibana.GlobalSemantics) > "$EVIDENCE/build.log" 2>&1
for proof in TraceValidity PhaseOwnership NestedReentry EligibleIngress ReentryAdmission ResetAlignment SendContinuation SendEntry NestedVisit SharedSuffix; do
  LEAN_PATH="$ROOT/proofs/lean/.lake/build/lib/lean" \
    bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" module "$PROOFS/$proof.lean" "$EVIDENCE/$proof.olean" > "$EVIDENCE/$proof.log" 2>&1
done
HIBANA_ROLL_COMPLETION_OBSERVATIONS="$EVIDENCE/completion.jsonl" \
  RUSTFLAGS="${RUSTFLAGS:-} --cfg hibana_repo_tests" \
  cargo +"${TOOLCHAIN:-stable}" test --locked --manifest-path "$ROOT/Cargo.toml" --lib \
  parallel_completion_observations_cover_every_bit_credit_and_entry \
  > "$EVIDENCE/completion-rust.log" 2>&1
LEAN_PATH="$EVIDENCE:$ROOT/proofs/lean/.lake/build/lib/lean" \
  lean --run "$PROOFS/CheckCompletion.lean" "$EVIDENCE/completion.jsonl" \
  > "$EVIDENCE/completion-lean.log" 2>&1
printf 'Rolled-route proofs passed: 10 Lean proof files; 384 Rust/model comparisons and both defect controls. Logs: %s\n' "$EVIDENCE"
