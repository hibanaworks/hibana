#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export TOOLCHAIN="${TOOLCHAIN:-stable}" CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
build="$ROOT/target/runtime-work"
results="$build/results"
mkdir -p "$results"
source_hashes() {
  git ls-files --cached --others --exclude-standard -z | xargs -0 shasum -a 256 --
}
source_hashes > "$results/source-before.sha256"
git rev-parse HEAD > "$results/revision.txt"
rustc +"$TOOLCHAIN" -vV > "$results/rustc.txt"
cargo +"$TOOLCHAIN" test --locked --manifest-path .github/ci-tests/Cargo.toml \
  --bin runtime-work --target-dir "$build" > "$results/reader-tests.log" 2>&1
cargo +"$TOOLCHAIN" build --locked --manifest-path .github/ci-tests/Cargo.toml \
  --bin runtime-work --target-dir "$build" > "$results/reader-build.log" 2>&1
reader="$build/debug/runtime-work"
# The instrumented core must also build without an OS or allocator.
for target in thumbv6m-none-eabi thumbv8m.main-none-eabihf; do
  RUSTFLAGS='--cfg hibana_profile' cargo +"$TOOLCHAIN" check --locked --lib \
    --no-default-features --target "$target" --target-dir "$build" \
    > "$results/no-std-$target.log" 2>&1
done
RUSTFLAGS='--cfg hibana_repo_tests --cfg hibana_profile' \
  cargo +"$TOOLCHAIN" test --locked --lib --no-run --message-format=json-render-diagnostics \
    > "$results/build.jsonl" 2> "$results/build.log"
binary="$("$reader" artifact "$results/build.jsonl")"
shasum -a 256 -- "$binary" "$reader" > "$results/binaries.sha256"
"$binary" global::event_program::work::tests --test-threads=1 \
  > "$results/counter-tests.log" 2>&1
"$binary" endpoint::runtime_work_tests::work_observation_preserves_live_endpoint_progress \
  --exact --test-threads=1 > "$results/observation-tests.log" 2>&1
"$binary" endpoint::runtime_work_tests::runtime_work_profile \
  --exact --nocapture --test-threads=1 > "$results/observations.log" 2>&1
"$reader" check "$results/observations.log" \
  "$ROOT/.github/measurement_snapshots/hibana-runtime-work.tsv" \
  | tee "$results/check.txt"
source_hashes > "$results/source-after.sha256"
cmp "$results/source-before.sha256" "$results/source-after.sha256"
shasum -a 256 --check "$results/binaries.sha256" > "$results/binaries-check.log"
