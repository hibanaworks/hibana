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
  set +e
  json_matches="$(printf '%s\n' "${json_paths}" | rg -n '(^|/)targets?/|thumb|riscv|aarch|x86|custom|target')"
  status=$?
  set -e
  if [[ "${status}" -gt 1 ]]; then
    echo "custom target JSON search failed" >&2
    FAILED=1
  fi
fi
if [[ -n "${json_matches}" ]]; then
  echo "${json_matches}" >&2
  echo "custom target JSON specs are forbidden" >&2
  FAILED=1
fi

roots=()
required_roots="$(existing_roots .github Cargo.toml Cargo.lock)" || {
  echo "custom target deny root mismatch" >&2
  exit 1
}
while IFS= read -r root; do
  [[ -n "${root}" ]] && roots+=("${root}")
done <<< "${required_roots}"
optional_root_list="$(optional_roots .cargo)"
while IFS= read -r root; do
  [[ -n "${root}" ]] && roots+=("${root}")
done <<< "${optional_root_list}"

target_matches=""
capture_rg target_matches \
  "custom target or bootstrap path" \
  -n \
  --glob '!.github/scripts/check_no_custom_target_json.sh' \
  --glob '!.github/scripts/check_no_nightly_features.sh' \
  'custom target|target-json|build-std|RUSTC_BOOTSTRAP' \
  "${roots[@]}"
if [[ -n "${target_matches}" ]]; then
  echo "${target_matches}" >&2
  echo "custom target or bootstrap path is forbidden" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "no custom target JSON check passed"
