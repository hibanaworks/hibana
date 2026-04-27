#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

if find . \
  -path './target' -prune -o \
  -path '*/target' -prune -o \
  -path './.git' -prune -o \
  -name '*.json' -print | rg -n '(^|/)targets?/|thumb|riscv|aarch|x86|custom|target' >/dev/null; then
  echo "custom target JSON specs are forbidden" >&2
  exit 1
fi

if rg -n 'custom target|target-json|build-std|RUSTC_BOOTSTRAP' \
  .github Cargo.toml Cargo.lock .cargo \
  --glob '!.github/scripts/check_no_custom_target_json.sh' \
  --glob '!.github/scripts/check_no_nightly_features.sh' 2>/dev/null; then
  echo "custom target or bootstrap path is forbidden" >&2
  exit 1
fi

echo "no custom target JSON check passed"
