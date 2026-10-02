#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
REPORT=${1:?Pass the extracted hibana-core-handoff-20261002 directory}
EVIDENCE=${2:-$(mktemp -d /tmp/hibana-route-repro-evidence.XXXXXX)}
mkdir -p "$EVIDENCE"
# These are the handoff's pinned carrier and cooperative executor, not core
# dependencies. The report includes their retrieval instructions and hashes.
test "$(shasum -a 256 "$REPORT/repro/support/carrier.rs" | awk '{print $1}')" = 9ac4397f66cca19cd5ba90ba464a3750823b3b6088c0af40683328399b2f5600
test "$(shasum -a 256 "$REPORT/repro/support/runtime.rs" | awk '{print $1}')" = 32cf0623837b35a681b1dab5ac50069885cafb8c7f3098323cd7ca5b61b622ea
TARGET=$(mktemp -d /tmp/hibana-route-repro-target.XXXXXX)
trap 'cargo +1.95.0 clean --target-dir "$TARGET"' EXIT
CARGO_TARGET_DIR="$TARGET" CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 \
  cargo +1.95.0 build --manifest-path "$ROOT/Cargo.toml" --lib > "$EVIDENCE/build.log" 2>&1
for repro in review_adjacent_routes review_adjacent_roll review_wrong_branch_commit review_nested_reentry five_binary_phases_trace tls_initial_three_way_failure; do
  rustc +1.95.0 --edition=2024 "$REPORT/repro/$repro.rs" \
    --extern "hibana=$TARGET/debug/libhibana.rlib" -L "dependency=$TARGET/debug/deps" \
    -o "$TARGET/$repro" > "$EVIDENCE/$repro-build.log" 2>&1
  perl -e 'alarm 30; exec @ARGV' "$TARGET/$repro" > "$EVIDENCE/$repro.log" 2>&1
  printf 'PASS %s\n' "$repro"
done
