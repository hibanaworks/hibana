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
BASE_WORKTREE="${WORK_ROOT}/base"
CURRENT_WORKTREE="${WORK_ROOT}/current"
SNAPSHOT_DIR="${WORK_ROOT}/snapshots"
mkdir -p "${SNAPSHOT_DIR}"

cleanup() {
  git -C "${ROOT_DIR}" worktree remove "${BASE_WORKTREE}" --force >/dev/null 2>&1 || true
  git -C "${ROOT_DIR}" worktree remove "${CURRENT_WORKTREE}" --force >/dev/null 2>&1 || true
  rm -rf "${WORK_ROOT}"
}
trap cleanup EXIT

tree_is_clean() {
  [[ -z "$(git status --porcelain --untracked-files=normal -- .)" ]]
}

PUBLISHED_CRATES_IO_0_8_0_REF="${HIBANA_SIZE_PUBLISHED_BASE_REF:-d95e83eb503f35f8beeb60a29d41b4cf6a8d5290}"

if [[ -n "${HIBANA_SIZE_BASE_REF:-}" ]]; then
  BASE_REF="${HIBANA_SIZE_BASE_REF}"
else
  BASE_REF="${PUBLISHED_CRATES_IO_0_8_0_REF}"
fi

if ! git rev-parse --verify "${BASE_REF}^{commit}" >/dev/null 2>&1; then
  echo "size snapshot regression check cannot resolve base ref: ${BASE_REF}" >&2
  echo "Default base is the crates.io 0.8.0 publish commit. Set HIBANA_SIZE_BASE_REF or HIBANA_SIZE_PUBLISHED_BASE_REF explicitly in shallow checkouts." >&2
  exit 1
fi

CURRENT_REF="${HIBANA_SIZE_CURRENT_REF:-HEAD}"
git worktree add --detach "${BASE_WORKTREE}" "${BASE_REF}" >/dev/null

if tree_is_clean; then
  git worktree add --detach "${CURRENT_WORKTREE}" "${CURRENT_REF}" >/dev/null
  CURRENT_TREE="${CURRENT_WORKTREE}"
  CURRENT_LABEL="$(git -C "${CURRENT_WORKTREE}" rev-parse --short HEAD)"
else
  CURRENT_TREE="${ROOT_DIR}"
  CURRENT_LABEL="working-tree"
fi
BASE_LABEL="$(git -C "${BASE_WORKTREE}" rev-parse --short HEAD)"

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

  local projected_crate="${WORK_ROOT}/projected-${label}"
  mkdir -p "${projected_crate}/src"
  cat >"${projected_crate}/Cargo.toml" <<EOF
[package]
name = "hibana-projected-measure"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
name = "hibana_projected_measure"

[dependencies]
hibana = { path = "${tree}", default-features = false }
EOF
  python3 - "${tree}" "${projected_crate}/src/lib.rs" <<'PY'
import sys
import re
from pathlib import Path

tree = Path(sys.argv[1])
dst = sys.argv[2]
global_source = (tree / "src" / "global.rs").read_text(encoding="utf-8")
g_source = (tree / "src" / "g.rs").read_text(encoding="utf-8")
send_signatures = re.findall(
    r"pub\s+const\s+fn\s+send\s*<(?P<generics>[^>]*)>",
    g_source + "\n" + global_source,
    flags=re.S,
)
const_role_send = any("const FROM" in sig for sig in send_signatures)
lane_send = any("const LANE" in sig for sig in send_signatures)

def send_expr(idx: int) -> str:
    label = 1 + (idx % 46)
    lane = idx % 4
    if const_role_send:
        if lane_send:
            return f"g::send::<0, 1, g::Msg<{label}, ()>, {lane}>()"
        return f"g::send::<0, 1, g::Msg<{label}, ()>>()"
    if lane_send:
        return f"g::send::<g::Role<0>, g::Role<1>, g::Msg<{label}, ()>, {lane}>()"
    return f"g::send::<g::Role<0>, g::Role<1>, g::Msg<{label}, ()>>()"

