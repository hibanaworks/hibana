#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0
source ./.github/scripts/lib/hygiene_common.sh

paths=(src tests Cargo.toml Cargo.lock .github)
if [[ -e .cargo ]]; then
  paths+=(.cargo)
fi
if [[ -e examples ]]; then
  paths+=(examples)
fi

set +e
roots="$(existing_roots "${paths[@]}")"
root_status=$?
set -e
if [[ "${root_status}" -ne 0 ]]; then
  echo "nightly feature hygiene violation: root mismatch" >&2
  FAILED=1
elif rg -n '#!\[feature|RUSTC_BOOTSTRAP|build-std|target-json|custom JSON target|custom target spec' \
  ${roots} \
  --glob '!.github/scripts/check_no_nightly_features.sh' \
  --glob '!.github/scripts/check_no_custom_target_json.sh'; then
  echo "nightly feature hygiene violation: core must stay on stable Rust 1.95" >&2
  FAILED=1
fi

if find . \
  -path './target' -prune -o \
  -path '*/target' -prune -o \
  -path './.git' -prune -o \
  -name '*.json' -print | rg -n '(^|/)targets?/|thumb|riscv|aarch|x86|custom|target' >/dev/null; then
  echo "nightly feature hygiene violation: custom JSON target specs are forbidden" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "no nightly feature check passed"
