#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

LIMIT="${SOURCE_FILE_LINE_LIMIT:-1000}"
HOTSPOT_LIMIT="${SOURCE_NEAR_CEILING_LINE_LIMIT:-900}"
HOTSPOT_MANIFEST=".github/maintainability/near_ceiling_source_budget.tsv"
FAILED=0
declare -A HOTSPOT_BUDGETS=()
declare -A HOTSPOT_SPLIT_TARGETS=()

while IFS=$'\t' read -r file max_lines split_target; do
    [[ -z "${file}" || "${file}" == \#* ]] && continue
    HOTSPOT_BUDGETS["${file}"]="${max_lines}"
    HOTSPOT_SPLIT_TARGETS["${file}"]="${split_target:-}"
    if [[ -z "${split_target:-}" ]]; then
        echo "source file budget violation: ${file} near-ceiling budget needs a split target owner" >&2
        FAILED=1
    fi
done < "${HOTSPOT_MANIFEST}"

while IFS= read -r -d '' file; do
    case "${file}" in
        src/**/tests.rs|src/**/*_tests.rs|src/**/test_support/*|src/**/tests/*)
            continue
            ;;
    esac

    lines="$(wc -l < "${file}" | tr -d ' ')"
    if (( lines > LIMIT )); then
        echo "source file budget violation: ${file} has ${lines} lines (>${LIMIT})" >&2
        FAILED=1
    fi
    if (( lines >= HOTSPOT_LIMIT )); then
        budget="${HOTSPOT_BUDGETS[${file}]:-}"
        if [[ -z "${budget}" ]]; then
            echo "source file budget violation: ${file} has ${lines} lines and needs a near-ceiling budget entry" >&2
            FAILED=1
        elif (( lines > budget )); then
            echo "source file budget violation: ${file} grew to ${lines} lines (>${budget} near-ceiling budget)" >&2
            FAILED=1
        fi
    fi
done < <(find src .github/scripts -type f \( -name '*.rs' -o -name '*.sh' \) -print0)

while IFS=$'\t' read -r file max_lines split_target; do
    [[ -z "${file}" || "${file}" == \#* ]] && continue
    if [[ ! -f "${file}" ]]; then
        echo "source file budget violation: stale near-ceiling budget entry: ${file}" >&2
        FAILED=1
        continue
    fi
    lines="$(wc -l < "${file}" | tr -d ' ')"
    if (( lines < HOTSPOT_LIMIT )); then
        echo "source file budget violation: remove resolved near-ceiling budget entry: ${file}" >&2
        FAILED=1
    elif (( lines != max_lines )); then
        echo "source file budget violation: ${file} near-ceiling budget must ratchet to the current line count (${lines}, manifest ${max_lines})" >&2
        FAILED=1
    fi
done < "${HOTSPOT_MANIFEST}"

if (( FAILED != 0 )); then
    exit 1
fi

echo "source file budget check passed"
