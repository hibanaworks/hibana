#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "tracked worktree closure check not run outside git worktree"
  exit 0
fi

FAILED=0
while IFS= read -r path; do
  case "${path}" in
    src/*|tests/*|.github/scripts/*|.github/maintainability/*|.github/workflows/*)
      echo "tracked worktree closure violation: required source/CI file is untracked: ${path}" >&2
      FAILED=1
      ;;
  esac
done < <(git ls-files --others --exclude-standard)

if (( FAILED != 0 )); then
  echo "stage or remove untracked source/CI files before running final-form gates" >&2
  exit 1
fi

echo "tracked worktree closure check passed"
