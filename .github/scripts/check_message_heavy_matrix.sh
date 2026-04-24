#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-stable}"
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
  echo "message-heavy matrix requires llvm-size" >&2
  exit 1
fi

MATRIX_DIR="${ROOT_DIR}/target/message_heavy_matrix"
rm -rf "${MATRIX_DIR}"
mkdir -p "${MATRIX_DIR}"

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

[dependencies]
hibana = { path = "../../..", default-features = false, features = ["std"] }
EOF

  python3 - "${count}" "${crate_dir}/src/main.rs" <<'PY'
import sys

count = int(sys.argv[1])
dst = sys.argv[2]


def send_expr(idx: int) -> str:
    label = 1 + (idx % 46)
    return (
        "g::send::<g::Role<0>, g::Role<1>, "
        f"g::Msg<{label}, Payload<{idx}>>, 0>()"
    )


def seq_expr(start: int, end: int) -> str:
    if end - start == 1:
        return send_expr(start)
    mid = start + ((end - start) // 2)
    return f"g::seq({seq_expr(start, mid)}, {seq_expr(mid, end)})"


program = seq_expr(0, count)

with open(dst, "w", encoding="utf-8") as f:
    f.write(
        '#![recursion_limit = "1024"]\n'
        "use hibana::g;\n"
        "use hibana::substrate::program::{project, RoleProgram};\n\n"
        "use hibana::substrate::wire::{CodecError, Payload as WirePayloadView, WireEncode, WirePayload};\n\n"
        "#[derive(Clone, Copy)]\n"
        "struct Payload<const ID: u16>;\n\n"
        "impl<const ID: u16> WireEncode for Payload<ID> {\n"
        "    fn encoded_len(&self) -> Option<usize> { Some(0) }\n"
        "    fn encode_into(&self, _out: &mut [u8]) -> Result<usize, CodecError> { Ok(0) }\n"
        "}\n\n"
        "impl<const ID: u16> WirePayload for Payload<ID> {\n"
        "    type Decoded<'a> = Self;\n"
        "    fn decode_payload<'a>(input: WirePayloadView<'a>) -> Result<Self::Decoded<'a>, CodecError> {\n"
        "        if input.as_bytes().is_empty() { Ok(Self) } else { Err(CodecError::Invalid(\"message-heavy payload length\")) }\n"
        "    }\n"
        "    fn synthetic_payload<'a>(_scratch: &'a mut [u8]) -> Result<WirePayloadView<'a>, CodecError> {\n"
        "        Ok(WirePayloadView::new(&[]))\n"
        "    }\n"
        "}\n\n"
        "fn main() {\n"
        f"    let program = {program};\n"
        "    let role0: RoleProgram<0> = project(&program);\n"
        "    let role1: RoleProgram<1> = project(&program);\n"
        "    std::hint::black_box((role0, role1));\n"
        "}\n"
    )
PY
}

text_bytes_for_case() {
  local count="$1"
  local crate_dir="${MATRIX_DIR}/messages_${count}"
  local target_dir="${crate_dir}/target"
  local bin="${target_dir}/release/hibana-message-heavy-${count}"

  generate_case "${count}"
  PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
  RUSTC="${TOOLCHAIN_RUSTC}" \
  CARGO_TERM_COLOR=never \
  CARGO_TERM_PROGRESS_WHEN=never \
  TERM=dumb \
    "${TOOLCHAIN_CARGO}" build \
      --manifest-path "${crate_dir}/Cargo.toml" \
      --release \
      --target-dir "${target_dir}" \
      >/dev/null

  if [[ ! -f "${bin}" ]]; then
    echo "message-heavy binary missing: ${bin}" >&2
    exit 1
  fi

  "${LLVM_SIZE}" --format=berkeley "${bin}" | awk 'NR==2 { print $1 }'
}

declare -A TEXT_BYTES=()
for count in 1 16 64 256; do
  TEXT_BYTES["${count}"]="$(text_bytes_for_case "${count}")"
  echo "message-heavy text bytes count=${count} text=${TEXT_BYTES[${count}]}"
done

growth() {
  local from="$1"
  local to="$2"
  local delta=$((TEXT_BYTES[$to] - TEXT_BYTES[$from]))
  if (( delta < 0 )); then
    delta=0
  fi
  printf '%s\n' "${delta}"
}

GROWTH_16="$(growth 1 16)"
GROWTH_64="$(growth 16 64)"
GROWTH_256="$(growth 64 256)"

echo "message-heavy text growth 1->16=${GROWTH_16} 16->64=${GROWTH_64} 64->256=${GROWTH_256}"

if (( TEXT_BYTES[256] > TEXT_BYTES[1] + 512 * 1024 )); then
  echo "message-heavy text growth exceeded absolute budget" >&2
  exit 1
fi

if (( GROWTH_64 > GROWTH_16 * 8 + 16384 )); then
  echo "message-heavy text growth is not sublinear enough between 16 and 64 message types" >&2
  exit 1
fi

if (( GROWTH_256 > GROWTH_64 * 8 + 32768 )); then
  echo "message-heavy text growth is not sublinear enough between 64 and 256 message types" >&2
  exit 1
fi

bash "${ROOT_DIR}/.github/scripts/check_message_monomorphization_hygiene.sh"

echo "message-heavy matrix check passed"
