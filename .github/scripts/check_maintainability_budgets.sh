#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

is_test_like_source() {
  local file="$1"
  [[ "${file}" == */tests.rs ]] \
    || [[ "${file}" == *_tests.rs ]] \
    || [[ "${file}" == */tests/* ]] \
    || [[ "${file}" == src/test_support/* ]] \
    || [[ "${file}" == src/endpoint/kernel/test_support/* ]]
}

source_lines() {
  local path="$1"
  local mode="${2:-recursive}"
  local find_args=(-type f -name '*.rs' -print0)
  if [[ "${mode}" == "direct" ]]; then
    find "${path}" -maxdepth 1 "${find_args[@]}" \
      | while IFS= read -r -d '' file; do
          if is_test_like_source "${file}"; then
            continue
          fi
          wc -l < "${file}"
        done \
      | awk '{sum += $1} END {print sum + 0}'
    return
  fi
  find "${path}" "${find_args[@]}" \
    | while IFS= read -r -d '' file; do
        if is_test_like_source "${file}"; then
          continue
        fi
        wc -l < "${file}"
      done \
    | awk '{sum += $1} END {print sum + 0}'
}

source_files() {
  local path="$1"
  local mode="${2:-recursive}"
  local find_args=(-type f -name '*.rs' -print0)
  if [[ "${mode}" == "direct" ]]; then
    find "${path}" -maxdepth 1 "${find_args[@]}" \
      | while IFS= read -r -d '' file; do
          if is_test_like_source "${file}"; then
            continue
          fi
          printf '%s\n' "${file}"
        done \
      | wc -l \
      | tr -d ' '
    return
  fi
  find "${path}" "${find_args[@]}" \
    | while IFS= read -r -d '' file; do
        if is_test_like_source "${file}"; then
          continue
        fi
        printf '%s\n' "${file}"
      done \
    | wc -l \
    | tr -d ' '
}

check_owner_budget() {
  local path="$1"
  local max_lines="$2"
  local max_files="$3"
  local mode="${4:-recursive}"
  local lines
  local files
  lines="$(source_lines "${path}" "${mode}")"
  files="$(source_files "${path}" "${mode}")"
  if (( lines > max_lines )); then
    echo "maintainability budget violation: ${path} has ${lines} production lines (>${max_lines})" >&2
    FAILED=1
  fi
  if (( files > max_files )); then
    echo "maintainability budget violation: ${path} has ${files} production files (>${max_files})" >&2
    FAILED=1
  fi
}

owner_budget_manifest=".github/maintainability/owner_budget.tsv"
declare -A OWNER_BUDGET_PATHS=()
while IFS=$'\t' read -r path max_lines max_files mode; do
  [[ -z "${path}" || "${path}" == \#* ]] && continue
  OWNER_BUDGET_PATHS["${path}"]=1
  mode="${mode:-recursive}"
  if (( max_lines > 2000 )); then
    echo "maintainability budget violation: ${path} budget covers ${max_lines} lines; split owner budgets below 2000 lines instead of freezing a broad subsystem" >&2
    FAILED=1
    continue
  fi
  check_owner_budget "${path}" "${max_lines}" "${max_files}" "${mode}"
done < "${owner_budget_manifest}"

if [[ -f ".github/maintainability/owner_budget_semantics.tsv" ]]; then
  echo "maintainability budget violation: owner_budget_semantics.tsv reintroduces path-mirrored leaf owners; use owner_semantics.tsv for semantic owner boundaries" >&2
  FAILED=1
fi

aggregate_budget_manifest=".github/maintainability/owner_aggregate_budget.tsv"
aggregate_budget_count=0
while IFS=$'\t' read -r path max_lines max_files mode; do
  [[ -z "${path}" || "${path}" == \#* ]] && continue
  aggregate_budget_count=$((aggregate_budget_count + 1))
  mode="${mode:-recursive}"
  if (( max_lines > 2000 )); then
    echo "maintainability budget violation: ${path} aggregate budget covers ${max_lines} lines; split authority into sub-owner budgets below 2000 lines" >&2
    FAILED=1
    continue
  fi
  check_owner_budget "${path}" "${max_lines}" "${max_files}" "${mode}"
done < "${aggregate_budget_manifest}"
if (( aggregate_budget_count == 0 )); then
  echo "maintainability budget violation: aggregate owner manifest must contain narrow semantic owner budgets; an empty manifest hides aggregate sprawl" >&2
  FAILED=1
fi

python3 ./.github/scripts/lib/check_owner_partitions.py || FAILED=1

while IFS= read -r -d '' file; do
  if is_test_like_source "${file}"; then
    continue
  fi
  lines="$(wc -l < "${file}" | tr -d ' ')"
  if (( lines >= 500 )) && [[ -z "${OWNER_BUDGET_PATHS[${file}]:-}" ]]; then
    echo "maintainability budget violation: ${file} has ${lines} production lines and needs an explicit owner budget" >&2
    FAILED=1
  fi
done < <(find src -type f -name '*.rs' -print0)

TEST_LIMIT="${TEST_SOURCE_FILE_LINE_LIMIT:-1000}"
test_debt_allowlist=".github/maintainability/test_source_debt_allowlist.txt"
declare -A TEST_DEBT_ALLOWLIST=()
while IFS= read -r file; do
  [[ -z "${file}" || "${file}" == \#* ]] && continue
  if [[ "${ALLOW_TEST_SOURCE_DEBT:-0}" != "1" ]]; then
    echo "test fixture budget violation: ${test_debt_allowlist} must not contain committed debt entries; split oversized test sources instead: ${file}" >&2
    FAILED=1
  fi
  TEST_DEBT_ALLOWLIST["${file}"]=1
done < "${test_debt_allowlist}"
while IFS= read -r -d '' file; do
  lines="$(wc -l < "${file}" | tr -d ' ')"
  if (( lines > TEST_LIMIT )); then
    if [[ -n "${TEST_DEBT_ALLOWLIST[${file}]:-}" ]]; then
      continue
    fi
    echo "test fixture budget violation: ${file} has ${lines} lines (>${TEST_LIMIT}); split it below the per-file budget" >&2
    FAILED=1
  fi
done < <(
  find tests src -type f -name '*.rs' -print0 \
    | while IFS= read -r -d '' file; do
        if is_test_like_source "${file}" || [[ "${file}" == tests/* ]]; then
          printf '%s\0' "${file}"
        fi
      done
)

while IFS= read -r file; do
  [[ -z "${file}" || "${file}" == \#* ]] && continue
  if [[ ! -f "${file}" ]]; then
    echo "test fixture budget violation: stale test debt allowlist entry: ${file}" >&2
    FAILED=1
    continue
  fi
  lines="$(wc -l < "${file}" | tr -d ' ')"
  if (( lines <= TEST_LIMIT )); then
    echo "test fixture budget violation: remove resolved test debt allowlist entry: ${file}" >&2
    FAILED=1
  fi
done < "${test_debt_allowlist}"

if find src tests -type f -name 'part[0-9]*.rs' | grep -q .; then
  echo "test/source decomposition violation: partN.rs shards hide ownership boundaries; use scenario or owner names" >&2
  find src tests -type f -name 'part[0-9]*.rs' >&2
  FAILED=1
fi

SRC_TEST_SUPPORT_PATH_REFS="$(
  rg -n '#\[path = "\.\./src/test_support/' tests --glob '*.rs' || true
)"
if [[ -n "${SRC_TEST_SUPPORT_PATH_REFS}" ]]; then
  echo "test support boundary violation: integration tests must not path-import src/test_support fixtures" >&2
  echo "${SRC_TEST_SUPPORT_PATH_REFS}" >&2
  FAILED=1
fi

LEXICAL_INCLUDES="$(
  while IFS= read -r -d '' file; do
    rg -n '^[[:space:]]*include![[:space:]]*\(' "${file}" || true
  done < <(find src tests -type f -name '*.rs' -print0)
)"
if [[ -n "${LEXICAL_INCLUDES}" ]]; then
  echo "module decomposition violation: Rust source must use real module boundaries instead of include! shards" >&2
  echo "${LEXICAL_INCLUDES}" >&2
  FAILED=1
fi

bash ./.github/scripts/check_semantic_surface_shape_hygiene.sh || FAILED=1

if (( FAILED != 0 )); then
  exit 1
fi

echo "maintainability budget check passed"
