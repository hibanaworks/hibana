#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
cd "$ROOT"
cargo +"${TOOLCHAIN:-stable}" run --quiet --locked -p hibana-ci-tests --bin dispatch-model
bash .github/scripts/check_smt2.sh proofs/checks/image/dispatch-subtree/Dispatch.smt2
