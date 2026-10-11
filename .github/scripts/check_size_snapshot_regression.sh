#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"
rustup target add --toolchain "${TOOLCHAIN}" thumbv6m-none-eabi >/dev/null

RUSTUP=(rustup run "${TOOLCHAIN}")
TOOLCHAIN_RUSTC="$(rustup which --toolchain "${TOOLCHAIN}" rustc)"
TOOLCHAIN_BIN_DIR="$(dirname "${TOOLCHAIN_RUSTC}")"
TOOLCHAIN_CARGO="${TOOLCHAIN_BIN_DIR}/cargo"
SYSROOT="$("${RUSTUP[@]}" rustc --print sysroot)"
HOST="$("${RUSTUP[@]}" rustc --print host-tuple)"
RUST_BIN_DIR="${SYSROOT}/lib/rustlib/${HOST}/bin"
LLVM_SIZE="${RUST_BIN_DIR}/llvm-size"
SIZE_TOOL="${ROOT_DIR}/.github/scripts/lib/size_snapshot_tool.sh"

if [[ ! -x "${LLVM_SIZE}" ]]; then
  echo "size snapshot regression check requires llvm-size" >&2
  exit 1
fi

WORK_ROOT="${HIBANA_SIZE_WORKTREE_ROOT:-${TMPDIR:-/tmp}/hibana-size-snapshot-${$}}"
CURRENT_WORKTREE="${WORK_ROOT}/current"
SNAPSHOT_DIR="${WORK_ROOT}/snapshots"
mkdir -p "${SNAPSHOT_DIR}"

cleanup() {
  if ! git -C "${ROOT_DIR}" worktree remove "${CURRENT_WORKTREE}" --force >/dev/null 2>&1; then
    :
  fi
  rm -rf "${WORK_ROOT}"
}
trap cleanup EXIT

tree_is_clean() {
  [[ -z "$(git status --porcelain --untracked-files=normal -- .)" ]]
}

CURRENT_REF="${HIBANA_SIZE_CURRENT_REF:-HEAD}"

if tree_is_clean; then
  git worktree add --detach "${CURRENT_WORKTREE}" "${CURRENT_REF}" >/dev/null
  CURRENT_TREE="${CURRENT_WORKTREE}"
  CURRENT_LABEL="$(git -C "${CURRENT_WORKTREE}" rev-parse --short HEAD)"
else
  CURRENT_TREE="${ROOT_DIR}"
  CURRENT_LABEL="working-tree"
fi

measure_tree() {
  local label="$1"
  local tree="$2"
  local out_json="$3"
  local target_dir="${WORK_ROOT}/target-${label}"

  echo "== measuring ${label} (${tree}) =="
  CARGO_TERM_COLOR=never \
  CARGO_TERM_PROGRESS_WHEN=never \
  TERM=dumb \
  PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
  CARGO_TARGET_DIR="${target_dir}" \
    "${TOOLCHAIN_CARGO}" build \
      --manifest-path "${tree}/Cargo.toml" \
      -p hibana \
      --no-default-features \
      --target thumbv6m-none-eabi \
      --release \
      --lib \
      >/dev/null

  local thumb_rlib="${target_dir}/thumbv6m-none-eabi/release/libhibana.rlib"
  if [[ ! -f "${thumb_rlib}" ]]; then
    echo "missing thumb measurement artifact for ${label}: ${thumb_rlib}" >&2
    exit 1
  fi

  local section_output
  "${LLVM_SIZE}" --format=sysv "${thumb_rlib}" > "${out_json}.sections.raw"
  section_output="$(bash "${SIZE_TOOL}" sections "${out_json}.sections.raw" section)"
  printf '%s\n' "${section_output}" > "${out_json}.sections"
  printf '%s\n' "${section_output}"

  local thumbv6m_example_manifest="${tree}/examples/pico/Cargo.toml"
  if [[ ! -f "${thumbv6m_example_manifest}" ]]; then
    echo "missing tracked thumbv6m projection example for ${label}: ${thumbv6m_example_manifest}" >&2
    exit 1
  fi
  CARGO_TERM_COLOR=never \
  CARGO_TERM_PROGRESS_WHEN=never \
  TERM=dumb \
  PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
  CARGO_TARGET_DIR="${target_dir}" \
    "${TOOLCHAIN_CARGO}" build \
      --manifest-path "${thumbv6m_example_manifest}" \
      --no-default-features \
      --target thumbv6m-none-eabi \
      --release \
      --lib \
      >/dev/null

  local projected_rlib="${target_dir}/thumbv6m-none-eabi/release/libhibana_pico_projection_example.rlib"
  if [[ ! -f "${projected_rlib}" ]]; then
    echo "missing projected RoleProgram measurement artifact for ${label}: ${projected_rlib}" >&2
    exit 1
  fi
  local projected_output
  "${LLVM_SIZE}" --format=sysv "${projected_rlib}" > "${out_json}.projected.raw"
  projected_output="$(bash "${SIZE_TOOL}" sections "${out_json}.projected.raw" 'projected section')"
  printf '%s\n' "${projected_output}" > "${out_json}.projected"
  printf '%s\n' "${projected_output}"

  local stack_output
  stack_output="$(
    CARGO_TERM_COLOR=never \
    CARGO_TERM_PROGRESS_WHEN=never \
    TERM=dumb \
    PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
    CARGO_TARGET_DIR="${target_dir}" \
      "${TOOLCHAIN_CARGO}" test \
        --manifest-path "${tree}/Cargo.toml" \
        -p hibana \
        large_choreography_runtime_peak_metrics \
        --lib \
        --release \
        -- \
        --ignored \
        --nocapture \
        --test-threads=1
  )"
  printf '%s\n' "${stack_output}"

  printf '%s\n' "${stack_output}" > "${out_json}.stack"
  bash "${SIZE_TOOL}" capture "${label}" "${out_json}.sections" \
    "${out_json}.projected" "${out_json}.stack" "${out_json}"
}

CURRENT_JSON="${SNAPSHOT_DIR}/current.json"
measure_tree "current-${CURRENT_LABEL}" "${CURRENT_TREE}" "${CURRENT_JSON}"

bash "${SIZE_TOOL}" compare "${CURRENT_JSON}" \
  "${ROOT_DIR}/.github/measurement_snapshots/hibana-size-snapshot.json" \
  "${HIBANA_MODELED_RUNTIME_SRAM_MIN_HEADROOM_BYTES-64}"

echo "size snapshot check passed: current=${CURRENT_LABEL}"
