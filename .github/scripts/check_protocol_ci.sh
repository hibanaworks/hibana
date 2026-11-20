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
out="${HIBANA_PROTOCOL_CI_OUTPUT_ROOT:-$ROOT_DIR/target/protocol-ci}/$1"
if [[ "$out" != /* ]]; then
  echo 'HIBANA_PROTOCOL_CI_OUTPUT_ROOT must be an absolute path' >&2
  exit 2
fi
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
    args=(--contracts "$FIRST" "$COUNT")
    if [[ "$FIRST:$COUNT" == 2:1 ]]; then args=(--admission-controls "${args[@]}"); fi
    if [[ "$FIRST:$COUNT" == 39:1 ]]; then
      source "$ROOT_DIR/.github/scripts/lib/current_input.sh"
      epf="$(dirname "$ROOT_DIR")/hibana-epf"
      checkout_current_input "$epf" https://github.com/hibanaworks/hibana-epf.git "$(cat "$ROOT_DIR/.github/epf-revision")" "$out"
      trap 'if [[ -d "$out/epf-target" ]]; then rm -r -- "$out/epf-target"; fi' EXIT
      cargo +stable build --locked --manifest-path "$epf/Cargo.toml" --features std --bin hibana-epf --target-dir "$out/epf-target"
      args=(--epf "$out/epf-target/debug/hibana-epf" "${args[@]}")
    fi
    bash .github/scripts/check_protocol_shapes.sh "${args[@]}"
    if [[ "$FIRST:$COUNT" == 39:1 ]]; then
      bash .github/scripts/check_protocol_shapes.sh --check "${args[@]}" > "$out/epf-receipt-check.log"
      # The same source-enabled receive at the same local operation must be
      # polled before arrival and after arrival in the two existing schedules.
      jq -ne '[inputs | select(.probe.receive and .probe.atReceive and (.probe.after | not) and (.sourceRows | length > 0))
        | . + {capture: input_filename}]
        | group_by([.capture, .script, .probe, .sourceRows])
        | any(.[]; any(.[]; .outcome == "completed") and any(.[]; .outcome == "pending")
          and (map(.reversePoll) | unique | length == 2))' \
        "$shape_root"/contracts/range-39-count-1/source-cuts-*/query-certificates.jsonl \
        > "$out/queued-receive-check.log"
    fi
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
