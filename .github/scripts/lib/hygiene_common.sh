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

optional_roots() {
  local root
  for root in "$@"; do
    if [[ -e "${root}" ]]; then
      printf '%s\n' "${root}"
    fi
  done
}

capture_rg() {
  local output_var="$1"
  local label="$2"
  shift 2
  local output
  local status
  set +e
  output="$(rg "$@")"
  status=$?
  set -e
  if [[ "${status}" -eq 0 ]]; then
    printf -v "${output_var}" '%s' "${output}"
  elif [[ "${status}" -eq 1 ]]; then
    printf -v "${output_var}" '%s' ""
  else
    printf -v "${output_var}" '%s' ""
    echo "boundary deny search failed: ${label}" >&2
    FAILED=1
  fi
}

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  local required=()
  local optional=()
  local paths=()
  local path
  local roots
  local root_status
  local optional_root_list
  local mode="required"
  while [[ "$#" -gt 0 ]]; do
    if [[ "$1" == "--optional" ]]; then
      mode="optional"
      shift
      continue
    fi
    if [[ "${mode}" == "optional" ]]; then
      optional+=("$1")
    else
      required+=("$1")
    fi
    shift
  done
  set +e
  roots="$(existing_roots "${required[@]}")"
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
  optional_root_list="$(optional_roots "${optional[@]}")"
  while IFS= read -r path; do
    [[ -n "${path}" ]] && paths+=("${path}")
  done <<< "${optional_root_list}"
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
  local required=()
  local optional=()
  local paths=()
  local path
  local roots
  local root_status
  local optional_root_list
  local mode="required"
  while [[ "$#" -gt 0 ]]; do
    if [[ "$1" == "--optional" ]]; then
      mode="optional"
      shift
      continue
    fi
    if [[ "${mode}" == "optional" ]]; then
      optional+=("$1")
    else
      required+=("$1")
    fi
    shift
  done
  set +e
  roots="$(existing_roots "${required[@]}")"
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
  optional_root_list="$(optional_roots "${optional[@]}")"
  while IFS= read -r path; do
    [[ -n "${path}" ]] && paths+=("${path}")
  done <<< "${optional_root_list}"
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
