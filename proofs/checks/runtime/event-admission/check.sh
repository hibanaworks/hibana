#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
proof_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
proof_evidence=${1:-$(mktemp -d /tmp/hibana-event-admission-proof.XXXXXX)}
mkdir -p "$proof_evidence"
lean --version | bash "$(cd "$(dirname "$0")/../../../.." && pwd)/.github/scripts/lib/supplemental_proofs_tool.sh" version
lean "$proof_dir/ImmutableAdmission.lean" 2>&1 | tee "$proof_evidence/lean.log"
cat > "$proof_evidence/lean.expected" <<'EOF'
'Hibana.ImmutableAdmission.immutable_read_reuse_preserves_live_observation' depends on axioms: [propext]
'Hibana.ImmutableAdmission.missing_row_rejects_without_live_observation' depends on axioms: [propext]
'Hibana.ImmutableAdmission.identity_mismatch_rejects_without_live_observation' depends on axioms: [propext]
'Hibana.ImmutableAdmission.later_admission_uses_its_current_live_check' depends on axioms: [propext]
'Hibana.ImmutableAdmission.scope_filter_before_admission_preserves_first_candidate' depends on axioms: [propext]
'Hibana.ImmutableAdmission.excluded_context_needs_no_admission' depends on axioms: [propext]
EOF
diff -u "$proof_evidence/lean.expected" "$proof_evidence/lean.log"
printf 'Immutable event admission: 6 Lean theorems. Evidence: %s\n' "$proof_evidence"
