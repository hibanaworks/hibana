#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

RUSTUP=(rustup run "${TOOLCHAIN}")
TOOLCHAIN_RUSTC="$(rustup which --toolchain "${TOOLCHAIN}" rustc)"
TOOLCHAIN_BIN_DIR="$(dirname "${TOOLCHAIN_RUSTC}")"
TOOLCHAIN_CARGO="${TOOLCHAIN_BIN_DIR}/cargo"

rustup component add llvm-tools-preview --toolchain "${TOOLCHAIN}" >/dev/null

SYSROOT="$("${RUSTUP[@]}" rustc --print sysroot)"
HOST="$("${RUSTUP[@]}" rustc -vV | sed -n 's|host: ||p')"
RUST_BIN_DIR="${SYSROOT}/lib/rustlib/${HOST}/bin"

if [[ -x "${RUST_BIN_DIR}/llvm-size" ]]; then
  LLVM_SIZE="${RUST_BIN_DIR}/llvm-size"
elif command -v llvm-size >/dev/null 2>&1; then
  LLVM_SIZE="$(command -v llvm-size)"
elif [[ -x /opt/homebrew/opt/llvm/bin/llvm-size ]]; then
  LLVM_SIZE="/opt/homebrew/opt/llvm/bin/llvm-size"
else
  echo "final-form measurements require llvm-size" >&2
  exit 1
fi

MEASURE_DIR="${ROOT_DIR}/target/final_form_measurements"
rm -rf "${MEASURE_DIR}"
mkdir -p "${MEASURE_DIR}/src"

cat >"${MEASURE_DIR}/Cargo.toml" <<'EOF'
[package]
name = "hibana-final-form-measure"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
hibana = { path = "../..", default-features = false, features = ["std"] }
EOF

cat >"${MEASURE_DIR}/src/main.rs" <<'EOF'
fn main() {
    std::hint::black_box(hibana::g::send::<
        hibana::g::Role<0>,
        hibana::g::Role<1>,
        hibana::g::Msg<7, ()>,
        0,
    >());
}
EOF

PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
RUSTC="${TOOLCHAIN_RUSTC}" \
CARGO_TERM_COLOR=never \
CARGO_TERM_PROGRESS_WHEN=never \
TERM=dumb \
  "${TOOLCHAIN_CARGO}" build \
    --manifest-path "${MEASURE_DIR}/Cargo.toml" \
    --release \
    --target-dir "${MEASURE_DIR}/target" \
    >/dev/null

BIN="${MEASURE_DIR}/target/release/hibana-final-form-measure"
if [[ ! -f "${BIN}" ]]; then
  echo "final-form measurement binary missing: ${BIN}" >&2
  exit 1
fi

echo "== final-form binary sections =="
"${LLVM_SIZE}" --format=sysv "${BIN}" \
  | awk '
      $1 ~ /^\.text/ || $1 == "__text" { text += $2 }
      $1 ~ /^\.rodata/ || $1 == "__const" || $1 == "__cstring" { rodata += $2 }
      $1 ~ /^\.data/ || $1 == "__data" { data += $2 }
      $1 ~ /^\.bss/ || $1 == "__bss" || $1 == "__common" || $1 == "__thread_bss" {
        bss += $2
      }
      END {
        printf("section name=.text bytes=%d\n", text)
        printf("section name=.rodata bytes=%d\n", rodata)
        printf("section name=.data bytes=%d\n", data)
        printf("section name=.bss bytes=%d\n", bss)
      }
    '

echo "== final-form thumb/no-std binary sections =="
rustup target add --toolchain "${TOOLCHAIN}" thumbv6m-none-eabi >/dev/null
CARGO_TERM_COLOR=never \
CARGO_TERM_PROGRESS_WHEN=never \
TERM=dumb \
  cargo +"${TOOLCHAIN}" build \
    -p hibana \
    --no-default-features \
    --target thumbv6m-none-eabi \
    --release \
    --lib \
    >/dev/null
THUMB_RLIB="${ROOT_DIR}/target/thumbv6m-none-eabi/release/libhibana.rlib"
if [[ ! -f "${THUMB_RLIB}" ]]; then
  echo "final-form thumb measurement artifact missing: ${THUMB_RLIB}" >&2
  exit 1
