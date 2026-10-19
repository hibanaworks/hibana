#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export TOOLCHAIN="${TOOLCHAIN:-stable}" CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
test "$(uname -s)" = Linux
test "$(uname -m)" = x86_64
command -v valgrind

build="$ROOT/target/runtime-instructions"
results="$build/results"
mkdir -p "$results"
rustc +"$TOOLCHAIN" --version > "$results/rustc.txt"
valgrind --version > "$results/valgrind.txt"
cargo +"$TOOLCHAIN" build --locked --release --bench runtime_cost --target-dir "$build"

executables=()
for candidate in "$build"/release/deps/runtime_cost-*; do
  if [[ -f "$candidate" && -x "$candidate" ]]; then
    executables+=("$candidate")
  fi
done
test "${#executables[@]}" -eq 1
for case in seq par; do
  for tail in 8 256; do
    for run in 1 2; do
      HIBANA_PERF_SAMPLES=1 HIBANA_PERF_CASE="$case" HIBANA_PERF_TAIL="$tail" \
        valgrind --tool=callgrind --instr-atstart=no --cache-sim=no \
          --callgrind-out-file="$results/$case-$tail-$run.callgrind" \
          "${executables[0]}" > "$results/$case-$tail-$run.stdout" \
          2> "$results/$case-$tail-$run.stderr"
    done
  done
done
rustc +"$TOOLCHAIN" --edition=2024 -D warnings -O \
  .github/ci-tests/runtime_instructions.rs -o "$build/read-instructions"
"$build/read-instructions" "$results" | tee "$results/counts.tsv"
