#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
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
SNAPSHOT_FILE="${ROOT_DIR}/.github/measurement_snapshots/hibana-size-snapshot.json"
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
        0,
        1,
        hibana::g::Msg<7, ()>
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
THUMB_SECTION_OUTPUT="$("${LLVM_SIZE}" --format=sysv "${THUMB_RLIB}" \
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
    ')"
printf '%s\n' "${THUMB_SECTION_OUTPUT}"
if [[ "${HIBANA_SKIP_FIXED_SNAPSHOT_CHECK:-0}" != "1" ]]; then
THUMB_SECTION_OUTPUT="${THUMB_SECTION_OUTPUT}" SNAPSHOT_FILE="${SNAPSHOT_FILE}" python3 - <<'PY'
import json
import os
import re
import sys

with open(os.environ["SNAPSHOT_FILE"], "r", encoding="utf-8") as f:
    snapshot = json.load(f)

values = {}
for line in os.environ["THUMB_SECTION_OUTPUT"].splitlines():
    match = re.search(r"thumb section name=(\.[A-Za-z0-9_]+) bytes=([0-9]+)", line)
    if match:
        values[match.group(1)] = int(match.group(2))
values["flash_total"] = (
    values.get(".text", 0) + values.get(".rodata", 0) + values.get(".data", 0)
)
budget = snapshot["budget"]["thumbv6m_none_eabi_no_std_release_lib"]["sections"]
for name, maximum in budget.items():
    actual = values.get(name)
    if actual is None:
        print(f"final-form measurement violation: missing thumb section metric {name}", file=sys.stderr)
        sys.exit(1)
    print(f"snapshot-check thumb {name} actual={actual} budget={maximum}")
    if actual > maximum:
        print(
            f"final-form measurement violation: thumb {name}={actual} exceeds snapshot budget {maximum}",
            file=sys.stderr,
        )
        sys.exit(1)
PY
else
  echo "fixed snapshot thumb budget check skipped by explicit override; worktree regression gate still runs"
fi

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

echo "== final-form runtime stack high-water =="
STACK_HIGH_WATER_OUTPUT="$(
  cargo +"${TOOLCHAIN}" test \
    -p hibana \
    large_choreography_runtime_peak_metrics \
    --lib \
    --features std \
    --release \
    -- \
    --ignored \
    --nocapture \
    --test-threads=1
)"
printf '%s\n' "${STACK_HIGH_WATER_OUTPUT}"
if [[ "${HIBANA_SKIP_FIXED_SNAPSHOT_CHECK:-0}" != "1" ]]; then
STACK_HIGH_WATER_OUTPUT="${STACK_HIGH_WATER_OUTPUT}" THUMB_SECTION_OUTPUT="${THUMB_SECTION_OUTPUT}" SNAPSHOT_FILE="${SNAPSHOT_FILE}" python3 - <<'PY'
import json
import os
import re
import sys

with open(os.environ["SNAPSHOT_FILE"], "r", encoding="utf-8") as f:
    snapshot = json.load(f)

budget = snapshot["budget"]["runtime_shapes"]
expected = set(budget)
seen = {}
for line in os.environ["STACK_HIGH_WATER_OUTPUT"].splitlines():
    if "large-choreography-runtime " not in line:
        continue
    shape = re.search(r"shape=([A-Za-z0-9_]+)", line)
    if not shape:
        continue
    metrics = {key: int(value) for key, value in re.findall(r"([A-Za-z0-9_]+)=([0-9]+)", line)}
    seen[shape.group(1)] = metrics

missing = sorted(expected - set(seen))
if missing:
    print(
        "final-form measurement violation: missing runtime stack high-water reports for "
        + ", ".join(missing),
        file=sys.stderr,
    )
    sys.exit(1)

