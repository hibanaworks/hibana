#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0
source ./.github/scripts/lib/hygiene_common.sh

check_absent \
  '#!\[feature|RUSTC_BOOTSTRAP|build-std|target-json|custom JSON target|custom target spec' \
  "core must stay on stable Rust 1.95" \
  src tests Cargo.toml Cargo.lock .github \
  --glob '!.github/scripts/check_no_nightly_features.sh' \
  --glob '!.github/scripts/check_no_custom_target_json.sh' \
  --glob '!.github/scripts/check_hygiene_roots_fail_closed.sh' \
  --optional .cargo examples

json_paths="$(find . \
  -path './target' -prune -o \
  -path '*/target' -prune -o \
  -path './.git' -prune -o \
  -name '*.json' -print)"
capture_pipe_rg CUSTOM_TARGET_JSON_MATCHES \
  "custom JSON target specs are forbidden" \
  "${json_paths}" \
  -n '(^|/)targets?/|thumb|riscv|aarch|x86|custom|target'
if [[ -n "${CUSTOM_TARGET_JSON_MATCHES}" ]]; then
  echo "${CUSTOM_TARGET_JSON_MATCHES}" >&2
  echo "nightly feature hygiene violation: custom JSON target specs are forbidden" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "no nightly feature check passed"
