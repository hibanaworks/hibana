#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
case "${1:-}" in
  theorem-inventory) tool=check_lean_theorem_inventory ;;
  generated-axioms) tool=check_generated_lean_axioms ;;
  proof-guard) tool=check_lean_proof_guard ;;
  *) echo 'usage: lean_audit_tool.sh theorem-inventory|generated-axioms|proof-guard ARGS...' >&2; exit 2 ;;
esac
shift
SOURCE="${ROOT_DIR}/.github/ci-tests/${tool}.rs"
SHARED="${ROOT_DIR}/.github/ci-tests/lean_audit.rs"
BUILD_DIR="${ROOT_DIR}/target/ci-tools"
BINARY="${BUILD_DIR}/${tool}"
if [[ ! -x "${BINARY}" || "${SOURCE}" -nt "${BINARY}" || "${SHARED}" -nt "${BINARY}" ]]; then
  mkdir -p "${BUILD_DIR}"
  temporary="$(mktemp "${BUILD_DIR}/${tool}.XXXXXX")"
  trap 'rm -f "${temporary}"' EXIT
  rustc +"${TOOLCHAIN:-1.95.0}" --edition=2024 -O "${SOURCE}" -o "${temporary}"
  mv -f "${temporary}" "${BINARY}"
fi
exec "${BINARY}" "$@"
