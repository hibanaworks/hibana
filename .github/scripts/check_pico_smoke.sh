#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

TARGET="thumbv6m-none-eabi"
SMOKE_MANIFEST="internal/pico_smoke/Cargo.toml"
SMOKE_NAME="hibana-pico-smoke"
SMOKE_TARGET_DIR="$ROOT/target/pico_smoke"
if [[ -n "${HIBANA_PICO_TARGET_DIR:-}" ]]; then
  SMOKE_TARGET_DIR="${HIBANA_PICO_TARGET_DIR}"
fi
LINKER_SCRIPT="$ROOT/internal/pico_smoke/pico_smoke.ld"
FLASH_BUDGET=$((2 * 1024 * 1024))
# Reserve 96 KiB of RP2040 SRAM for future engine/app wiring work.
SRAM_BUDGET=$((168 * 1024))
PRACTICAL_FLASH_BUDGET=$((768 * 1024))
PRACTICAL_STATIC_SRAM_BUDGET=$((48 * 1024))
PRACTICAL_KERNEL_STACK_BUDGET=$((24 * 1024))
PRACTICAL_PEAK_SRAM_BUDGET=$((96 * 1024))
export TOOLCHAIN="${HIBANA_PICO_TOOLCHAIN:-stable}"
FEATURES="${HIBANA_PICO_FEATURES:-}"

bash "${ROOT}/.github/scripts/ensure_rust_toolchain.sh" "$TARGET"

RUSTUP=(rustup run "$TOOLCHAIN")
TOOLCHAIN_RUSTC="$(rustup which --toolchain "$TOOLCHAIN" rustc)"
TOOLCHAIN_BIN_DIR="$(dirname "$TOOLCHAIN_RUSTC")"
TOOLCHAIN_CARGO="$TOOLCHAIN_BIN_DIR/cargo"

rustup component add llvm-tools-preview --toolchain "$TOOLCHAIN" >/dev/null

SYSROOT="$("${RUSTUP[@]}" rustc --print sysroot)"
HOST="$("${RUSTUP[@]}" rustc -vV | sed -n 's|host: ||p')"
RUST_BIN_DIR="$SYSROOT/lib/rustlib/$HOST/bin"

if [[ -x "$RUST_BIN_DIR/rust-lld" ]]; then
  LINKER="$RUST_BIN_DIR/rust-lld"
elif command -v ld.lld >/dev/null 2>&1; then
  LINKER="$(command -v ld.lld)"
else
  echo "pico smoke gate requires rust-lld or ld.lld" >&2
  exit 1
fi

if [[ -x "$RUST_BIN_DIR/llvm-size" ]]; then
  LLVM_SIZE="$RUST_BIN_DIR/llvm-size"
elif command -v llvm-size >/dev/null 2>&1; then
  LLVM_SIZE="$(command -v llvm-size)"
elif [[ -x /opt/homebrew/opt/llvm/bin/llvm-size ]]; then
  LLVM_SIZE="/opt/homebrew/opt/llvm/bin/llvm-size"
else
  echo "pico smoke gate requires llvm-size" >&2
  exit 1
fi

if [[ -x "$RUST_BIN_DIR/llvm-nm" ]]; then
  LLVM_NM="$RUST_BIN_DIR/llvm-nm"
elif command -v llvm-nm >/dev/null 2>&1; then
  LLVM_NM="$(command -v llvm-nm)"
elif [[ -x /opt/homebrew/opt/llvm/bin/llvm-nm ]]; then
  LLVM_NM="/opt/homebrew/opt/llvm/bin/llvm-nm"
else
  echo "pico smoke gate requires llvm-nm" >&2
  exit 1
fi

CARGO_ARGS=(
  rustc
  --manifest-path "$SMOKE_MANIFEST"
  --release
  --target "$TARGET"
  --target-dir "$SMOKE_TARGET_DIR"
)

if [[ -n "$FEATURES" ]]; then
  CARGO_ARGS+=(--features "$FEATURES")
fi

