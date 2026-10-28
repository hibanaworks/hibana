#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
proof_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$proof_dir/../../../.." && pwd)
evidence=${1:-$(mktemp -d /tmp/hibana-operation-index.XXXXXX)}
mkdir -p "$evidence"
if ! lean "$proof_dir/Lookup.lean" > "$evidence/lean.log" 2>&1; then
  cat "$evidence/lean.log" >&2
  exit 1
fi
cat "$evidence/lean.log"
cat > "$evidence/lean.expected" <<'EOF'
'Hibana.OperationIndex.lower_partition' depends on axioms: [propext, Quot.sound]
'Hibana.OperationIndex.candidate_range_exact' depends on axioms: [propext, Quot.sound]
'Hibana.OperationIndex.live_candidates_exact' depends on axioms: [propext, Quot.sound]
'Hibana.OperationIndex.message_key_exact' depends on axioms: [propext, Quot.sound]
'Hibana.OperationIndex.operation_key_exact' depends on axioms: [propext, Quot.sound]
'Hibana.OperationIndex.source_restriction_preserves_exact_candidates' depends on axioms: [propext, Quot.sound]
'Hibana.OperationIndex.completion_prefilter_preserves_admission' depends on axioms: [propext]
EOF
diff -u "$evidence/lean.expected" "$evidence/lean.log"
if ! lean "$proof_dir/RowPrefix.lean" > "$evidence/row-prefix.log" 2>&1; then
  cat "$evidence/row-prefix.log" >&2
  exit 1
fi
cat "$evidence/row-prefix.log"
bash "$root/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$evidence/row-prefix.log" 2
bash "$root/.github/scripts/check_smt2.sh" "$proof_dir/Lookup.smt2"
bash "$root/.github/scripts/check_smt2.sh" "$proof_dir/RowPrefix.smt2"
