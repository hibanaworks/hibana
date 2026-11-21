#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/hibana-implementation-proofs.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
rustc +"${TOOLCHAIN:-stable}" --edition=2024 -D warnings -O \
  "$ROOT/.github/ci-tests/implementation_proofs.rs" -o "$scratch/run"
"$scratch/run" "$ROOT" "$scratch"
observations="$scratch/alternate-visit-polls.txt"
: > "$observations"
HIBANA_VISIT_WAIT_OBSERVATIONS="$observations" CARGO_BUILD_JOBS=1 \
  cargo +"${TOOLCHAIN:-stable}" test --locked --manifest-path "$ROOT/Cargo.toml" \
  --test nested_parallel_reentry alternate_visit -- --test-threads=1
HIBANA_VISIT_WAIT_OBSERVATIONS="$observations" CARGO_BUILD_JOBS=1 \
  cargo +"${TOOLCHAIN:-stable}" test --locked --manifest-path "$ROOT/Cargo.toml" \
  --test route_branch_send a_fresh_send_can_wait_for_a_choice_that_skips_its_own_predecessor -- --exact
ELAN_TOOLCHAIN="$(cat "$ROOT/proofs/lean/lean-toolchain")" \
  LEAN_PATH="$scratch:$ROOT/proofs/lean/.lake/build/lib/lean" \
  lean -DwarningAsError=true --run "$ROOT/.github/ci-tests/alternate_visit_wait.lean" "$observations"

# Compare the two actor-local write counts with the source-derived reset on
# the same public Endpoint calls, including the independently retained child.
(
  source "$ROOT/.github/scripts/repo_rustflags.sh"
  hibana_enable_repo_tests_cfg
  export HIBANA_REENTRY_WRITE_PROOF="$scratch/RetainedChildWrites.lean"
  CARGO_BUILD_JOBS=1 cargo +"${TOOLCHAIN:-stable}" test --locked --manifest-path "$ROOT/Cargo.toml" \
    --lib g::role_projection::tests::reentry_parallel::a_parent_reset_counts_the_actual_credited_child_writes -- --exact
  ELAN_TOOLCHAIN="$(cat "$ROOT/proofs/lean/lean-toolchain")" \
    LEAN_PATH="$ROOT/proofs/lean/.lake/build/lib/lean" \
    lean -DwarningAsError=true "$HIBANA_REENTRY_WRITE_PROOF"
)
