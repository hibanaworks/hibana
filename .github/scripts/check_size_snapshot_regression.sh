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
HOST="$("${RUSTUP[@]}" rustc -vV | sed -n 's|host: ||p')"
RUST_BIN_DIR="${SYSROOT}/lib/rustlib/${HOST}/bin"
LLVM_SIZE="${RUST_BIN_DIR}/llvm-size"

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
  section_output="$("${LLVM_SIZE}" --format=sysv "${thumb_rlib}" \
    | awk '
        $1 ~ /^\.text/ || $1 == "__text" { text += $2 }
        $1 ~ /^\.rodata/ || $1 == "__const" || $1 == "__cstring" { rodata += $2 }
        $1 ~ /^\.data/ || $1 == "__data" { data += $2 }
        $1 ~ /^\.bss/ || $1 == "__bss" || $1 == "__common" || $1 == "__thread_bss" {
          bss += $2
        }
        END {
          printf("section .text %d\n", text)
          printf("section .rodata %d\n", rodata)
          printf("section .data %d\n", data)
          printf("section .bss %d\n", bss)
        }
      ')"
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
  projected_output="$("${LLVM_SIZE}" --format=sysv "${projected_rlib}" \
    | awk '
        $1 ~ /^\.text/ || $1 == "__text" { text += $2 }
        $1 ~ /^\.rodata/ || $1 == "__const" || $1 == "__cstring" { rodata += $2 }
        $1 ~ /^\.data/ || $1 == "__data" { data += $2 }
        $1 ~ /^\.bss/ || $1 == "__bss" || $1 == "__common" || $1 == "__thread_bss" {
          bss += $2
        }
        END {
          printf("projected section .text %d\n", text)
          printf("projected section .rodata %d\n", rodata)
          printf("projected section .data %d\n", data)
          printf("projected section .bss %d\n", bss)
        }
      ')"
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

  LABEL="${label}" \
  SECTION_OUTPUT="${section_output}" \
  PROJECTED_OUTPUT="${projected_output}" \
  STACK_OUTPUT="${stack_output}" \
  OUT_JSON="${out_json}" \
  python3 - <<'PY'
import json
import os
import re

sections = {}
for line in os.environ["SECTION_OUTPUT"].splitlines():
    match = re.match(r"section (\.[A-Za-z0-9_]+) ([0-9]+)", line)
    if match:
        sections[match.group(1)] = int(match.group(2))
sections["flash_total"] = sections.get(".text", 0) + sections.get(".rodata", 0) + sections.get(".data", 0)

projected_sections = {}
for line in os.environ["PROJECTED_OUTPUT"].splitlines():
    match = re.match(r"projected section (\.[A-Za-z0-9_]+) ([0-9]+)", line)
    if match:
        projected_sections[match.group(1)] = int(match.group(2))
projected_sections["flash_total"] = (
    projected_sections.get(".text", 0)
    + projected_sections.get(".rodata", 0)
    + projected_sections.get(".data", 0)
)

runtime_shapes = {}
for line in os.environ["STACK_OUTPUT"].splitlines():
    if "large-choreography-runtime " not in line:
        continue
    shape = re.search(r"shape=([A-Za-z0-9_]+)", line)
    if not shape:
        continue
    shape_name = shape.group(1)
    metrics = {
        key: int(value)
        for key, value in re.findall(r"([A-Za-z0-9_]+)=([0-9]+)", line)
    }
    runtime_shapes[shape_name] = metrics

data_bss_bytes = sections.get(".data", 0) + sections.get(".bss", 0)
for metrics in runtime_shapes.values():
    if metrics.get("resident_prefix_bytes", 0) < metrics.get("tap_ring_bytes", 0):
        raise SystemExit(
            "resident_prefix_bytes must include the internal tap ring carved before the runtime slab"
        )
    # resident_prefix_bytes is the full pre-runtime carve:
    # Rendezvous header, transport T field, alignment padding, and tap ring.
    metrics["modeled_runtime_sram_bytes"] = (
        data_bss_bytes
        + metrics.get("session_kit_storage_bytes", 0)
        + metrics.get("resident_prefix_bytes", 0)
        + metrics.get("peak_live_slab_bytes", 0)
        + metrics.get("localside_peak_stack_bytes", 0)
    )

runtime_max = {}
for metrics in runtime_shapes.values():
    for key, value in metrics.items():
        if key == "slab_bytes":
            continue
        runtime_max[key] = max(runtime_max.get(key, 0), value)

with open(os.environ["OUT_JSON"], "w", encoding="utf-8") as f:
    json.dump(
        {
            "label": os.environ["LABEL"],
            "sections": sections,
            "projected_sections": projected_sections,
            "runtime_shapes": runtime_shapes,
            "runtime_max": runtime_max,
        },
        f,
        indent=2,
        sort_keys=True,
    )
    f.write("\n")
PY
}

CURRENT_JSON="${SNAPSHOT_DIR}/current.json"
measure_tree "current-${CURRENT_LABEL}" "${CURRENT_TREE}" "${CURRENT_JSON}"

