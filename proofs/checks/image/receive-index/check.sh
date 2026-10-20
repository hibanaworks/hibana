#!/usr/bin/env bash
set -euo pipefail
export ELAN_TOOLCHAIN=leanprover/lean4:v4.30.0
proof_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$proof_dir/../../../.." && pwd)
evidence=${1:-$(mktemp -d /tmp/hibana-receive-index.XXXXXX)}
mkdir -p "$evidence"
if ! lean "$proof_dir/Lookup.lean" > "$evidence/lean.log" 2>&1; then
  cat "$evidence/lean.log" >&2
  exit 1
fi
cat "$evidence/lean.log"
cat > "$evidence/lean.expected" <<'EOF'
'Hibana.ReceiveIndex.lower_partition' depends on axioms: [propext, Quot.sound]
'Hibana.ReceiveIndex.candidate_range_exact' depends on axioms: [propext, Quot.sound]
'Hibana.ReceiveIndex.live_candidates_exact' depends on axioms: [propext, Quot.sound]
EOF
diff -u "$evidence/lean.expected" "$evidence/lean.log"
bash "$root/.github/scripts/check_smt2.sh" "$proof_dir/Lookup.smt2"
