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

if [[ -x "${RUST_BIN_DIR}/llvm-nm" ]]; then
  LLVM_NM="${RUST_BIN_DIR}/llvm-nm"
elif command -v llvm-nm >/dev/null 2>&1; then
  LLVM_NM="$(command -v llvm-nm)"
elif [[ -x /opt/homebrew/opt/llvm/bin/llvm-nm ]]; then
  LLVM_NM="/opt/homebrew/opt/llvm/bin/llvm-nm"
else
  echo "final-form measurements require llvm-nm" >&2
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
  echo "fixed snapshot thumb budget check skipped by explicit override; worktree size snapshot still runs"
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

echo "== final-form projected protocol matrix =="
PROTOCOL_MATRIX_OUTPUT="$(
  cargo +"${TOOLCHAIN}" test \
    -p hibana \
    projected_protocol_matrix_reports_compact_resident_images \
    --lib \
    --features std \
    -- \
    --nocapture
)"
printf '%s\n' "${PROTOCOL_MATRIX_OUTPUT}"
PROTOCOL_MATRIX_OUTPUT="${PROTOCOL_MATRIX_OUTPUT}" python3 - <<'PY'
import os
import re
import sys

expected = {
    "minimal_send_recv",
    "nested_par_join",
    "route_with_unselected_nested_par",
    "triple_nested_route",
    "passive_nested_route_observer",
    "alternating_par_route",
    "huge_legal_choreography",
}
rows = {}
for line in os.environ["PROTOCOL_MATRIX_OUTPUT"].splitlines():
    if not line.startswith("protocol-matrix "):
        continue
    name_match = re.search(r"name=([A-Za-z0-9_]+)", line)
    if not name_match:
        continue
    metrics = {key: int(value) for key, value in re.findall(r"([A-Za-z0-9_]+)=([0-9]+)", line)}
    rows[name_match.group(1)] = metrics

missing = sorted(expected - set(rows))
if missing:
    print(
        "final-form measurement violation: missing projected protocol matrix rows for "
        + ", ".join(missing),
        file=sys.stderr,
    )
    sys.exit(1)

minimal = rows["minimal_send_recv"]
minimal_budgets = {
    "program_blob_len": 16,
    "role_blob_len": 32,
    "endpoint_scratch_bytes": 512,
    "largest_section_bytes": 192,
}
for key, budget in sorted(minimal_budgets.items()):
    actual = minimal.get(key)
    print(f"snapshot-check protocol-matrix minimal {key} actual={actual} budget={budget}")
    if actual is None or actual > budget:
        print(
            f"final-form measurement violation: minimal_send_recv {key}={actual} exceeds {budget}",
            file=sys.stderr,
        )
        sys.exit(1)

matrix_budgets = {
    "program_blob_len": 256,
    "role_blob_len": 512,
    "endpoint_scratch_bytes": 1024,
    "largest_section_bytes": 384,
}
for name in sorted(expected):
    metrics = rows[name]
    for key, budget in sorted(matrix_budgets.items()):
        actual = metrics.get(key)
        print(f"snapshot-check protocol-matrix name={name} {key} actual={actual} budget={budget}")
        if actual is None or actual > budget:
            print(
                f"final-form measurement violation: {name} {key}={actual} exceeds {budget}",
                file=sys.stderr,
            )
            sys.exit(1)
PY

echo "== final-form protocol artifact flash matrix =="
PROTOCOL_ARTIFACT_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/hibana-protocol-artifacts-XXXXXX")"
cleanup_protocol_artifacts() {
  rm -rf "${PROTOCOL_ARTIFACT_ROOT}"
}
trap cleanup_protocol_artifacts EXIT

write_protocol_artifact_manifest() {
  local crate_dir="$1"
  local package_name="$2"
  mkdir -p "${crate_dir}/src"
  cat >"${crate_dir}/Cargo.toml" <<EOF
[package]
name = "${package_name}"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
hibana = { path = "${ROOT_DIR}", default-features = false }
EOF
}

FINAL_FORM_PROTOCOL_FIXTURE="${ROOT_DIR}/src/global/role_program/tests/final_form_protocol_matrix.rs"
FINAL_FORM_PROTOCOL_BLACK_BOX_FIXTURE="${ROOT_DIR}/src/global/role_program/tests/final_form_protocol_black_box_roles.rs"

