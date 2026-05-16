#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
source "${ROOT_DIR}/.github/scripts/configure_ui_diagnostics.sh"
bash "${ROOT_DIR}/.github/scripts/ensure_rust_toolchain.sh"

log="$(mktemp)"
trap 'rm -f "${log}"' EXIT

cargo +"${TOOLCHAIN}" test \
  -p hibana \
  --lib \
  --features std \
  --no-run \
  --message-format=json >"${log}"

test_exe="$(
  python3 - "${log}" <<'PY'
import json
import sys

executable = None
with open(sys.argv[1], encoding="utf-8") as stream:
    for line in stream:
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("reason") != "compiler-artifact":
            continue
        target = event.get("target", {})
        if target.get("name") != "hibana":
            continue
        if not target.get("test"):
            continue
        candidate = event.get("executable")
        if candidate:
            executable = candidate

if not executable:
    raise SystemExit("hibana lib test executable not found")

print(executable)
PY
)"

# Keep the runtime stack probe separate from compilation.  RUST_MIN_STACK also
# affects rustc worker threads, and the huge-choreography compile tests are a
# compiler stress case rather than the runtime stack budget we want to probe.
RUST_MIN_STACK="${HIBANA_CI_TEST_STACK_BYTES:-2097152}" \
  "${test_exe}" control::cluster::core::tests:: --test-threads=1

echo "CI-sized cluster test stack check passed"
