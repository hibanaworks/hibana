#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"
# Every generated assignment owns its build directory.
unset CARGO_TARGET_DIR
if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo 'usage: check_protocol_ci.sh ASSIGNMENT [SEED]' >&2
  exit 2
fi
ASSIGNMENT="$(cargo +"${TOOLCHAIN:-stable}" run --quiet --locked --manifest-path .github/ci-tests/Cargo.toml --bin protocol-shape-partitions -- ci-case "$1")"
SELECTION="$(jq -er '.selection' <<< "$ASSIGNMENT")"
PARTITION="$(jq -r '.partition // empty' <<< "$ASSIGNMENT")"
BUDGET="$(jq -r '.budget // empty' <<< "$ASSIGNMENT")"
WINDOW="$(jq -r '.window // empty' <<< "$ASSIGNMENT")"
FIRST="$(jq -r '.first // empty' <<< "$ASSIGNMENT")"
COUNT="$(jq -r '.count // empty' <<< "$ASSIGNMENT")"
DEPTH="$(jq -r '.depth // empty' <<< "$ASSIGNMENT")"
SEED="${2:-}"
out="$ROOT_DIR/target/protocol-ci/$1"
shape_root="${HIBANA_PROTOCOL_SHAPE_OUTPUT_ROOT:-$ROOT_DIR/target/protocol-shapes}"
mkdir -p "$(dirname "$out")"
mkdir "$out"
printf '%s\n' "$ASSIGNMENT" > "$out/assignment.json"
git rev-parse HEAD > "$out/revision.txt"
case "$SELECTION" in
  constructors)
    cargo +stable run --quiet --locked --manifest-path .github/ci-tests/Cargo.toml --bin protocol-shape-partitions -- shards "$BUDGET" "$WINDOW" "$PARTITION" > "$out/assigned-shards.tsv"
    while IFS=$'\t' read -r shard shards; do
      bash .github/scripts/check_protocol_shapes.sh "$BUDGET" "$shard" "$shards"
    done < "$out/assigned-shards.tsv"
    ;;
  contracts)
    extra=()
    if [[ "$FIRST:$COUNT" == 2:1 ]]; then extra+=(--admission-controls); fi
    bash .github/scripts/check_protocol_shapes.sh "${extra[@]}" "--$SELECTION" "$FIRST" "$COUNT"
    if [[ "$FIRST:$COUNT" == 17:1 ]]; then
      awk -F '\t' 'NR > 1 { seen[$3] = 1 } END { exit !(seen["false"] && seen["true"]) }' "$shape_root/contracts/range-17-count-1/roll-schedule-coverage.tsv"
    fi
    ;;
  interactions|spines)
    bash .github/scripts/check_protocol_shapes.sh "--$SELECTION" "$DEPTH" "$FIRST" "$COUNT"
    if [[ "$SELECTION:$DEPTH:$FIRST" == spines:3:658 ]]; then
      awk -F '\t' 'NR > 1 { seen[$3] = 1 } END { exit !(seen["false"] && seen["true"]) }' "$shape_root/spines-3/range-658-count-1/roll-schedule-coverage.tsv"
    fi
    ;;
  sample)
    cargo +stable build --locked --manifest-path .github/ci-tests/Cargo.toml --bin protocol-shapes --bin protocol-shape-controls
    seed="${SEED:-$(date -u +%Y%m%d)}"
    target/debug/protocol-shapes syntax-fuzz "$PWD" "$DEPTH" "$seed" 4 "target/protocol-fuzz-$DEPTH-$seed"
    ;;
  *) echo "Unknown protocol selection: $SELECTION" >&2; exit 2 ;;
esac
