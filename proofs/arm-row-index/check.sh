#!/bin/sh
set -eu
ROOT=/workspace/scratch/0915e8fbff81
cd "$ROOT/hibana-arm-row-index"
"$ROOT/tools/lean/lean-4.30.0-linux/bin/lean" proofs/arm-row-index/ArmRowIndex.lean > proofs/arm-row-index/lean.log 2>&1
/tmp/hibana-quic-proof-venv/bin/python -u proofs/arm-row-index/check_arm_row_index.py > proofs/arm-row-index/z3.log 2>&1
printf 'Lean and Z3 arm-row equivalence gates passed\n'
