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

MATRIX_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hibana-route-arm-pressure-XXXXXX")"
TARGET_DIR="${MATRIX_DIR}/target"
cleanup() {
  rm -rf "${MATRIX_DIR}"
}
trap cleanup EXIT

generate_case() {
  local count="$1"
  local crate_dir="${MATRIX_DIR}/route_arm_${count}"
  mkdir -p "${crate_dir}/src"

  cat >"${crate_dir}/Cargo.toml" <<EOF
[package]
name = "hibana-route-arm-heavy-${count}"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
name = "hibana_route_arm_heavy_${count}"

[dependencies]
hibana = { path = "${ROOT_DIR}", default-features = false }
EOF

  python3 - "${count}" "${crate_dir}/src/lib.rs" <<'PY'
import sys

count = int(sys.argv[1])
dst = sys.argv[2]


def send_expr(idx: int) -> str:
    label = 1 + (idx % 46)
    return "g::send::<0, 1, " f"g::Msg<{label}, Payload<{idx}>>>()"


def seq_expr(start: int, end: int) -> str:
    if end - start == 1:
        return send_expr(start)
    mid = start + ((end - start) // 2)
    return f"g::seq({seq_expr(start, mid)}, {seq_expr(mid, end)})"


program = f"g::route({seq_expr(0, count)}, {seq_expr(count, count * 2)})"

with open(dst, "w", encoding="utf-8") as f:
    f.write(
        "#![no_std]\n"
        "#![deny(warnings)]\n\n"
        "use hibana::g;\n"
        "use hibana::runtime::program::{project, RoleProgram};\n"
        "use hibana::runtime::wire::{"
        "CodecError, Payload as WirePayloadView, WireEncode, WirePayload};\n\n"
        "#[derive(Clone, Copy)]\n"
        "struct Payload<const ID: u16>;\n\n"
        "impl<const ID: u16> WireEncode for Payload<ID> {\n"
        "    fn encode_into(&self, _out: &mut [u8]) -> Result<usize, CodecError> { Ok(0) }\n"
        "}\n\n"
        "impl<const ID: u16> WirePayload for Payload<ID> {\n"
        "    const SCHEMA_ID: u32 = 0x5001_0000 | ID as u32;\n"
        "    type Decoded<'a> = Self;\n"
        "    fn validate_payload(input: WirePayloadView<'_>) -> Result<(), CodecError> {\n"
        "        if input.as_bytes().is_empty() { Ok(()) } else { Err(CodecError::Malformed) }\n"
        "    }\n"
        "    fn decode_validated_payload<'a>(input: WirePayloadView<'a>) -> Self::Decoded<'a> {\n"
        "        let _ = input.as_bytes();\n"
        "        Self\n"
        "    }\n"
        "}\n\n"
        "#[inline(never)]\n"
        "pub fn projected_pair() -> (RoleProgram<0>, RoleProgram<1>) {\n"
        f"    let program = {program};\n"
        "    (project(&program), project(&program))\n"
        "}\n"
    )
PY
}

declare -A COMPILE_SECONDS=()
declare -A COMPILE_RSS_MIB=()

build_case() {
  local count="$1"
  local crate_dir="${MATRIX_DIR}/route_arm_${count}"
  local crate_name="hibana_route_arm_heavy_${count}"
  local output
  local observed
  local status

  generate_case "${count}"
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-route-arm-pressure.XXXXXX")"
  set +e
  HIBANA_COMPILE_PRESSURE_LABEL="route_arm_heavy_${count}" \
    HIBANA_COMPILE_PRESSURE_BUDGETS="${ROOT_DIR}/.github/measurement_snapshots/hibana-compile-pressure-budget.tsv" \
    HIBANA_COMPILE_PRESSURE_CRATE_NAME="${crate_name}" \
    HIBANA_COMPILE_PRESSURE_POLL_SECONDS=0.1 \
    run_with_compile_pressure_guard \
      "route-arm-heavy ${count}" \
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
  observed="$(grep -E "^compile pressure observed: route-arm-heavy ${count} " "${output}" | tail -n 1)"
  rm -f "${output}"
  if [[ ! "${observed}" =~ elapsed=([0-9]+)s[[:space:]]seconds_budget=[0-9]+s[[:space:]]max_rss=([0-9]+)MiB ]]; then
    echo "route-arm projection pressure missing observation for ${count}" >&2
    exit 1
  fi
  COMPILE_SECONDS["${count}"]="${BASH_REMATCH[1]}"
  COMPILE_RSS_MIB["${count}"]="${BASH_REMATCH[2]}"
}

for count in 1 64 256; do
  build_case "${count}"
done

if (( COMPILE_SECONDS[256] > COMPILE_SECONDS[64] * 6 + 8 )); then
  echo "route-arm projection compile time became superlinear" >&2
  exit 1
fi
if (( COMPILE_RSS_MIB[256] > COMPILE_RSS_MIB[64] * 2 + 128 )); then
  echo "route-arm projection compile RSS exceeded the bounded lane-fact budget" >&2
  exit 1
fi

echo "route-arm projection pressure passed target=${TARGET} arm-events=256 elapsed=${COMPILE_SECONDS[256]}s rss=${COMPILE_RSS_MIB[256]}MiB"
