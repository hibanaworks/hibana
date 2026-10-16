#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT/proofs/lean"
lake build Hibana.StaticProjectability
lake env lean ../roll-reentry-selectors/Check.lean
cd "$ROOT"
bash .github/scripts/check_smt2.sh proofs/roll-reentry-selectors/selectors.smt2
