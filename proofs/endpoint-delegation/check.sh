#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
export TOOLCHAIN="${TOOLCHAIN:-stable}"
PROOFS="$ROOT/proofs/endpoint-delegation"
MODEL="$ROOT/proofs/lean/Hibana/EndpointOwnership.lean"
MODEL_PATH="$ROOT/proofs/lean/.lake/build/lib/lean"
EVIDENCE=${1:-$(mktemp -d /tmp/hibana-delegation-projection.XXXXXX)}
mkdir -p "$EVIDENCE"
EVIDENCE=$(cd "$EVIDENCE" && pwd)
cd "$ROOT"
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
# The standalone model has its own tests. Core-only repository fixtures must
# not be enabled in this independently compiled package.
source "$ROOT/.github/scripts/repo_rustflags.sh"
hibana_disable_repo_tests_cfg
source "$ROOT/.github/scripts/lib/proof_audit.sh"
FAILED=0
check_trusted_proof_sources 'endpoint delegation projection' "$MODEL" "$PROOFS/Claims.lean" "$PROOFS/Knowledge.lean" "$PROOFS/Issuance.lean" "$PROOFS/Causality.lean" "$PROOFS/ReceiptWait.lean"
[[ "$FAILED" -eq 0 ]]
lean --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
cargo +"$TOOLCHAIN" fmt --manifest-path "$PROOFS/compiler/Cargo.toml" --check
cargo +"$TOOLCHAIN" test --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --lib > "$EVIDENCE/rust-tests.log" 2>&1
cargo +"$TOOLCHAIN" clippy --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --all-targets -- -D warnings > "$EVIDENCE/clippy.log" 2>&1
cargo +"$TOOLCHAIN" check --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --lib --no-default-features > "$EVIDENCE/no-std-host.log" 2>&1
cargo +"$TOOLCHAIN" check --manifest-path "$PROOFS/compiler/Cargo.toml" --locked --lib --no-default-features --target thumbv6m-none-eabi > "$EVIDENCE/no-std-thumb.log" 2>&1
cargo +"$TOOLCHAIN" run --manifest-path "$PROOFS/compiler/Cargo.toml" --locked -- "$EVIDENCE/Generated.lean" > "$EVIDENCE/export.log" 2>&1
check_trusted_proof_sources 'generated endpoint delegation projection' "$EVIDENCE/Generated.lean"
[[ "$FAILED" -eq 0 ]]
(
  cd "$ROOT/proofs/lean"
  lake build Hibana.EndpointOwnership
) > "$EVIDENCE/lean-model-build.log" 2>&1
LEAN_PATH="$MODEL_PATH" lean "$PROOFS/Claims.lean" > "$EVIDENCE/lean-model.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-model.log" 14
LEAN_PATH="$EVIDENCE:$MODEL_PATH" lean -o "$EVIDENCE/Knowledge.olean" "$PROOFS/Knowledge.lean" > "$EVIDENCE/lean-knowledge.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-knowledge.log" 3
LEAN_PATH="$EVIDENCE:$MODEL_PATH" lean -o "$EVIDENCE/Causality.olean" "$PROOFS/Causality.lean" > "$EVIDENCE/lean-causality.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-causality.log" 4
lean -o "$EVIDENCE/Issuance.olean" "$PROOFS/Issuance.lean" > "$EVIDENCE/lean-issuance.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-issuance.log" 4
lean -j1 "$PROOFS/ReceiptWait.lean" > "$EVIDENCE/lean-receipt-wait.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-receipt-wait.log" 3
LEAN_PATH="$EVIDENCE:$MODEL_PATH" lean "$EVIDENCE/Generated.lean" > "$EVIDENCE/lean-generated.log" 2>&1
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$EVIDENCE/lean-generated.log" 193
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/Ownership.smt2" > "$EVIDENCE/z3.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/Issuance.smt2" > "$EVIDENCE/z3-issuance.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/Causality.smt2" > "$EVIDENCE/z3-causality.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/Reentry.smt2" > "$EVIDENCE/z3-reentry.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/ReentryKnowledge.smt2" > "$EVIDENCE/z3-reentry-knowledge.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/PendingSend.smt2" > "$EVIDENCE/z3-pending-send.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/OwnedEntry.smt2" > "$EVIDENCE/z3-owned-entry.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/BorrowedPredecessor.smt2" > "$EVIDENCE/z3-borrowed-predecessor.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/OwnedEnvelope.smt2" > "$EVIDENCE/z3-owned-envelope.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/ReceiptWait.smt2" > "$EVIDENCE/z3-receipt-wait.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/SourceUseCut.smt2" > "$EVIDENCE/z3-source-use-cut.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/RollBoundary.smt2" > "$EVIDENCE/z3-roll-boundary.log"
bash "$ROOT/.github/scripts/check_smt2.sh" "$PROOFS/ViewGeneration.smt2" > "$EVIDENCE/z3-view-generation.log"
printf 'Actor view generations: Z3 4 UNSAT, 1 rejected-rule SAT, 1 positive SAT. Evidence: %s\n' "$EVIDENCE/z3-view-generation.log"
printf 'Receipt wait frontier: Lean 3 general rules; Z3 2 UNSAT, 1 rejected-rule SAT, 1 positive SAT. Evidence: %s\n' "$EVIDENCE/z3-receipt-wait.log"
printf 'Source resource use cut: Z3 3 UNSAT, 3 positive SAT witnesses. Evidence: %s\n' "$EVIDENCE/z3-source-use-cut.log"
printf 'Reentry dependency cut and choice reset: Z3 2 UNSAT, 4 counterexample SAT. Evidence: %s\n' "$EVIDENCE/z3-reentry.log"
printf 'Resource-dependent reentry origins: Z3 3 UNSAT, 2 witnesses. Evidence: %s\n' "$EVIDENCE/z3-reentry-knowledge.log"
printf 'Pending-send admission boundary: Z3 3 UNSAT, 2 witnesses. Evidence: %s\n' "$EVIDENCE/z3-pending-send.log"
printf 'Explicit roll boundaries: Z3 6 UNSAT, 3 SAT witnesses. Evidence: %s\n' "$EVIDENCE/z3-roll-boundary.log"
printf 'Endpoint ownership projection: Rust tests passed; 17 projection/knowledge, 4 issuance and 4 causal-path general Lean theorems, 193 generated; ownership Z3 7 UNSAT, 2 counterexample SAT, 1 positive SAT; issuance Z3 3 UNSAT, 1 counterexample SAT, 1 positive SAT; causality Z3 3 UNSAT, 3 counterexample SAT, 1 independent-visit SAT. Evidence: %s\n' "$EVIDENCE"
