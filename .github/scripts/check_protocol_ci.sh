#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"
# Mutant checkouts must own their build directories too.
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
  contracts|frontiers)
    bash .github/scripts/check_protocol_shapes.sh "--$SELECTION" "$FIRST" "$COUNT"
    ;;
  interactions|spines)
    bash .github/scripts/check_protocol_shapes.sh "--$SELECTION" "$DEPTH" "$FIRST" "$COUNT"
    if [[ "$SELECTION:$DEPTH:$FIRST" == spines:3:658 ]]; then
      awk -F '\t' 'NR > 1 { seen[$3] = 1 } END { exit !(seen["false"] && seen["true"]) }' target/protocol-shapes/spines-3/range-658-count-1/roll-schedule-coverage.tsv
    fi
    ;;
  sample)
    cargo +stable build --locked --manifest-path .github/ci-tests/Cargo.toml --bin protocol-shapes --bin protocol-shape-controls
    seed="${SEED:-$(date -u +%Y%m%d)}"
    target/debug/protocol-shapes syntax-fuzz "$PWD" "$DEPTH" "$seed" 4 "target/protocol-fuzz-$DEPTH-$seed"
    ;;
  controls)
    # Positive baselines belong to this assignment, rather than being executed
    # both here and in separate matrix jobs. Mutants never inherit an outer
    # capture root shared with the passing source.
    unset HIBANA_PROTOCOL_SHAPE_OUTPUT_ROOT
    ranks="$(jq -er '.contracts | select(length > 0) | .[]' <<< "$ASSIGNMENT")"
    while IFS= read -r rank; do
      extra=()
      if [[ "$rank" == 2 ]]; then extra+=(--admission-controls); fi
      bash .github/scripts/check_protocol_shapes.sh "${extra[@]}" --contracts "$rank" 1
    done <<< "$ranks"
    # Preserve the existing combined parked-receive execution fixture too.
    bash .github/scripts/check_protocol_shapes.sh --contracts 6 2
    for check in shape-resumption prefix-input resolver-occurrence resolver-observation recv-contract parked-offer parked-receive reentry-component; do
      bash ".github/scripts/check_${check//-/_}_sensitivity.sh"
    done
    cargo +stable build --locked --manifest-path .github/ci-tests/Cargo.toml --bin protocol-shapes --bin protocol-shape-controls
    target/debug/protocol-shapes syntax-shrink "$PWD" 3 19011422329217 target/protocol-fuzz-shrink-control -- "$PWD/target/debug/protocol-shape-controls" --shrink-probe
    target/debug/protocol-shape-controls --check-shrink target/protocol-fuzz-shrink-control
    ;;
  *) echo "Unknown protocol selection: $SELECTION" >&2; exit 2 ;;
esac
