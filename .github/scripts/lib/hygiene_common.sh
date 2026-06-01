check_absent() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local path
  for path in "$@"; do
    if [[ -e "${path}" ]]; then
      paths+=("${path}")
    fi
  done
  if [[ "${#paths[@]}" -eq 0 ]]; then
    return
  fi
  if rg -n "${pattern}" "${paths[@]}"; then
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  fi
}

check_absent_multiline() {
  local pattern="$1"
  local label="$2"
  shift 2
  local paths=()
  local path
  for path in "$@"; do
    if [[ -e "${path}" ]]; then
      paths+=("${path}")
    fi
  done
  if [[ "${#paths[@]}" -eq 0 ]]; then
    return
  fi
  if rg -n -U "${pattern}" "${paths[@]}"; then
    echo "boundary deny pattern detected: ${label}" >&2
    FAILED=1
  fi
}
