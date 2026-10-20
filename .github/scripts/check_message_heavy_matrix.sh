#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-stable}"
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
HOST="$("${RUSTUP[@]}" rustc --print host-tuple)"
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

  bash "${ROOT_DIR}/.github/scripts/lib/pressure_source_tool.sh" message unused "${count}" "${crate_dir}/src/lib.rs"
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
    observed="$(bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" observation "${output}" "compile pressure observed: message-heavy ${count} ")"
  rm -f "${output}"
  read -r "COMPILE_SECONDS[${count}]" "COMPILE_RSS_MIB[${count}]" <<< "${observed}"
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

  IMAGE_BYTES["${count}"]="$("${LLVM_SIZE}" --format=sysv "${rlib}" | bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" llvm-image)"
  RLIB_BYTES["${count}"]="$(bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" file-size "${rlib}")"
  echo "message-heavy thumb count=${count} image_bytes=${IMAGE_BYTES[${count}]} rlib_bytes=${RLIB_BYTES[${count}]}"
}

for count in 1 8 16 32 64 256; do
  build_case "${count}"
done

bash "${ROOT_DIR}/.github/scripts/lib/final_measurements_tool.sh" message-budget \
  "${IMAGE_BYTES[1]}" "${IMAGE_BYTES[64]}" "${IMAGE_BYTES[256]}" \
  "${RLIB_BYTES[1]}" "${RLIB_BYTES[64]}" "${RLIB_BYTES[256]}"
echo "message-heavy compile pressure count=1 elapsed=${COMPILE_SECONDS[1]}s rss=${COMPILE_RSS_MIB[1]}MiB count=64 elapsed=${COMPILE_SECONDS[64]}s rss=${COMPILE_RSS_MIB[64]}MiB count=256 elapsed=${COMPILE_SECONDS[256]}s rss=${COMPILE_RSS_MIB[256]}MiB"

# Each independently compiled case is already bounded by its named snapshot-derived
# time and RSS budgets. Cross-case comparisons of sampled peaks are not stable gates.

echo "message-heavy matrix check passed target=${TARGET} messages=256"
