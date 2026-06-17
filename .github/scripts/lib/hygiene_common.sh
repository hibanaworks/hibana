existing_roots() {
  local root
  local missing=0
  for root in "$@"; do
    if [[ -e "${root}" ]]; then
      printf '%s\n' "${root}"
    else
      echo "hygiene root missing: ${root}" >&2
      missing=1
    fi
  done
  return "${missing}"
}

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local path
  local roots
  local root_status
  set +e
  roots="$(existing_roots "$@")"
  root_status=$?
  set -e
  if [[ "${root_status}" -ne 0 ]]; then
    echo "boundary deny root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  while IFS= read -r path; do
    [[ -n "${path}" ]] && paths+=("${path}")
  done <<< "${roots}"
  if [[ "${#paths[@]}" -eq 0 ]]; then
    return
  fi
  set +e
  rg -n "${pattern}" "${paths[@]}"
  local status=$?
  set -e
  if [[ "${status}" -eq 0 ]]; then
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  elif [[ "${status}" -gt 1 ]]; then
    echo "boundary deny search failed: ${label}" >&2
    FAILED=1
  fi
}

check_absent_multiline() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local path
  local roots
  local root_status
  set +e
  roots="$(existing_roots "$@")"
  root_status=$?
  set -e
  if [[ "${root_status}" -ne 0 ]]; then
    echo "boundary deny root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  while IFS= read -r path; do
    [[ -n "${path}" ]] && paths+=("${path}")
  done <<< "${roots}"
  if [[ "${#paths[@]}" -eq 0 ]]; then
    return
  fi
  set +e
  rg -n -U "${pattern}" "${paths[@]}"
  local status=$?
  set -e
  if [[ "${status}" -eq 0 ]]; then
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  elif [[ "${status}" -gt 1 ]]; then
    echo "boundary deny search failed: ${label}" >&2
    FAILED=1
  fi
}
