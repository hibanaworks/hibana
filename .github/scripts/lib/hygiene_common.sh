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

append_existing_roots() {
  local output_var="$1"
  shift
  local roots
  local root_status
  local path
  set +e
  roots="$(existing_roots "$@")"
  root_status=$?
  set -e
  if [[ "${root_status}" -ne 0 ]]; then
    return 1
  fi
  while IFS= read -r path; do
    [[ -n "${path}" ]] && eval "${output_var}+=(\"\${path}\")"
  done <<< "${roots}"
  return 0
}

append_optional_roots() {
  local output_var="$1"
  shift
  local roots
  local path
  roots="$(optional_roots "$@")"
  while IFS= read -r path; do
    [[ -n "${path}" ]] && eval "${output_var}+=(\"\${path}\")"
  done <<< "${roots}"
  return 0
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

capture_pipe_rg() {
  local output_var="$1"
  local label="$2"
  local input="$3"
  shift 3
  local output
  local status
  set +e
  output="$(printf '%s\n' "${input}" | rg "$@")"
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

collect_hygiene_search() {
  local paths_var="$1"
  local rg_args_var="$2"
  shift 2
  local required=()
  local optional=()
  local mode="required"
  while [[ "$#" -gt 0 ]]; do
    case "$1" in
      --optional)
        mode="optional"
        shift
        ;;
      --required)
        mode="required"
        shift
        ;;
      --glob)
        eval "${rg_args_var}+=(\"-g\" \"\$2\")"
        shift 2
        ;;
      *)
        if [[ "${mode}" == "optional" ]]; then
          optional+=("$1")
        else
          required+=("$1")
        fi
        shift
        ;;
    esac
  done
  if ! append_existing_roots "${paths_var}" "${required[@]}"; then
    return 1
  fi
  append_optional_roots "${paths_var}" "${optional[@]}"
  return 0
}

check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local rg_args=("-n" "${pattern}")
  if ! collect_hygiene_search paths rg_args "$@"; then
    echo "boundary deny root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  if [[ "${#paths[@]}" -eq 0 ]]; then
    return
  fi
  local matches
  capture_rg matches "${label}" "${rg_args[@]}" "${paths[@]}"
  if [[ -n "${matches}" ]]; then
    echo "${matches}"
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  fi
}

check_absent_multiline() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local rg_args=("-n" "-U" "${pattern}")
  if ! collect_hygiene_search paths rg_args "$@"; then
    echo "boundary deny root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  if [[ "${#paths[@]}" -eq 0 ]]; then
    return
  fi
  local matches
  capture_rg matches "${label}" "${rg_args[@]}" "${paths[@]}"
  if [[ -n "${matches}" ]]; then
    echo "${matches}"
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  fi
}

check_absent_literal() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local rg_args=("-n" "-F" "${pattern}")
  if ! collect_hygiene_search paths rg_args "$@"; then
    echo "boundary deny root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  if [[ "${#paths[@]}" -eq 0 ]]; then
    return
  fi
  local matches
  capture_rg matches "${label}" "${rg_args[@]}" "${paths[@]}"
  if [[ -n "${matches}" ]]; then
    echo "${matches}"
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  fi
}

check_required() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local rg_args=("-n" "-F" "${pattern}")
  if ! collect_hygiene_search paths rg_args "$@"; then
    echo "boundary required root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  if [[ "${#paths[@]}" -eq 0 ]]; then
    echo "boundary required root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  local matches
  capture_rg matches "${label}" "${rg_args[@]}" "${paths[@]}"
  if [[ -z "${matches}" ]]; then
    echo "boundary required pattern missing: ${label}" >&2
    FAILED=1
  fi
}

check_required_regex() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local rg_args=("-n" "${pattern}")
  if ! collect_hygiene_search paths rg_args "$@"; then
    echo "boundary required root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  if [[ "${#paths[@]}" -eq 0 ]]; then
    echo "boundary required root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  local matches
  capture_rg matches "${label}" "${rg_args[@]}" "${paths[@]}"
  if [[ -z "${matches}" ]]; then
    echo "boundary required pattern missing: ${label}" >&2
    FAILED=1
  fi
}

check_required_multiline() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local rg_args=("-n" "-U" "${pattern}")
  if ! collect_hygiene_search paths rg_args "$@"; then
    echo "boundary required root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  if [[ "${#paths[@]}" -eq 0 ]]; then
    echo "boundary required root mismatch: ${label}" >&2
    FAILED=1
    return
  fi
  local matches
  capture_rg matches "${label}" "${rg_args[@]}" "${paths[@]}"
  if [[ -z "${matches}" ]]; then
    echo "boundary required pattern missing: ${label}" >&2
    FAILED=1
  fi
}

check_absent_outside_tests() {
  local pattern="$1"
  local label="$2"
  shift 2
  local args=(
    src
    --glob '!**/tests.rs'
    --glob '!**/*_tests.rs'
    --glob '!**/test_support/**'
  )
  local exclude
  for exclude in "$@"; do
    args+=(--glob "!${exclude}")
  done
  check_absent_multiline "${pattern}" "${label}" "${args[@]}"
}

check_pipe_absent() {
  local pattern="$1"
  local label="$2"
  local input="$3"
  shift 3
  local matches
  capture_pipe_rg matches "${label}" "${input}" -n "$@" "${pattern}"
  if [[ -n "${matches}" ]]; then
    echo "${matches}"
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  fi
}

check_pipe_required() {
  local pattern="$1"
  local label="$2"
  local input="$3"
  shift 3
  local matches
  capture_pipe_rg matches "${label}" "${input}" -n -F "$@" "${pattern}"
  if [[ -z "${matches}" ]]; then
    echo "boundary required pattern missing: ${label}" >&2
    FAILED=1
  fi
}

check_forbidden_paths() {
  local label="$1"
  shift
  local path
  for path in "$@"; do
    if [[ -e "${path}" ]]; then
      echo "${path}" >&2
      echo "boundary forbidden path present: ${label}" >&2
      FAILED=1
    fi
  done
}