CARGO_ARGS+=(
  --config "target.$TARGET.linker = '$LINKER'"
  --
  -C "link-arg=-T$LINKER_SCRIPT"
  -C link-arg=--gc-sections
)

PATH="$TOOLCHAIN_BIN_DIR:$PATH" \
RUSTC="$TOOLCHAIN_RUSTC" \
"$TOOLCHAIN_CARGO" "${CARGO_ARGS[@]}"

BIN="$SMOKE_TARGET_DIR/$TARGET/release/$SMOKE_NAME"
if [[ ! -f "$BIN" ]]; then
  echo "pico smoke binary missing: $BIN" >&2
  exit 1
fi

read -r TEXT DATA BSS _DEC _HEX _NAME < <(
  "$LLVM_SIZE" --format=berkeley "$BIN" | awk 'NR==2 { print $1, $2, $3, $4, $5, $6 }'
)

FLASH_BYTES=$((TEXT + DATA))
SRAM_BYTES=$((DATA + BSS))

symbol_addr() {
  local symbol="$1"
  local value
  value="$("$LLVM_NM" -n "$BIN" | awk -v sym="$symbol" '$NF == sym { print $1; exit }')"
  if [[ -z "$value" ]]; then
    echo "missing pico smoke linker symbol: $symbol" >&2
    exit 1
  fi
  printf '%s\n' "$((16#$value))"
}

STACK_TOP_ADDR="$(symbol_addr __stack_top)"
STACK_LIMIT_ADDR="$(symbol_addr __stack_limit)"
STACK_RESERVED_BYTES=$((STACK_TOP_ADDR - STACK_LIMIT_ADDR))
PEAK_STACK_UPPER_BOUND_BYTES="$STACK_RESERVED_BYTES"
PEAK_SRAM_UPPER_BOUND_BYTES=$((SRAM_BYTES + STACK_RESERVED_BYTES))

shape_name="route_heavy"
runtime_test_name="pico_smoke_runtime_peak_metrics_route_heavy"
case "$FEATURES" in
  "")
    shape_name="route_heavy"
    runtime_test_name="pico_smoke_runtime_peak_metrics_route_heavy"
    ;;
  "linear-heavy")
    shape_name="linear_heavy"
    runtime_test_name="pico_smoke_runtime_peak_metrics_linear_heavy"
    ;;
  "fanout-heavy")
    shape_name="fanout_heavy"
    runtime_test_name="pico_smoke_runtime_peak_metrics_fanout_heavy"
    ;;
  *)
    echo "unsupported pico smoke feature shape: $FEATURES" >&2
    exit 1
    ;;
esac

PATH="$TOOLCHAIN_BIN_DIR:$PATH" \
RUSTC="$TOOLCHAIN_RUSTC" \
CARGO_TERM_COLOR=never \
CARGO_TERM_PROGRESS_WHEN=never \
TERM=dumb \
  "$TOOLCHAIN_CARGO" test -p hibana --lib --features std \
    "$runtime_test_name" --release --no-run >/dev/null

RUNTIME_TEST_OUTPUT="$(
  PATH="$TOOLCHAIN_BIN_DIR:$PATH" \
  RUSTC="$TOOLCHAIN_RUSTC" \
  CARGO_TERM_COLOR=never \
  CARGO_TERM_PROGRESS_WHEN=never \
  TERM=dumb \
  RUST_MIN_STACK=32768 \
    "$TOOLCHAIN_CARGO" test -p hibana --lib --features std \
      "$runtime_test_name" --release -- --ignored --nocapture
)"

RUNTIME_METRICS_LINE="$(printf '%s\n' "$RUNTIME_TEST_OUTPUT" | awk '/^pico-runtime / { line = $0 } END { print line }')"
if [[ -z "$RUNTIME_METRICS_LINE" ]]; then
  echo "missing pico runtime metrics line for $shape_name" >&2
  printf '%s\n' "$RUNTIME_TEST_OUTPUT" >&2
  exit 1
fi

