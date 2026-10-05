#!/usr/bin/env bash
set -euo pipefail
proof_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
proof_evidence=${1:-$(mktemp -d /tmp/hibana-recv-census-proofs.XXXXXX)}
mkdir -p "$proof_evidence"
(cd "$proof_dir/../.." && shasum -a 256 -c "$proof_dir/sources.sha256") > "$proof_evidence/source-check.log"
if rg -n '\b(sorry|admit|native_decide)\b|^[[:space:]]*axiom[[:space:]]' "$proof_dir/Census.lean"; then
    printf 'Untrusted supplemental proof declaration\n' >&2
    exit 1
fi
lean +leanprover/lean4:v4.30.0 "$proof_dir/Census.lean" > "$proof_evidence/lean.log" 2>&1
cat > "$proof_evidence/lean.expected" <<'EOF'
'Hibana.RecvLaneCensus.fold_exact' depends on axioms: [propext]
'Hibana.RecvLaneCensus.census_exact' depends on axioms: [propext]
'Hibana.RecvLaneCensus.ordered_transport_polls_agree' depends on axioms: [propext, Quot.sound]
'Hibana.RecvLaneCensus.census_has_actual_eligible_row' depends on axioms: [propext, Quot.sound]
'Hibana.RecvLaneCensus.wire_lane_word_bounds' depends on axioms: [propext]
EOF
diff -u "$proof_evidence/lean.expected" "$proof_evidence/lean.log"
z3 "$proof_dir/Census.smt2" > "$proof_evidence/z3.log"
printf 'unsat\nunsat\nunsat\nunsat\nunsat\nsat\n' > "$proof_evidence/z3.expected"
diff -u "$proof_evidence/z3.expected" "$proof_evidence/z3.log"
printf 'Receive lane census passed: 5 Lean theorems; Z3 5 UNSAT and 1 SAT witness. Logs: %s\n' "$proof_evidence"