def seq_expr(start: int, end: int) -> str:
    if end - start == 1:
        return send_expr(start)
    mid = start + ((end - start) // 2)
    return f"g::seq({seq_expr(start, mid)}, {seq_expr(mid, end)})"

program = seq_expr(0, 32)
with open(dst, "w", encoding="utf-8") as f:
    f.write(
        "#![no_std]\n"
        "use hibana::g;\n"
        "use hibana::integration::program::{project, RoleProgram};\n\n"
        "#[inline(never)]\n"
        "pub fn projected_pair() -> (RoleProgram<0>, RoleProgram<1>) {\n"
        f"    let program = {program};\n"
        "    (project(&program), project(&program))\n"
        "}\n"
    )
PY
  CARGO_TERM_COLOR=never \
  CARGO_TERM_PROGRESS_WHEN=never \
  TERM=dumb \
  PATH="${TOOLCHAIN_BIN_DIR}:$PATH" \
  CARGO_TARGET_DIR="${target_dir}" \
    "${TOOLCHAIN_CARGO}" build \
      --manifest-path "${projected_crate}/Cargo.toml" \
      --no-default-features \
      --target thumbv6m-none-eabi \
      --release \
      --lib \
      >/dev/null

  local projected_rlib="${target_dir}/thumbv6m-none-eabi/release/libhibana_projected_measure.rlib"
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
        --features std \
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
    if "localside_peak_stack_bytes" not in metrics and os.environ["LABEL"].startswith("base-"):
        metrics["localside_peak_stack_bytes"] = metrics.get("peak_stack_bytes", 0)
    runtime_shapes[shape_name] = metrics

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

BASE_JSON="${SNAPSHOT_DIR}/base.json"
CURRENT_JSON="${SNAPSHOT_DIR}/current.json"
measure_tree "base-${BASE_LABEL}" "${BASE_WORKTREE}" "${BASE_JSON}"
measure_tree "current-${CURRENT_LABEL}" "${CURRENT_TREE}" "${CURRENT_JSON}"

BASE_JSON="${BASE_JSON}" CURRENT_JSON="${CURRENT_JSON}" SNAPSHOT_FILE="${ROOT_DIR}/.github/measurement_snapshots/hibana-size-snapshot.json" python3 - <<'PY'
import json
import os
import sys

with open(os.environ["BASE_JSON"], "r", encoding="utf-8") as f:
    base = json.load(f)
with open(os.environ["CURRENT_JSON"], "r", encoding="utf-8") as f:
    current = json.load(f)
with open(os.environ["SNAPSHOT_FILE"], "r", encoding="utf-8") as f:
    budget_snapshot = json.load(f)

failures = []
fail_on_published_baseline = (
    os.environ.get("HIBANA_FAIL_ON_PUBLISHED_BASELINE_REGRESSION", "0") == "1"
)

expected_shapes = {"route_heavy", "linear_heavy", "fanout_heavy"}
runtime_metrics = {
    "sidecar_scratch_high_water_bytes",
    "live_endpoint_bytes",
    "peak_live_slab_bytes",
    "localside_peak_stack_bytes",
    "peak_stack_bytes",
}

for label, snapshot in [("base", base), ("current", current)]:
    shapes = snapshot.get("runtime_shapes", {})
    missing_shapes = sorted(expected_shapes - set(shapes))
    if missing_shapes:
        failures.append(
            f"{label} runtime snapshot missing shapes: {', '.join(missing_shapes)}"
        )
        continue
    for shape in sorted(expected_shapes):
        missing_metrics = sorted(runtime_metrics - set(shapes[shape]))
        if missing_metrics:
            failures.append(
                f"{label} runtime snapshot shape={shape} missing metrics: "
                + ", ".join(missing_metrics)
            )

for key in [".text", ".rodata", ".data", ".bss", "flash_total"]:
    old = base["sections"].get(key, 0)
    new = current["sections"].get(key, 0)
    print(f"worktree-snapshot section {key} base={old} current={new} delta={new - old}")

for key in [".text", ".rodata", ".data", ".bss", "flash_total"]:
    old = base.get("projected_sections", {}).get(key, 0)
    new = current.get("projected_sections", {}).get(key, 0)
    print(
        f"worktree-snapshot projected-section {key} "
        f"base={old} current={new} delta={new - old}"
    )

for key in [
    "sidecar_scratch_high_water_bytes",
    "live_endpoint_bytes",
    "peak_live_slab_bytes",
    "peak_stack_bytes",
]:
    old = base["runtime_max"].get(key, 0)
    new = current["runtime_max"].get(key, 0)
    print(f"worktree-snapshot runtime-max {key} base={old} current={new} delta={new - old}")

for shape in sorted(expected_shapes):
    if shape not in base.get("runtime_shapes", {}) or shape not in current.get("runtime_shapes", {}):
        continue
    old = base["runtime_shapes"][shape].get("peak_stack_bytes")
    new = current["runtime_shapes"][shape].get("peak_stack_bytes")
    if old is None or new is None:
        continue
    print(f"worktree-snapshot runtime-shape-stack shape={shape} base={old} current={new} delta={new - old}")
    if fail_on_published_baseline and new > old:
        failures.append(
            f"runtime shape {shape} peak_stack_bytes exceeds published baseline: "
            f"base={old} current={new} delta={new - old}"
        )
    old_local = base["runtime_shapes"][shape].get("localside_peak_stack_bytes")
    new_local = current["runtime_shapes"][shape].get("localside_peak_stack_bytes")
    if old_local is None or new_local is None:
        continue
    print(
        f"worktree-snapshot runtime-shape-localside-stack shape={shape} "
        f"base={old_local} current={new_local} delta={new_local - old_local}"
    )
    if fail_on_published_baseline and new_local > old_local:
        failures.append(
            f"runtime shape {shape} localside_peak_stack_bytes exceeds published baseline: "
            f"base={old_local} current={new_local} delta={new_local - old_local}"
        )

section_budget = budget_snapshot["budget"]["thumbv6m_none_eabi_no_std_release_lib"]["sections"]
for key in [".text", ".rodata", ".data", ".bss", "flash_total"]:
    actual = current["sections"].get(key, 0)
    maximum = section_budget.get(key, 0)
    print(f"worktree-snapshot budget-section {key} actual={actual} budget={maximum}")
    if actual > maximum:
        failures.append(f"section {key} exceeds snapshot budget: actual={actual} budget={maximum}")

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

base_max_stack = base["runtime_max"].get("peak_stack_bytes", 0)
current_max_stack = current["runtime_max"].get("peak_stack_bytes", 0)
budget_max_stack = max(metrics["peak_stack_bytes"] for metrics in runtime_budget.values())
base_sram = (
    base["sections"].get(".data", 0)
    + base["sections"].get(".bss", 0)
    + base["runtime_max"].get("peak_live_slab_bytes", 0)
)
current_sram = (
    current["sections"].get(".data", 0)
    + current["sections"].get(".bss", 0)
    + current["runtime_max"].get("peak_live_slab_bytes", 0)
)
budget_sram = (
    section_budget.get(".data", 0)
    + section_budget.get(".bss", 0)
    + max(metrics["peak_live_slab_bytes"] for metrics in runtime_budget.values())
)
base_flash = base["sections"].get("flash_total", 0)
current_flash = current["sections"].get("flash_total", 0)
aggregate = [
    ("max_stack", base_max_stack, current_max_stack, budget_max_stack),
    ("sram", base_sram, current_sram, budget_sram),
    ("flash", base_flash, current_flash, section_budget["flash_total"]),
]
non_growing = 0
decreased = 0
for name, old, new, maximum in aggregate:
    print(f"worktree-snapshot aggregate {name} base={old} current={new} delta={new - old}")
    print(f"worktree-snapshot budget-aggregate {name} actual={new} budget={maximum}")
    if fail_on_published_baseline and new > old:
        failures.append(
            f"aggregate {name} exceeds published baseline: "
            f"base={old} current={new} delta={new - old}"
        )
    if new > maximum:
        failures.append(f"aggregate {name} exceeds snapshot budget: actual={new} budget={maximum}")
    if new <= maximum:
        non_growing += 1
    if new < maximum:
        decreased += 1
if non_growing < 3 or decreased < 1:
    failures.append(
        "aggregate snapshot budget gate failed: max_stack/sram/flash must all be <= budget "
        "and at least one must decrease below budget"
    )
min_sram_headroom = int(os.environ.get("HIBANA_PICO_SRAM_MIN_HEADROOM_BYTES", "64"))
sram_headroom = budget_sram - current_sram
print(
    f"worktree-snapshot aggregate sram_headroom current={sram_headroom} "
    f"minimum={min_sram_headroom}"
)
if sram_headroom < min_sram_headroom:
    failures.append(
        "aggregate Pico-class SRAM headroom is too small: "
        f"headroom={sram_headroom} minimum={min_sram_headroom}"
    )

if failures:
    print("size snapshot regression detected:", file=sys.stderr)
    for failure in failures:
        print(f"  - {failure}", file=sys.stderr)
    sys.exit(1)
PY

echo "size snapshot regression check passed: base=${BASE_REF} current=${CURRENT_LABEL}"
