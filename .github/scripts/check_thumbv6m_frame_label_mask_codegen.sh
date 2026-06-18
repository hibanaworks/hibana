#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

TOOLCHAIN="${TOOLCHAIN:-stable}"
TARGET="thumbv6m-none-eabi"
WORK_DIR="${ROOT_DIR}/target/thumbv6m-frame-label-mask-codegen"
HARNESS_DIR="${WORK_DIR}/harness"
TARGET_DIR="${WORK_DIR}/target"
LABELS_RS="${ROOT_DIR}/src/transport/labels.rs"

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
  echo "thumbv6m FrameLabelMask codegen check requires llvm-nm or rust-nm" >&2
  exit 1
fi

rm -rf "${WORK_DIR}"
mkdir -p "${HARNESS_DIR}/src"

cat >"${HARNESS_DIR}/Cargo.toml" <<'EOF'
[package]
name = "hibana-frame-label-mask-codegen"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
path = "src/lib.rs"
EOF

cat >"${HARNESS_DIR}/src/lib.rs" <<EOF
#![no_std]

#[path = "${LABELS_RS}"]
mod labels;

use labels::{FrameLabel, FrameLabelMask, LogicalLabel};

#[unsafe(no_mangle)]
pub extern "C" fn hibana_frame_label_mask_codegen_probe(a: u8, b: u8, c: u8) -> u8 {
    let mut mask = FrameLabelMask::from_frame_label(a) | FrameLabelMask::from_frame_label(b);
    let peer = FrameLabelMask::from_frame_label(c);
    let mut out = LogicalLabel::new(a).raw() ^ FrameLabel::new(b).raw();
    if mask.contains_frame_label(a) {
        out |= 1;
    }
    if mask.intersects(peer) {
        out |= 2;
    }
    mask.remove_frame_label(a);
    if mask.insert_frame_label(a) {
        out |= 4;
    }
    let mut filtered = (mask & !FrameLabelMask::EMPTY).without(peer);
    if filtered.take_matching(|frame_label| frame_label == a || frame_label == b).is_some() {
        out |= 8;
    }
    out
}
EOF

CARGO_TERM_COLOR=never \
CARGO_TERM_PROGRESS_WHEN=never \
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
  find "${TARGET_DIR}/${TARGET}/release/deps" -name '*.o' -print -quit
)"
if [[ -z "${OBJECT_FILE}" ]]; then
  echo "thumbv6m FrameLabelMask codegen object missing" >&2
  exit 1
fi

NM_OUTPUT="$("${LLVM_NM}" --undefined-only "${OBJECT_FILE}" 2>/dev/null || "${LLVM_NM}" -u "${OBJECT_FILE}")"
if printf '%s\n' "${NM_OUTPUT}" \
  | rg -n "__aeabi_(lmul|lcmp|ulcmp|ldivmod|uldivmod|llsl|llsr|lasr)\\b" >/dev/null
then
  printf '%s\n' "${NM_OUTPUT}" \
    | rg -n "__aeabi_(lmul|lcmp|ulcmp|ldivmod|uldivmod|llsl|llsr|lasr)\\b" >&2
  echo "thumbv6m FrameLabelMask codegen regained aeabi u64 helper calls" >&2
  exit 1
fi

echo "thumbv6m FrameLabelMask codegen has no aeabi u64 helpers"
