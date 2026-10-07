#!/usr/bin/env bash
set -euo pipefail
proof_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
proof_evidence=${1:-$(mktemp -d /tmp/hibana-event-admission-proof.XXXXXX)}
mkdir -p "$proof_evidence"
lean +leanprover/lean4:v4.30.0 "$proof_dir/ImmutableAdmission.lean" > "$proof_evidence/lean.log" 2>&1
cat > "$proof_evidence/lean.expected" <<'EOF'
'Hibana.ImmutableAdmission.immutable_read_reuse_preserves_live_observation' depends on axioms: [propext]
'Hibana.ImmutableAdmission.missing_row_rejects_without_live_observation' depends on axioms: [propext]
'Hibana.ImmutableAdmission.identity_mismatch_rejects_without_live_observation' depends on axioms: [propext]
'Hibana.ImmutableAdmission.later_admission_uses_its_current_live_check' depends on axioms: [propext]
EOF
diff -u "$proof_evidence/lean.expected" "$proof_evidence/lean.log"
z3 "$proof_dir/Admission.smt2" > "$proof_evidence/z3.log"
printf 'sat\nunsat\nunsat\nunsat\nunsat\nunsat\nsat\nsat\n' > "$proof_evidence/z3.expected"
diff -u "$proof_evidence/z3.expected" "$proof_evidence/z3.log"
printf 'Immutable event admission: Lean 4 theorems; Z3 5 UNSAT, 3 SAT premises/witnesses. Evidence: %s\n' "$proof_evidence"
