#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../../.." && pwd)"
LEAN_BIN="${LEAN_BIN:-lean}"
"$LEAN_BIN" --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
"$LEAN_BIN" "$HERE/RouteIndex.lean"
"$LEAN_BIN" "$HERE/RouteAncestry.lean"
"$LEAN_BIN" "$HERE/RetainedRange.lean"
topology_evidence="$(mktemp -d /tmp/hibana-route-topology.XXXXXX)"
trap 'rm -r -- "$topology_evidence"' EXIT
topology_log="$topology_evidence/lean.log"
chain_log="$topology_evidence/chain.log"
scope_log="$topology_evidence/scope.log"
"$LEAN_BIN" "$HERE/ScopeParent.lean" > "$scope_log" 2>&1
cat "$scope_log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$scope_log" 2
"$LEAN_BIN" "$HERE/CanonicalChain.lean" > "$chain_log" 2>&1
cat "$chain_log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$chain_log" 6
if ! "$LEAN_BIN" "$HERE/RouteTopology.lean" > "$topology_log" 2>&1; then
  cat "$topology_log" >&2
  exit 1
fi
cat "$topology_log"
bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$topology_log" 4
rustc +stable --edition=2024 -O "$HERE/source_shape_check.rs" \
  -o "$topology_evidence/source-shape-check"
"$topology_evidence/source-shape-check"
bash "$ROOT/.github/scripts/check_smt2.sh" "$HERE/RouteIndex.smt2"
bash "$ROOT/.github/scripts/check_smt2.sh" "$HERE/RouteAncestry.smt2"
bash "$ROOT/.github/scripts/check_smt2.sh" "$HERE/ScopeParent.smt2"
bash "$ROOT/.github/scripts/check_smt2.sh" "$HERE/RouteTopology.smt2"
bash "$ROOT/.github/scripts/check_smt2.sh" "$HERE/RetainedRange.smt2"
bash "$ROOT/.github/scripts/check_smt2.sh" "$HERE/CanonicalChain.smt2"
