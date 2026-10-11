#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="${ROOT_DIR}/proofs/unix-carrier/Cargo.toml"
TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
TARGET_DIR="${CARGO_TARGET_DIR:-${ROOT_DIR}/target/unix-carrier-proof}"

case "$(uname -s)" in
  Darwin|Linux) ;;
  *)
    echo "Unix carrier proof gate requires an AF_UNIX host" >&2
    exit 1
    ;;
esac

if [[ ! -f "${MANIFEST}" ]]; then
  echo "Unix carrier proof manifest is missing" >&2
  exit 1
fi

CARGO_BUILD_JOBS=1 \
  CARGO_TARGET_DIR="${TARGET_DIR}" \
  RUST_TEST_THREADS=1 \
  cargo +"${TOOLCHAIN}" test --locked --manifest-path "${MANIFEST}"
CARGO_BUILD_JOBS=1 \
  CARGO_TARGET_DIR="${TARGET_DIR}" \
  cargo +"${TOOLCHAIN}" clippy --locked --manifest-path "${MANIFEST}" \
    --all-targets -- -D warnings

echo "Unix carrier proof gate passed medium=unix-datagram profile=closing fifo=1 no-replay=1 peer-binding=1 close-wake=1 generation-isolation=1"
