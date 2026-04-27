#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

TARGET="thumbv6m-none-eabi"
SMOKE_MANIFEST="internal/pico_smoke/Cargo.toml"
SMOKE_NAME="hibana-pico-smoke"
TARGET_DIR="${HIBANA_MEMORY_CONTROL_TARGET_DIR:-$ROOT/target/memory_control_matrix}"
LINKER_SCRIPT="$ROOT/internal/pico_smoke/pico_smoke.ld"
TOOLCHAIN="${TOOLCHAIN:-1.95.0}"

if (($# == 0)); then
  set -- 1 4 8
fi

bash "${ROOT}/.github/scripts/ensure_rust_toolchain.sh" "$TARGET"

RUSTUP=(rustup run "$TOOLCHAIN")
TOOLCHAIN_RUSTC="$(rustup which --toolchain "$TOOLCHAIN" rustc)"
TOOLCHAIN_BIN_DIR="$(dirname "$TOOLCHAIN_RUSTC")"
TOOLCHAIN_CARGO="$TOOLCHAIN_BIN_DIR/cargo"

rustup component add llvm-tools-preview --toolchain "$TOOLCHAIN" >/dev/null

SYSROOT="$("${RUSTUP[@]}" rustc --print sysroot)"
HOST="$("${RUSTUP[@]}" rustc -vV | sed -n 's|host: ||p')"
RUST_BIN_DIR="$SYSROOT/lib/rustlib/$HOST/bin"

if [[ -x "$RUST_BIN_DIR/rust-lld" ]]; then
  LINKER="$RUST_BIN_DIR/rust-lld"
elif command -v ld.lld >/dev/null 2>&1; then
  LINKER="$(command -v ld.lld)"
else
  echo "memory-control matrix requires rust-lld or ld.lld" >&2
  exit 1
fi

if [[ -x "$RUST_BIN_DIR/llvm-size" ]]; then
  LLVM_SIZE="$RUST_BIN_DIR/llvm-size"
elif command -v llvm-size >/dev/null 2>&1; then
  LLVM_SIZE="$(command -v llvm-size)"
elif [[ -x /opt/homebrew/opt/llvm/bin/llvm-size ]]; then
  LLVM_SIZE="/opt/homebrew/opt/llvm/bin/llvm-size"
else
  echo "memory-control matrix requires llvm-size" >&2
  exit 1
fi

for count in "$@"; do
  case "$count" in
    1|4|8) ;;
    *)
      echo "unsupported memory-control matrix count: $count" >&2
      exit 1
      ;;
  esac

  feature="memory-control-${count}"
  count_target_dir="$TARGET_DIR/count_${count}"
  PATH="$TOOLCHAIN_BIN_DIR:$PATH" \
  RUSTC="$TOOLCHAIN_RUSTC" \
  CARGO_TERM_COLOR=never \
  CARGO_TERM_PROGRESS_WHEN=never \
  TERM=dumb \
    "$TOOLCHAIN_CARGO" rustc \
      --manifest-path "$SMOKE_MANIFEST" \
      --release \
      --target "$TARGET" \
      --target-dir "$count_target_dir" \
      --features "$feature" \
      --config "target.$TARGET.linker = '$LINKER'" \
      -- \
      -C "link-arg=-T$LINKER_SCRIPT" \
      -C link-arg=--gc-sections \
      >/dev/null

  bin="$count_target_dir/$TARGET/release/$SMOKE_NAME"
  if [[ ! -f "$bin" ]]; then
    echo "memory-control matrix binary missing: $bin" >&2
    exit 1
  fi

  "$LLVM_SIZE" --format=sysv "$bin" \
    | awk -v count="$count" -v owner="hibana-internal-pico-smoke" '
          $1 ~ /^\.text/ || $1 == "__text" { text += $2 }
          $1 ~ /^\.rodata/ || $1 == "__const" || $1 == "__cstring" { rodata += $2 }
          $1 ~ /^\.data/ || $1 == "__data" { data += $2 }
          $1 ~ /^\.bss/ || $1 == "__bss" || $1 == "__common" || $1 == "__thread_bss" {
            bss += $2
          }
          END {
            printf("memory-control matrix count=%d section=.text bytes=%d owner=%s path=internal-pico-smoke\n", count, text, owner)
            printf("memory-control matrix count=%d section=.rodata bytes=%d owner=%s path=internal-pico-smoke\n", count, rodata, owner)
            printf("memory-control matrix count=%d section=.data bytes=%d owner=%s path=internal-pico-smoke\n", count, data, owner)
            printf("memory-control matrix count=%d section=.bss bytes=%d owner=%s path=internal-pico-smoke\n", count, bss, owner)
          }
        '
done