write_protocol_artifact_source() {
  local crate_dir="$1"
  local protocol_name="$2"
  cp "${FINAL_FORM_PROTOCOL_FIXTURE}" "${crate_dir}/src/final_form_protocol_matrix.rs"
  cp "${FINAL_FORM_PROTOCOL_BLACK_BOX_FIXTURE}" "${crate_dir}/src/final_form_protocol_black_box_roles.rs"
  cat >"${crate_dir}/src/main.rs" <<EOF
#![no_std]
#![no_main]

use core::panic::PanicInfo;
use hibana::{g, runtime::program::{project, RoleProgram}};
use hibana::g::Msg;

include!("final_form_protocol_matrix.rs");
include!("final_form_protocol_black_box_roles.rs");

#[panic_handler]
fn panic(_: &PanicInfo) -> ! { loop {} }

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let program = final_form_protocol!(${protocol_name});
    final_form_protocol_black_box_roles!(${protocol_name}, &program);
    loop {}
}
EOF
}

protocol_matrix_metrics_for() {
  PROTOCOL_MATRIX_OUTPUT="${PROTOCOL_MATRIX_OUTPUT}" PROTOCOL_NAME="$1" python3 - <<'PY'
import os
import re
import sys

target = os.environ["PROTOCOL_NAME"]
for line in os.environ["PROTOCOL_MATRIX_OUTPUT"].splitlines():
    if not line.startswith("protocol-matrix "):
        continue
    name_match = re.search(r"name=([A-Za-z0-9_]+)", line)
    if not name_match or name_match.group(1) != target:
        continue
    metrics = {key: int(value) for key, value in re.findall(r"([A-Za-z0-9_]+)=([0-9]+)", line)}
    print(
        "program_blob_len={program_blob_len} role_blob_len={role_blob_len} "
        "endpoint_scratch_bytes={endpoint_scratch_bytes} largest_section_bytes={largest_section_bytes}".format(**metrics)
    )
    sys.exit(0)
print(f"missing protocol matrix metrics for {target}", file=sys.stderr)
sys.exit(1)
PY
}

protocol_artifact_role_bucket_count() {
  case "$1" in
    minimal_send_recv|triple_nested_route|huge_legal_choreography)
      printf '2'
      ;;
    nested_par_join|route_with_unselected_nested_par|passive_nested_route_observer|alternating_par_route)
      printf '4'
      ;;
    *)
      echo "unknown protocol artifact name: $1" >&2
      exit 1
      ;;
  esac
}

protocol_artifact_map_metrics() {
  MAP_PATH="$1" BIN_PATH="$2" LLVM_NM="${LLVM_NM}" python3 - <<'PY'
import os
import re
import subprocess
import sys

map_path = os.environ["MAP_PATH"]
bin_path = os.environ["BIN_PATH"]
llvm_nm = os.environ["LLVM_NM"]
bucket_names = (
    "ProgramProjectionBlob",
    "RoleProjectionBlob",
    "ProgramImageBytes",
    "RoleImageBytes",
)

try:
    map_text = open(map_path, encoding="utf-8", errors="replace").read()
except OSError as err:
    print(f"final-form measurement violation: cannot read link map: {err}", file=sys.stderr)
    sys.exit(1)

rodata_fragments = []
for line in map_text.splitlines():
    if ":(.rodata" not in line:
        continue
    parts = line.split()
    if len(parts) < 3:
        continue
    try:
        rodata_fragments.append(int(parts[2], 16))
    except ValueError:
        pass

nm = subprocess.run(
    [llvm_nm, "--defined-only", bin_path],
    check=False,
    capture_output=True,
    text=True,
)
if nm.returncode != 0:
    print(nm.stderr, file=sys.stderr)
    print("final-form measurement violation: llvm-nm failed for protocol artifact", file=sys.stderr)
    sys.exit(1)

map_bucket_symbol_count = sum(map_text.count(name) for name in bucket_names)
bucket_symbol_count = sum(nm.stdout.count(name) for name in bucket_names)
bucket_sizes = [32, 64, 96, 128, 192, 256, 384, 512, 1024, 2048, 4096, 8192]
print(
    "rodata_map_bytes={rodata_map_bytes} rodata_map_fragments={rodata_map_fragments} "
    "bucket_symbol_count={bucket_symbol_count} map_bucket_symbol_count={map_bucket_symbol_count} "
    "full_bucket_floor_bytes={full_bucket_floor_bytes}".format(
        rodata_map_bytes=sum(rodata_fragments),
        rodata_map_fragments=len(rodata_fragments),
        bucket_symbol_count=bucket_symbol_count,
        map_bucket_symbol_count=map_bucket_symbol_count,
        full_bucket_floor_bytes=sum(bucket_sizes),
    )
)
PY
}

