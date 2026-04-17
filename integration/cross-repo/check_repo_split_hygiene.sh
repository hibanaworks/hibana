#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HARNESS_CARGO="${ROOT_DIR}/Cargo.toml"
HARNESS_README="${ROOT_DIR}/README.md"

FAILED=0

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  if rg -n "${pattern}" "$@"; then
    echo "cross-repo boundary violation: ${label}" >&2
    FAILED=1
  fi
}

check_absent 'path *= *"(\.\./|/Users/)' \
  "cross-repo harness must not depend on local path manifests" \
  "${HARNESS_CARGO}"
check_absent '\[patch\.crates-io\]' \
  "cross-repo harness must not rely on a local crates.io patch overlay" \
  "${HARNESS_CARGO}" \
  "${HARNESS_README}"
check_absent '\.\./\.\./hibana/tests/|../hibana-epf|../hibana-mgmt' \
  "cross-repo harness docs must not assume sibling checkout layout" \
  "${HARNESS_README}" \
  "${ROOT_DIR}/tests"

for required in \
  'git = "https://github.com/hibanaworks/hibana"' \
  'git = "https://github.com/hibanaworks/hibana-epf"' \
  'git = "https://github.com/hibanaworks/hibana-mgmt"'
do
  if ! grep -Fq "${required}" "${HARNESS_CARGO}"; then
    echo "cross-repo harness must pin GitHub repo dependency: ${required}" >&2
    FAILED=1
  fi
done

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "cross-repo split hygiene check passed"
