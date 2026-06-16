#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

TOOLCHAIN="${TOOLCHAIN:-stable}"
TARGET="thumbv6m-none-eabi"
WORK_DIR="${ROOT_DIR}/target/thumbv6m-frame-header-codegen"
HARNESS_DIR="${WORK_DIR}/harness"
TARGET_DIR="${WORK_DIR}/target"

rustup target add --toolchain "${TOOLCHAIN}" "${TARGET}" >/dev/null

RUSTUP=(rustup run "${TOOLCHAIN}")
SYSROOT="$("${RUSTUP[@]}" rustc --print sysroot)"
HOST="$("${RUSTUP[@]}" rustc -vV | sed -n 's|host: ||p')"
RUST_BIN_DIR="${SYSROOT}/lib/rustlib/${HOST}/bin"

if [[ -x "${RUST_BIN_DIR}/llvm-nm" ]]; then
  LLVM_NM="${RUST_BIN_DIR}/llvm-nm"
elif command -v rust-nm >/dev/null 2>&1; then
  LLVM_NM="$(command -v rust-nm)"
elif command -v llvm-nm >/dev/null 2>&1; then
  LLVM_NM="$(command -v llvm-nm)"
elif [[ -x /opt/homebrew/opt/llvm/bin/llvm-nm ]]; then
  LLVM_NM="/opt/homebrew/opt/llvm/bin/llvm-nm"
else
  echo "thumbv6m FrameHeader codegen check requires llvm-nm or rust-nm" >&2
  exit 1
fi

rm -rf "${WORK_DIR}"
mkdir -p "${HARNESS_DIR}/src"

cat >"${HARNESS_DIR}/Cargo.toml" <<EOF
[package]
name = "hibana-frame-header-codegen"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
hibana = { path = "${ROOT_DIR}", default-features = false }

[lib]
path = "src/lib.rs"
EOF

cat >"${HARNESS_DIR}/src/lib.rs" <<'EOF'
#![no_std]

use hibana::runtime::transport::FrameHeader;

#[unsafe(no_mangle)]
pub extern "C" fn hibana_frame_header_codegen_probe(a: u8, b: u8) -> u8 {
    let header = FrameHeader::from_bytes([0, 0, 0, a, b, 3, 4, 5]);
    let bytes = header.bytes();
    bytes[3] ^ bytes[4] ^ bytes[5] ^ bytes[6] ^ bytes[7]
}
EOF

CARGO_TERM_COLOR=never \
CARGO_TERM_PROGRESS_WHEN=never \
CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}" \
TERM=dumb \
  cargo +"${TOOLCHAIN}" rustc \
    --manifest-path "${HARNESS_DIR}/Cargo.toml" \
    --lib \
    --release \
    --target "${TARGET}" \
    --target-dir "${TARGET_DIR}" \
    -- \
    --emit=obj \
    >/dev/null

OBJECT_FILE="$(
  find "${TARGET_DIR}/${TARGET}/release/deps" -name 'hibana_frame_header_codegen-*.o' -print -quit
)"
if [[ -z "${OBJECT_FILE}" ]]; then
  echo "thumbv6m FrameHeader codegen object missing" >&2
  exit 1
fi

NM_OUTPUT="$("${LLVM_NM}" --undefined-only "${OBJECT_FILE}" 2>/dev/null || "${LLVM_NM}" -u "${OBJECT_FILE}")"
if printf '%s\n' "${NM_OUTPUT}" \
  | rg -n "__aeabi_(lmul|lcmp|ulcmp|ldivmod|uldivmod|llsl|llsr|lasr)\\b" >/dev/null
then
  printf '%s\n' "${NM_OUTPUT}" \
    | rg -n "__aeabi_(lmul|lcmp|ulcmp|ldivmod|uldivmod|llsl|llsr|lasr)\\b" >&2
  echo "thumbv6m FrameHeader codegen regained aeabi u64 helper calls" >&2
  exit 1
fi

echo "thumbv6m FrameHeader codegen has no aeabi u64 helpers"
