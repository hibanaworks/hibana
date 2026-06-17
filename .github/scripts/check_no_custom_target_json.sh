#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

source ./.github/scripts/lib/hygiene_common.sh

FAILED=0

json_paths="$(
  find . \
  -path './target' -prune -o \
  -path '*/target' -prune -o \
  -path './.git' -prune -o \
  -name '*.json' -print
)"
json_matches=""
if [[ -n "${json_paths}" ]]; then
  capture_pipe_rg json_matches \
    "custom target JSON specs" \
    "${json_paths}" \
    -n '(^|/)targets?/|thumb|riscv|aarch|x86|custom|target'
fi
if [[ -n "${json_matches}" ]]; then
  echo "${json_matches}" >&2
  echo "custom target JSON specs are forbidden" >&2
  FAILED=1
fi

check_absent \
  'custom target|target-json|build-std|RUSTC_BOOTSTRAP' \
  "custom target or bootstrap path is forbidden" \
  .github Cargo.toml Cargo.lock \
  --optional .cargo \
  --glob '!.github/scripts/check_no_custom_target_json.sh' \
  --glob '!.github/scripts/check_no_nightly_features.sh'

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "no custom target JSON check passed"
