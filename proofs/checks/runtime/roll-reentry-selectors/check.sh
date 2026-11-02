#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
cd "$ROOT/proofs/lean"
lake build Hibana.StaticProjectability
lake env lean "$ROOT/proofs/checks/runtime/roll-reentry-selectors/Check.lean"
cd "$ROOT"
printf '%s\n' 'roll-reentry-selectors: Lean checks passed'