for shape in sorted(expected):
    for key, maximum in sorted(budget[shape].items()):
        actual = seen[shape].get(key)
        if actual is None:
            print(
                f"final-form measurement violation: missing {shape} runtime metric {key}",
                file=sys.stderr,
            )
            sys.exit(1)
        print(f"snapshot-check runtime shape={shape} {key} actual={actual} budget={maximum}")
        if actual > maximum:
            print(
                f"final-form measurement violation: {shape} {key}={actual} exceeds snapshot budget {maximum}",
                file=sys.stderr,
            )
            sys.exit(1)

actual_max_stack = max(metrics["peak_stack_bytes"] for metrics in seen.values())
budget_max_stack = max(metrics["peak_stack_bytes"] for metrics in budget.values())
print(f"snapshot-check runtime max_peak_stack_bytes actual={actual_max_stack} budget={budget_max_stack}")
if actual_max_stack > budget_max_stack:
    print(
        f"final-form measurement violation: max peak_stack_bytes={actual_max_stack} exceeds snapshot budget {budget_max_stack}",
        file=sys.stderr,
    )
    sys.exit(1)

thumb_values = {}
for line in os.environ["THUMB_SECTION_OUTPUT"].splitlines():
    match = re.search(r"thumb section name=(\.[A-Za-z0-9_]+) bytes=([0-9]+)", line)
    if match:
        thumb_values[match.group(1)] = int(match.group(2))
thumb_values["flash_total"] = (
    thumb_values.get(".text", 0) + thumb_values.get(".rodata", 0) + thumb_values.get(".data", 0)
)
section_budget = snapshot["budget"]["thumbv6m_none_eabi_no_std_release_lib"]["sections"]
actual_sram = (
    thumb_values.get(".data", 0)
    + thumb_values.get(".bss", 0)
    + max(metrics["peak_live_slab_bytes"] for metrics in seen.values())
)
budget_sram = (
    section_budget.get(".data", 0)
    + section_budget.get(".bss", 0)
    + max(metrics["peak_live_slab_bytes"] for metrics in budget.values())
)
aggregate = [
    ("max_stack", budget_max_stack, actual_max_stack),
    ("sram", budget_sram, actual_sram),
    ("flash", section_budget["flash_total"], thumb_values["flash_total"]),
]
non_growing = 0
decreased = 0
for name, maximum, actual in aggregate:
    print(f"snapshot-check aggregate {name} actual={actual} budget={maximum}")
    if actual > maximum:
        print(
            f"final-form measurement violation: aggregate {name}={actual} exceeds snapshot budget {maximum}",
            file=sys.stderr,
        )
        sys.exit(1)
    if actual <= maximum:
        non_growing += 1
    if actual < maximum:
        decreased += 1
if non_growing < 3 or decreased < 1:
    print(
        "final-form measurement violation: aggregate refactor gate requires "
        "max_stack/sram/flash all <= snapshot budget and at least one decrease",
        file=sys.stderr,
    )
    sys.exit(1)
min_sram_headroom = int(os.environ.get("HIBANA_PICO_SRAM_MIN_HEADROOM_BYTES", "64"))
sram_headroom = budget_sram - actual_sram
print(
    f"snapshot-check aggregate sram_headroom actual={sram_headroom} "
    f"minimum={min_sram_headroom}"
)
if sram_headroom < min_sram_headroom:
    print(
        "final-form measurement violation: Pico-class SRAM headroom is too small: "
        f"headroom={sram_headroom} minimum={min_sram_headroom}",
        file=sys.stderr,
    )
    sys.exit(1)
PY
else
  echo "fixed snapshot runtime budget check skipped by explicit override; worktree regression gate still runs"
fi

if [[ "${HIBANA_SKIP_WORKTREE_SIZE_REGRESSION:-0}" != "1" ]]; then
  echo "== final-form worktree size regression =="
  HIBANA_SKIP_FIXED_SNAPSHOT_CHECK=1 \
    bash "${ROOT_DIR}/.github/scripts/check_size_snapshot_regression.sh"
fi

echo "== final-form message-heavy matrix =="
bash "${ROOT_DIR}/.github/scripts/check_message_heavy_matrix.sh"

echo "final-form measurement check passed"
