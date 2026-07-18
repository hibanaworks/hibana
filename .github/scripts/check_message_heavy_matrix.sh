#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
export CARGO_BUILD_JOBS=1
source "${ROOT_DIR}/.github/scripts/lib/compile_pressure_guard.sh"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

RUSTUP=(rustup run "${TOOLCHAIN}")
TOOLCHAIN_RUSTC="$(rustup which --toolchain "${TOOLCHAIN}" rustc)"
TOOLCHAIN_BIN_DIR="$(dirname "${TOOLCHAIN_RUSTC}")"
TOOLCHAIN_CARGO="${TOOLCHAIN_BIN_DIR}/cargo"
TARGET="thumbv6m-none-eabi"

rustup target add --toolchain "${TOOLCHAIN}" "${TARGET}" >/dev/null
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
  echo "message-heavy matrix requires llvm-size" >&2
  exit 1
fi

MATRIX_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hibana-message-heavy-matrix-XXXXXX")"
TARGET_DIR="${MATRIX_DIR}/target"
cleanup() {
  rm -rf "${MATRIX_DIR}"
}
trap cleanup EXIT

generate_case() {
  local count="$1"
  local crate_dir="${MATRIX_DIR}/messages_${count}"
  mkdir -p "${crate_dir}/src"

  cat >"${crate_dir}/Cargo.toml" <<EOF
[package]
name = "hibana-message-heavy-${count}"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
name = "hibana_message_heavy_${count}"

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


program = seq_expr(0, count)

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
        "    const SCHEMA_ID: u32 = 0x4001_0000 | ID as u32;\n"
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

declare -A IMAGE_BYTES=()
declare -A RLIB_BYTES=()
declare -A COMPILE_SECONDS=()
declare -A COMPILE_RSS_MIB=()

build_case() {
  local count="$1"
  local crate_dir="${MATRIX_DIR}/messages_${count}"
  local crate_name="hibana_message_heavy_${count}"
  local rlib="${TARGET_DIR}/${TARGET}/release/lib${crate_name}.rlib"
  local output
  local observed
  local status
  local guard=0

  generate_case "${count}"
  case "${count}" in
    1|64|256) guard=1 ;;
  esac

  if [[ "${guard}" == "1" ]]; then
    output="$(mktemp "${TMPDIR:-/tmp}/hibana-message-heavy-pressure.XXXXXX")"
    set +e
    HIBANA_COMPILE_PRESSURE_LABEL="message_heavy_${count}" \
      HIBANA_COMPILE_PRESSURE_BUDGETS="${ROOT_DIR}/.github/measurement_snapshots/hibana-compile-pressure-budget.tsv" \
      HIBANA_COMPILE_PRESSURE_CRATE_NAME="${crate_name}" \
      HIBANA_COMPILE_PRESSURE_POLL_SECONDS=0.1 \
      run_with_compile_pressure_guard \
        "message-heavy ${count}" \
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
    observed="$(grep -E "^compile pressure observed: message-heavy ${count} " "${output}" | tail -n 1)"
    rm -f "${output}"
    if [[ ! "${observed}" =~ elapsed=([0-9]+)s[[:space:]]seconds_budget=[0-9]+s[[:space:]]max_rss=([0-9]+)MiB ]]; then
      echo "message-heavy matrix missing compile-pressure observation for ${count}" >&2
      exit 1
    fi
    COMPILE_SECONDS["${count}"]="${BASH_REMATCH[1]}"
    COMPILE_RSS_MIB["${count}"]="${BASH_REMATCH[2]}"
  else
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
        >/dev/null
  fi

  if [[ ! -f "${rlib}" ]]; then
    echo "message-heavy projected rlib missing: ${rlib}" >&2
    exit 1
  fi

  IMAGE_BYTES["${count}"]="$("${LLVM_SIZE}" --format=sysv "${rlib}" | awk '
    $1 ~ /^\.text/ || $1 ~ /^\.rodata/ || $1 == "__text" ||
      $1 == "__const" || $1 == "__cstring" { total += $2 }
    END { print total + 0 }
  ')"
  RLIB_BYTES["${count}"]="$(wc -c <"${rlib}" | tr -d ' ')"
  echo "message-heavy thumb count=${count} image_bytes=${IMAGE_BYTES[${count}]} rlib_bytes=${RLIB_BYTES[${count}]}"
}

for count in 1 8 16 32 64 256; do
  build_case "${count}"
done

growth() {
  local metric="$1"
  local from="$2"
  local to="$3"
  local from_value
  local to_value
  local delta
  if [[ "${metric}" == "image" ]]; then
    from_value="${IMAGE_BYTES[$from]}"
    to_value="${IMAGE_BYTES[$to]}"
  else
    from_value="${RLIB_BYTES[$from]}"
    to_value="${RLIB_BYTES[$to]}"
  fi
  delta=$((to_value - from_value))
  if (( delta < 0 )); then
    delta=0
  fi
  printf '%s\n' "${delta}"
}

IMAGE_GROWTH_64="$(growth image 1 64)"
IMAGE_GROWTH_256="$(growth image 64 256)"
RLIB_GROWTH_64="$(growth rlib 1 64)"
RLIB_GROWTH_256="$(growth rlib 64 256)"

echo "message-heavy thumb growth image_1_to_64=${IMAGE_GROWTH_64} image_64_to_256=${IMAGE_GROWTH_256} rlib_1_to_64=${RLIB_GROWTH_64} rlib_64_to_256=${RLIB_GROWTH_256}"
echo "message-heavy compile pressure count=1 elapsed=${COMPILE_SECONDS[1]}s rss=${COMPILE_RSS_MIB[1]}MiB count=64 elapsed=${COMPILE_SECONDS[64]}s rss=${COMPILE_RSS_MIB[64]}MiB count=256 elapsed=${COMPILE_SECONDS[256]}s rss=${COMPILE_RSS_MIB[256]}MiB"

if (( IMAGE_BYTES[256] > IMAGE_BYTES[1] + 512 * 1024 )); then
  echo "message-heavy thumb image exceeded absolute growth budget" >&2
  exit 1
fi
if (( RLIB_BYTES[256] > RLIB_BYTES[1] + 2 * 1024 * 1024 )); then
  echo "message-heavy thumb rlib metadata/code exceeded absolute growth budget" >&2
  exit 1
fi
if (( IMAGE_GROWTH_256 > IMAGE_GROWTH_64 * 6 + 16384 )); then
  echo "message-heavy thumb image growth became superlinear" >&2
  exit 1
fi
if (( RLIB_GROWTH_256 > RLIB_GROWTH_64 * 6 + 65536 )); then
  echo "message-heavy thumb rlib growth became superlinear" >&2
  exit 1
fi
# Each independently compiled case is already bounded by its named snapshot-derived
# time and RSS budgets. Cross-case comparisons of sampled peaks are not stable gates.

bash "${ROOT_DIR}/.github/scripts/check_message_monomorphization_hygiene.sh"

echo "message-heavy matrix check passed target=${TARGET} messages=256"
