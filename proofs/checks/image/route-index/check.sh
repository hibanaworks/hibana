#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../../.." && pwd)"
LEAN_BIN="${LEAN_BIN:-lean}"
"$LEAN_BIN" --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
"$LEAN_BIN" "$HERE/RouteIndex.lean"
"$LEAN_BIN" "$HERE/RouteAncestry.lean"
bash "$ROOT/.github/scripts/check_smt2.sh" "$HERE/RouteIndex.smt2"
bash "$ROOT/.github/scripts/check_smt2.sh" "$HERE/RouteAncestry.smt2"
