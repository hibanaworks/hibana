#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

if rg -n '#!\[feature|RUSTC_BOOTSTRAP|build-std|target-json|custom JSON target|custom target spec' \
  src tests examples integration internal Cargo.toml Cargo.lock .cargo .github \
  --glob '!.github/scripts/check_no_nightly_features.sh' \
  --glob '!.github/scripts/check_no_custom_target_json.sh' 2>/dev/null; then
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
