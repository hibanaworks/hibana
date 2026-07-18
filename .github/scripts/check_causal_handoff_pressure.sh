#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
export CARGO_BUILD_JOBS=1
source "${ROOT_DIR}/.github/scripts/lib/compile_pressure_guard.sh"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

TOOLCHAIN_RUSTC="$(rustup which --toolchain "${TOOLCHAIN}" rustc)"
TOOLCHAIN_BIN_DIR="$(dirname "${TOOLCHAIN_RUSTC}")"
TOOLCHAIN_CARGO="${TOOLCHAIN_BIN_DIR}/cargo"
TARGET="thumbv6m-none-eabi"
rustup target add --toolchain "${TOOLCHAIN}" "${TARGET}" >/dev/null

MATRIX_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hibana-causal-handoff-pressure-XXXXXX")"
TARGET_DIR="${MATRIX_DIR}/target"
cleanup() {
  rm -rf "${MATRIX_DIR}"
}
trap cleanup EXIT

generate_case() {
  local shape="$1"
  local count="$2"
  local crate_dir="${MATRIX_DIR}/causal_handoff_${shape}_${count}"
  mkdir -p "${crate_dir}/src"

  cat >"${crate_dir}/Cargo.toml" <<EOF
[package]
name = "hibana-causal-handoff-${shape}-${count}"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
name = "hibana_causal_handoff_${shape}_${count}"

[dependencies]
hibana = { path = "${ROOT_DIR}", default-features = false }
EOF

  python3 - "${shape}" "${count}" "${crate_dir}/src/lib.rs" <<'PY'
import sys

shape = sys.argv[1]
count = int(sys.argv[2])
dst = sys.argv[3]


def send_expr(idx: int) -> str:
    source, target = ((0, 2), (2, 1), (1, 2), (2, 0))[idx % 4]
    label = 1 + (idx % 46)
    return f"g::send::<{source}, {target}, g::Msg<{label}, ()>>()"


def seq_expr(start: int, end: int) -> str:
    if end - start == 1:
        return send_expr(start)
    mid = start + ((end - start) // 2)
    return f"g::seq({seq_expr(start, mid)}, {seq_expr(mid, end)})"


if shape == "linear":
    program = seq_expr(0, count)
elif shape == "route":
    program = (
        f"g::route({seq_expr(0, count)}, {seq_expr(count, count * 2)})"
        ".resolve::<7>()"
    )
elif shape == "roll":
    program = f"({seq_expr(0, count)}).roll()"
else:
    raise ValueError(f"unsupported causal pressure shape: {shape}")

with open(dst, "w", encoding="utf-8") as f:
    f.write(
        "#![no_std]\n"
        "#![deny(warnings)]\n\n"
        "use hibana::g;\n"
        "use hibana::runtime::program::{project, RoleProgram};\n\n"
        "#[inline(never)]\n"
        "pub fn projected_roles() -> "
        "(RoleProgram<0>, RoleProgram<1>, RoleProgram<2>) {\n"
        f"    let program = {program};\n"
        "    (project(&program), project(&program), project(&program))\n"
        "}\n"
    )
PY
}

declare -A COMPILE_SECONDS=()
declare -A COMPILE_RSS_MIB=()

build_case() {
  local shape="$1"
  local count="$2"
  local crate_dir="${MATRIX_DIR}/causal_handoff_${shape}_${count}"
  local crate_name="hibana_causal_handoff_${shape}_${count}"
  local pressure_label="causal_handoff_${shape}_${count}"
  local output
  local observed
  local status

  generate_case "${shape}" "${count}"
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-causal-handoff-pressure.XXXXXX")"
  set +e
  if [[ "${shape}" == "linear" ]]; then
    pressure_label="causal_handoff_${count}"
  fi
  HIBANA_COMPILE_PRESSURE_LABEL="${pressure_label}" \
    HIBANA_COMPILE_PRESSURE_BUDGETS="${ROOT_DIR}/.github/measurement_snapshots/hibana-compile-pressure-budget.tsv" \
    HIBANA_COMPILE_PRESSURE_CRATE_NAME="${crate_name}" \
    HIBANA_COMPILE_PRESSURE_POLL_SECONDS=0.1 \
    run_with_compile_pressure_guard \
      "causal-handoff ${shape} ${count}" \
      env \
        PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
        RUSTC="${TOOLCHAIN_RUSTC}" \
        CARGO_TERM_COLOR=never \
        CARGO_TERM_PROGRESS_WHEN=never \
        TERM=dumb \
        CARGO_TARGET_DIR="${TARGET_DIR}" \
        "${TOOLCHAIN_CARGO}" build \
          --manifest-path "${crate_dir}/Cargo.toml" \
          --no-default-features \
          --target "${TARGET}" \
          --release \
          --lib \
      2>&1 | tee "${output}"
  status="${PIPESTATUS[0]}"
  set -e
  if [[ "${status}" -ne 0 ]]; then
    rm -f "${output}"
    exit "${status}"
  fi
  observed="$(grep -E "^compile pressure observed: causal-handoff ${shape} ${count} " "${output}" | tail -n 1)"
  rm -f "${output}"
  if [[ ! "${observed}" =~ elapsed=([0-9]+)s[[:space:]]seconds_budget=[0-9]+s[[:space:]]max_rss=([0-9]+)MiB ]]; then
    echo "causal-handoff pressure missing observation for ${shape} ${count}" >&2
    exit 1
  fi
  COMPILE_SECONDS["${shape}_${count}"]="${BASH_REMATCH[1]}"
  COMPILE_RSS_MIB["${shape}_${count}"]="${BASH_REMATCH[2]}"
}

for count in 4 64 256; do
  build_case linear "${count}"
done
for shape in route roll; do
  for count in 4 32 64; do
    build_case "${shape}" "${count}"
  done
done

# Each independently compiled case is already bounded by its named snapshot-derived
# time and RSS budgets. Cross-case comparisons of sampled peaks are not stable gates.

echo "causal-handoff pressure passed target=${TARGET} linear-events=256 route-arm-events=64 roll-events=64 linear-elapsed=${COMPILE_SECONDS[linear_256]}s route-elapsed=${COMPILE_SECONDS[route_64]}s roll-elapsed=${COMPILE_SECONDS[roll_64]}s linear-rss=${COMPILE_RSS_MIB[linear_256]}MiB route-rss=${COMPILE_RSS_MIB[route_64]}MiB roll-rss=${COMPILE_RSS_MIB[roll_64]}MiB"