build_protocol_artifact() {
  local protocol_name="$1"
  local package_name="hibana-protocol-${protocol_name//_/-}"
  local crate_dir="${PROTOCOL_ARTIFACT_ROOT}/${protocol_name}"
  local map="${crate_dir}/firmware.map"
  write_protocol_artifact_manifest "${crate_dir}" "${package_name}"
  write_protocol_artifact_source "${crate_dir}" "${protocol_name}"
  PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
  RUSTC="${TOOLCHAIN_RUSTC}" \
  RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-e -C link-arg=_start -C link-arg=-Map=${map}" \
  CARGO_TERM_COLOR=never \
  CARGO_TERM_PROGRESS_WHEN=never \
  TERM=dumb \
    "${TOOLCHAIN_CARGO}" build \
      --manifest-path "${crate_dir}/Cargo.toml" \
      --target thumbv6m-none-eabi \
      --release \
      --target-dir "${crate_dir}/target" \
      >/dev/null
  local bin="${crate_dir}/target/thumbv6m-none-eabi/release/${package_name}"
  if [[ ! -f "${bin}" ]]; then
    echo "protocol artifact missing: ${bin}" >&2
    exit 1
  fi
  local section_metrics
  section_metrics="$("${LLVM_SIZE}" --format=sysv "${bin}" \
    | awk '
        $1 ~ /^\.text/ || $1 == "__text" { text += $2 }
        $1 ~ /^\.rodata/ || $1 == "__const" || $1 == "__cstring" { rodata += $2 }
        $1 ~ /^\.data/ || $1 == "__data" { data += $2 }
        $1 ~ /^\.bss/ || $1 == "__bss" || $1 == "__common" || $1 == "__thread_bss" {
          bss += $2
        }
        END {
          printf("flash_total=%d text=%d rodata=%d data=%d bss=%d", text + rodata + data, text, rodata, data, bss)
        }
      ')"
  printf 'protocol-artifact name=%s %s %s\n' \
    "${protocol_name}" \
    "$(protocol_matrix_metrics_for "${protocol_name}")" \
    "selected_program_bucket_count=1 selected_role_bucket_count=$(protocol_artifact_role_bucket_count "${protocol_name}") ${section_metrics} $(protocol_artifact_map_metrics "${map}" "${bin}")"
}

PROTOCOL_ARTIFACT_OUTPUT="$(
  for protocol_name in \
    minimal_send_recv \
    nested_par_join \
    route_with_unselected_nested_par \
    triple_nested_route \
    passive_nested_route_observer \
    alternating_par_route \
    huge_legal_choreography
  do
    build_protocol_artifact "${protocol_name}"
  done
)"
printf '%s\n' "${PROTOCOL_ARTIFACT_OUTPUT}"
PROTOCOL_ARTIFACT_OUTPUT="${PROTOCOL_ARTIFACT_OUTPUT}" python3 - <<'PY'
import os
import re
import sys

expected = {
    "minimal_send_recv",
    "nested_par_join",
    "route_with_unselected_nested_par",
    "triple_nested_route",
    "passive_nested_route_observer",
    "alternating_par_route",
    "huge_legal_choreography",
}
rows = {}
for line in os.environ["PROTOCOL_ARTIFACT_OUTPUT"].splitlines():
    if not line.startswith("protocol-artifact "):
        continue
    name_match = re.search(r"name=([A-Za-z0-9_]+)", line)
    if not name_match:
        continue
    rows[name_match.group(1)] = {
        key: int(value) for key, value in re.findall(r"([A-Za-z0-9_]+)=([0-9]+)", line)
    }

missing = sorted(expected - set(rows))
if missing:
    print(
        "final-form measurement violation: missing protocol artifact rows for "
        + ", ".join(missing),
        file=sys.stderr,
    )
    sys.exit(1)