fi
"${LLVM_SIZE}" --format=sysv "${THUMB_RLIB}" \
  | awk '
      $1 ~ /^\.text/ || $1 == "__text" { text += $2 }
      $1 ~ /^\.rodata/ || $1 == "__const" || $1 == "__cstring" { rodata += $2 }
      $1 ~ /^\.data/ || $1 == "__data" { data += $2 }
      $1 ~ /^\.bss/ || $1 == "__bss" || $1 == "__common" || $1 == "__thread_bss" {
        bss += $2
      }
      END {
        printf("thumb section name=.text bytes=%d target=thumbv6m-none-eabi no_default_features=1\n", text)
        printf("thumb section name=.rodata bytes=%d target=thumbv6m-none-eabi no_default_features=1\n", rodata)
        printf("thumb section name=.data bytes=%d target=thumbv6m-none-eabi no_default_features=1\n", data)
        printf("thumb section name=.bss bytes=%d target=thumbv6m-none-eabi no_default_features=1\n", bss)
      }
    '

echo "== final-form future/layout sizes =="
cargo +"${TOOLCHAIN}" test -p hibana endpoint_surface_size_gates_hold --lib --features std
cargo +"${TOOLCHAIN}" test -p hibana message_type_variation_does_not_change_future_layout --lib --features std
cargo +"${TOOLCHAIN}" test -p hibana send_flow_and_runtime_descriptor_size_gates_hold --lib --features std
FUTURE_LAYOUT_OUTPUT="$(
  cargo +"${TOOLCHAIN}" test -p hibana final_form_future_layout_measurement_report --lib --features std -- --nocapture
)"
printf '%s\n' "${FUTURE_LAYOUT_OUTPUT}"
FUTURE_LAYOUT_OUTPUT="${FUTURE_LAYOUT_OUTPUT}" python3 - <<'PY'
import os
import re
import struct
import sys

line = next(
    (line for line in os.environ["FUTURE_LAYOUT_OUTPUT"].splitlines() if line.startswith("future-layout ")),
    None,
)
if line is None:
    print("final-form measurement violation: missing future-layout report", file=sys.stderr)
    sys.exit(1)
values = {key: int(value) for key, value in re.findall(r"([A-Za-z0-9]+)=([0-9]+)", line)}
word = struct.calcsize("P")
budgets = {
    "Endpoint": 3 * word,
    "RouteBranch": 2 * word,
    "OfferFuture": 3 * word,
    "RecvFuture": 3 * word,
    "DecodeFuture": 3 * word,
    "SendFuture": 3 * word,
}
for name, budget in budgets.items():
    if values.get(name, budget + 1) > budget:
        print(f"final-form measurement violation: {name}={values.get(name)} exceeds {budget}", file=sys.stderr)
        sys.exit(1)
if not (
    values.get("RecvFuture") == values.get("RecvFutureU8") == values.get("RecvFutureU64") == values.get("RecvFutureBytes")
    and values.get("DecodeFuture") == values.get("DecodeFutureU8") == values.get("DecodeFutureU64") == values.get("DecodeFutureBytes")
):
    print("final-form measurement violation: future size depends on message payload type", file=sys.stderr)
    sys.exit(1)
PY

echo "== final-form resident descriptor high-water =="
cargo +"${TOOLCHAIN}" test -p hibana huge_shape_matrix_resident_bytes_stay_measured_and_local --lib --features std -- --nocapture

echo "== final-form pico high-water =="
HIBANA_PICO_TOOLCHAIN="${TOOLCHAIN}" \
HIBANA_PICO_TARGET_DIR="${ROOT_DIR}/target/final_form_measurements/pico_route_heavy" \
  bash "${ROOT_DIR}/.github/scripts/check_pico_smoke.sh"

echo "== final-form message-heavy matrix =="
bash "${ROOT_DIR}/.github/scripts/check_message_heavy_matrix.sh"

echo "== final-form memory-control matrix =="
HIBANA_MEMORY_CONTROL_TARGET_DIR="${MEASURE_DIR}/memory_control_matrix" \
  bash "${ROOT_DIR}/.github/scripts/check_memory_control_matrix.sh" 1 4 8

echo "final-form measurement check passed"