runtime_metric_field() {
  local key="$1"
  local value
  value="$(printf '%s\n' "$RUNTIME_METRICS_LINE" | tr ' ' '\n' | awk -F= -v key="$key" '$1 == key { print $2; exit }')"
  if [[ -z "$value" ]]; then
    echo "missing pico runtime metric field: $key" >&2
    printf '%s\n' "$RUNTIME_METRICS_LINE" >&2
    exit 1
  fi
  printf '%s\n' "$value"
}

MEASURED_SLAB_BYTES="$(runtime_metric_field slab_bytes)"
MEASURED_SIDECAR_SCRATCH_HIGH_WATER_BYTES="$(runtime_metric_field sidecar_scratch_high_water_bytes)"
MEASURED_LIVE_ENDPOINT_BYTES="$(runtime_metric_field live_endpoint_bytes)"
MEASURED_PEAK_LIVE_SLAB_BYTES="$(runtime_metric_field peak_live_slab_bytes)"
MEASURED_PEAK_STACK_BYTES="$(runtime_metric_field peak_stack_bytes)"

if (( MEASURED_SLAB_BYTES > SRAM_BYTES )); then
  echo "pico smoke slab bytes exceed static SRAM bytes: $MEASURED_SLAB_BYTES > $SRAM_BYTES" >&2
  exit 1
fi

STATIC_SRAM_BASE_BYTES=$((SRAM_BYTES - MEASURED_SLAB_BYTES))
MEASURED_PEAK_SRAM_BYTES=$((STATIC_SRAM_BASE_BYTES + MEASURED_PEAK_LIVE_SLAB_BYTES + MEASURED_PEAK_STACK_BYTES))

echo "pico smoke flash bytes: $FLASH_BYTES"
echo "pico smoke sram bytes: $SRAM_BYTES"
echo "pico smoke static sram bytes: $SRAM_BYTES"
echo "pico smoke kernel stack reserve bytes: $STACK_RESERVED_BYTES"
echo "pico smoke peak stack upper-bound bytes: $PEAK_STACK_UPPER_BOUND_BYTES"
echo "pico smoke peak sram upper-bound bytes: $PEAK_SRAM_UPPER_BOUND_BYTES"
echo "pico smoke measured sidecar/scratch high-water bytes: $MEASURED_SIDECAR_SCRATCH_HIGH_WATER_BYTES"
echo "pico smoke measured live endpoint bytes: $MEASURED_LIVE_ENDPOINT_BYTES"
echo "pico smoke measured live slab bytes: $MEASURED_PEAK_LIVE_SLAB_BYTES"
echo "pico smoke measured peak stack bytes: $MEASURED_PEAK_STACK_BYTES"
echo "pico smoke measured peak sram bytes: $MEASURED_PEAK_SRAM_BYTES"

if (( FLASH_BYTES >= FLASH_BUDGET )); then
  echo "pico smoke flash budget exceeded: $FLASH_BYTES >= $FLASH_BUDGET" >&2
  exit 1
fi

if (( SRAM_BYTES >= SRAM_BUDGET )); then
  echo "pico smoke SRAM budget exceeded: $SRAM_BYTES >= $SRAM_BUDGET" >&2
  exit 1
fi

report_practical_budget() {
  local kind="$1"
  local value="$2"
  local budget="$3"
  if (( value > budget )); then
    echo "pico smoke practical contract exceeded for $kind: $value > $budget" >&2
    exit 1
  fi
}

report_practical_budget "flash bytes" "$FLASH_BYTES" "$PRACTICAL_FLASH_BUDGET"
report_practical_budget "static sram bytes" "$SRAM_BYTES" "$PRACTICAL_STATIC_SRAM_BUDGET"
report_practical_budget "kernel stack reserve bytes" "$STACK_RESERVED_BYTES" "$PRACTICAL_KERNEL_STACK_BUDGET"
report_practical_budget "peak sram upper-bound bytes" "$PEAK_SRAM_UPPER_BOUND_BYTES" "$PRACTICAL_PEAK_SRAM_BUDGET"