minimal_flash_budget = 2048
matrix_flash_budget = 16384
minimal_rodata_budget = 1024
matrix_rodata_budget = 4096
max_rodata_fragments = 32
max_rodata_section_padding_bytes = 64
for name in sorted(expected):
    row = rows[name]
    required = [
        "flash_total",
        "rodata",
        "rodata_map_bytes",
        "rodata_map_fragments",
        "program_blob_len",
        "role_blob_len",
        "endpoint_scratch_bytes",
        "largest_section_bytes",
        "selected_program_bucket_count",
        "selected_role_bucket_count",
        "bucket_symbol_count",
        "map_bucket_symbol_count",
        "full_bucket_floor_bytes",
    ]
    for key in required:
        if key not in row:
            print(f"final-form measurement violation: {name} missing artifact metric {key}", file=sys.stderr)
            sys.exit(1)
    budget = minimal_flash_budget if name == "minimal_send_recv" else matrix_flash_budget
    actual = row["flash_total"]
    print(f"snapshot-check protocol-artifact name={name} flash_total actual={actual} budget={budget}")
    if actual > budget:
        print(
            f"final-form measurement violation: {name} protocol artifact flash_total={actual} exceeds {budget}",
            file=sys.stderr,
        )
        sys.exit(1)
    rodata_budget = minimal_rodata_budget if name == "minimal_send_recv" else matrix_rodata_budget
    rodata = row["rodata"]
    print(f"snapshot-check protocol-artifact name={name} rodata actual={rodata} budget={rodata_budget}")
    if rodata > rodata_budget:
        print(
            f"final-form measurement violation: {name} protocol artifact rodata={rodata} exceeds {rodata_budget}",
            file=sys.stderr,
        )
        sys.exit(1)
    if row["rodata_map_bytes"] > rodata:
        print(
            f"final-form measurement violation: {name} map rodata={row['rodata_map_bytes']} exceeds section rodata={rodata}",
            file=sys.stderr,
        )
        sys.exit(1)
    rodata_padding = rodata - row["rodata_map_bytes"]
    print(
        f"snapshot-check protocol-artifact name={name} rodata_section_padding_bytes actual={rodata_padding} budget={max_rodata_section_padding_bytes}"
    )
    if rodata_padding > max_rodata_section_padding_bytes:
        print(
            f"final-form measurement violation: {name} rodata section padding={rodata_padding} exceeds {max_rodata_section_padding_bytes}",
            file=sys.stderr,
        )
        sys.exit(1)
    if row["rodata_map_fragments"] > max_rodata_fragments:
        print(
            f"final-form measurement violation: {name} map rodata fragments={row['rodata_map_fragments']} exceeds {max_rodata_fragments}",
            file=sys.stderr,
        )
        sys.exit(1)
    selected_bucket_count = row["selected_program_bucket_count"] + row["selected_role_bucket_count"]
    for key in ("bucket_symbol_count", "map_bucket_symbol_count"):
        actual_symbols = row[key]
        print(
            f"snapshot-check protocol-artifact name={name} {key} actual={actual_symbols} budget={selected_bucket_count}"
        )
        if actual_symbols > selected_bucket_count:
            print(
                f"final-form measurement violation: {name} {key}={actual_symbols} exceeds selected bucket count {selected_bucket_count}",
                file=sys.stderr,
            )
            sys.exit(1)
    full_ladder_floor = row["full_bucket_floor_bytes"] * selected_bucket_count
    if rodata >= full_ladder_floor:
        print(
            f"final-form measurement violation: {name} rodata={rodata} still retains every bucket ladder entry",
            file=sys.stderr,
        )
        sys.exit(1)
PY

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
  echo "fixed snapshot runtime budget check skipped by explicit override; worktree size snapshot still runs"
fi

if [[ "${HIBANA_SKIP_WORKTREE_SIZE_SNAPSHOT:-0}" != "1" ]]; then
  echo "== final-form worktree size snapshot =="
  HIBANA_SKIP_FIXED_SNAPSHOT_CHECK=1 \
    bash "${ROOT_DIR}/.github/scripts/check_size_snapshot_regression.sh"
fi

echo "== final-form message-heavy matrix =="
bash "${ROOT_DIR}/.github/scripts/check_message_heavy_matrix.sh"

echo "final-form measurement check passed"
