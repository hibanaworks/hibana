#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

LIMIT="${SOURCE_FILE_LINE_LIMIT:-880}"
FAILED=0

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
done < <(find src .github/scripts -type f \( -name '*.rs' -o -name '*.sh' \) -print0)

if (( FAILED != 0 )); then
    exit 1
fi

echo "source file budget check passed"