CURRENT_JSON="${CURRENT_JSON}" SNAPSHOT_FILE="${ROOT_DIR}/.github/measurement_snapshots/hibana-size-snapshot.json" python3 - <<'PY'
import json
import os
import sys

with open(os.environ["CURRENT_JSON"], "r", encoding="utf-8") as f:
    current = json.load(f)
with open(os.environ["SNAPSHOT_FILE"], "r", encoding="utf-8") as f:
    budget_snapshot = json.load(f)

failures = []

expected_shapes = {"route_heavy", "linear_heavy", "fanout_heavy"}
runtime_metrics = {
    "session_kit_storage_bytes",
    "resident_prefix_bytes",
    "tap_ring_bytes",
    "sidecar_scratch_high_water_bytes",
    "live_endpoint_bytes",
    "peak_live_slab_bytes",
    "localside_peak_stack_bytes",
    "peak_stack_bytes",
    "modeled_runtime_sram_bytes",
}

shapes = current.get("runtime_shapes", {})
missing_shapes = sorted(expected_shapes - set(shapes))
if missing_shapes:
    failures.append(
        f"current runtime snapshot missing shapes: {', '.join(missing_shapes)}"
    )
for shape in sorted(expected_shapes & set(shapes)):
    missing_metrics = sorted(runtime_metrics - set(shapes[shape]))
    if missing_metrics:
        failures.append(
            f"current runtime snapshot shape={shape} missing metrics: "
            + ", ".join(missing_metrics)
        )

section_budget = budget_snapshot["budget"]["thumbv6m_none_eabi_no_std_release_lib"]["sections"]
for key in [".text", ".rodata", ".data", ".bss", "flash_total"]:
    actual = current["sections"].get(key, 0)
    maximum = section_budget.get(key, 0)
    print(f"worktree-snapshot budget-section {key} actual={actual} budget={maximum}")
    if actual > maximum:
        failures.append(f"section {key} exceeds snapshot budget: actual={actual} budget={maximum}")

for key in [".text", ".rodata", ".data", ".bss", "flash_total"]:
    actual = current.get("projected_sections", {}).get(key, 0)
    maximum = section_budget.get(key, 0)
    print(f"worktree-snapshot budget-projected-section {key} actual={actual} budget={maximum}")
    if actual > maximum:
        failures.append(f"projected section {key} exceeds snapshot budget: actual={actual} budget={maximum}")

runtime_budget = budget_snapshot["budget"]["runtime_shapes"]
for shape in sorted(expected_shapes):
    current_shape = current.get("runtime_shapes", {}).get(shape)
    if current_shape is None:
        continue
    for key, maximum in sorted(runtime_budget.get(shape, {}).items()):
        actual = current_shape.get(key)
        if actual is None:
            continue
        print(f"worktree-snapshot budget-runtime shape={shape} {key} actual={actual} budget={maximum}")
        if actual > maximum:
            failures.append(
                f"runtime shape {shape} {key} exceeds snapshot budget: "
                f"actual={actual} budget={maximum}"
            )

current_max_stack = current["runtime_max"].get("peak_stack_bytes", 0)
budget_max_stack = max(metrics["peak_stack_bytes"] for metrics in runtime_budget.values())
current_sram = current["runtime_max"].get("modeled_runtime_sram_bytes", 0)
budget_sram = max(metrics["modeled_runtime_sram_bytes"] for metrics in runtime_budget.values())
current_flash = current["sections"].get("flash_total", 0)
aggregate = [
    ("max_stack", current_max_stack, budget_max_stack),
    ("sram", current_sram, budget_sram),
    ("flash", current_flash, section_budget["flash_total"]),
]
non_growing = 0
decreased = 0
for name, actual, maximum in aggregate:
    print(f"worktree-snapshot budget-aggregate {name} actual={actual} budget={maximum}")
    if actual > maximum:
        failures.append(f"aggregate {name} exceeds snapshot budget: actual={actual} budget={maximum}")
    if actual <= maximum:
        non_growing += 1
    if actual < maximum:
        decreased += 1
if non_growing < 3 or decreased < 1:
    failures.append(
        "aggregate snapshot budget gate failed: max_stack/sram/flash must all be <= budget "
        "and at least one must decrease below budget"
    )
min_sram_headroom = int(os.environ.get("HIBANA_MODELED_RUNTIME_SRAM_MIN_HEADROOM_BYTES", "64"))
sram_headroom = budget_sram - current_sram
print(
    f"worktree-snapshot aggregate sram_headroom current={sram_headroom} "
    f"minimum={min_sram_headroom}"
)
if sram_headroom < min_sram_headroom:
    failures.append(
        "aggregate modeled runtime SRAM headroom is too small: "
        f"headroom={sram_headroom} minimum={min_sram_headroom}"
    )

if failures:
    print("size snapshot regression detected:", file=sys.stderr)
    for failure in failures:
        print(f"  - {failure}", file=sys.stderr)
    sys.exit(1)
PY

echo "size snapshot check passed: current=${CURRENT_LABEL}"
