# Reject untrusted proof declarations and failures of the audit tool itself.
check_absent() {
  local pattern="$1" label="$2"
  shift 2
  local output status
  if output="$(rg -n "${pattern}" "$@")"; then
    status=0
  else
    status=$?
  fi
  case "${status}" in
    0)
      printf '%s\nUntrusted proof declaration: %s\n' "${output}" "${label}" >&2
      FAILED=1
      ;;
    1) ;;
    *)
      printf 'Proof audit failed: %s (status %s)\n' "${label}" "${status}" >&2
      FAILED=1
      ;;
  esac
}
