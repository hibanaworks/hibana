# The Rust audit owns token policy and treats unreadable sources as failures.
check_trusted_proof_sources() {
  local helper
  helper="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lean_audit_tool.sh"
  if ! bash "${helper}" proof-guard sources "$@"; then
    FAILED=1
  fi
}
