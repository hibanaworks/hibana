#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="${ROOT_DIR}/proofs/kani/Cargo.toml"
EXPECTED_INVENTORY="${ROOT_DIR}/proofs/kani/harness-inventory.json"
EXPECTED_VERSION="$(< "${ROOT_DIR}/.github/kani-version")"
ACTUAL_VERSION="$(cargo kani --version)"

if [[ "${ACTUAL_VERSION}" != "cargo-kani ${EXPECTED_VERSION}" ]]; then
  echo "Kani gate requires cargo-kani ${EXPECTED_VERSION}, found: ${ACTUAL_VERSION}" >&2
  exit 1
fi

python3 - "${ROOT_DIR}/src" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
marker = re.compile(
    r"#\[kani::should_panic\]\s*(?:#\[[^\n]+\]\s*)*"
    r"(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?fn\s+([A-Za-z0-9_]+)\s*\(\s*\)\s*\{"
)
for path in sorted(root.rglob("*.rs")):
    source = path.read_text()
    for match in marker.finditer(source):
        depth = 1
        cursor = match.end()
        while cursor < len(source) and depth:
            if source[cursor] == "{":
                depth += 1
            elif source[cursor] == "}":
                depth -= 1
            cursor += 1
        if depth:
            print(f"unterminated Kani should-panic harness: {path}:{match.group(1)}", file=sys.stderr)
            sys.exit(1)
        body = source[match.end():cursor - 1]
        direct_panic = re.search(
            r"\b(?:(?:debug_)?assert(?:_eq|_ne)?|panic|unreachable|todo|unimplemented)!\s*\("
            r"|\b(?:crate::)?invariant\s*\(",
            body,
        )
        if direct_panic:
            print(
                f"Kani should-panic harness may panic before its production call: {path}:{match.group(1)}",
                file=sys.stderr,
            )
            sys.exit(1)
PY

inventory_dir="$(mktemp -d "${TMPDIR:-/tmp}/hibana-kani-inventory.XXXXXX")"
verification_log="$(mktemp "${TMPDIR:-/tmp}/hibana-kani-verification.XXXXXX")"
cleanup() {
  rm -rf "${inventory_dir}"
  rm -f "${verification_log}"
}
trap cleanup EXIT

(
  cd "${inventory_dir}"
  RUSTFLAGS="-D warnings" CARGO_BUILD_JOBS=1 cargo kani \
    --manifest-path "${MANIFEST}" \
    --target-dir "${ROOT_DIR}/target/kani" \
    list --format json
)
ACTUAL_INVENTORY="${inventory_dir}/kani-list.json"
if [[ ! -s "${ACTUAL_INVENTORY}" ]]; then
  echo "Kani gate did not produce a nonempty structured harness inventory" >&2
  exit 1
fi
if ! cmp -s "${EXPECTED_INVENTORY}" "${ACTUAL_INVENTORY}"; then
  set +e
  diff -u "${EXPECTED_INVENTORY}" "${ACTUAL_INVENTORY}" >&2
  inventory_diff_status="$?"
  set -e
  if [[ "${inventory_diff_status}" -gt 1 ]]; then
    echo "Kani harness inventory diff failed" >&2
    exit "${inventory_diff_status}"
  fi
  echo "Kani harness inventory changed" >&2
  exit 1
fi
expected_harness_total="$(python3 - "${EXPECTED_INVENTORY}" "${EXPECTED_VERSION}" <<'PY'
import json
import pathlib
import sys

inventory = json.loads(pathlib.Path(sys.argv[1]).read_text())
expected_version = sys.argv[2]
if inventory.get("kani-version") != expected_version:
    raise SystemExit("Kani inventory version does not match .github/kani-version")
totals = inventory.get("totals")
if not isinstance(totals, dict):
    raise SystemExit("Kani inventory is missing totals")
standard = totals.get("standard-harnesses")
contract = totals.get("contract-harnesses")
if not isinstance(standard, int) or standard <= 0 or contract != 0:
    raise SystemExit("Kani inventory must contain nonempty standard harnesses only")
print(standard)
PY
)"

RUSTFLAGS="-D warnings" CARGO_BUILD_JOBS=1 cargo kani \
  --manifest-path "${MANIFEST}" \
  --target-dir "${ROOT_DIR}/target/kani" \
  -Z unstable-options \
  --run-sanity-checks \
  --output-format terse 2>&1 | tee "${verification_log}"

summary_count="$(grep -Ec '^Complete - [0-9]+ successfully verified harnesses, 0 failures, [0-9]+ total\.$' "${verification_log}")"
if [[ "${summary_count}" != "1" ]]; then
  echo "Kani gate requires exactly one successful complete-harness summary" >&2
  exit 1
fi
kani_harness_total="$(sed -nE 's/^Complete - ([0-9]+) successfully verified harnesses, 0 failures, [0-9]+ total\.$/\1/p' "${verification_log}")"
reported_harness_total="$(sed -nE 's/^Complete - [0-9]+ successfully verified harnesses, 0 failures, ([0-9]+) total\.$/\1/p' "${verification_log}")"
if [[ -z "${kani_harness_total}" \
  || "${kani_harness_total}" == "0" \
  || "${kani_harness_total}" != "${expected_harness_total}" \
  || "${kani_harness_total}" != "${reported_harness_total}" ]]; then
  echo \
    "Kani gate requires the complete ${expected_harness_total}-harness inventory" \
    >&2
  exit 1
fi

echo "Kani gate passed version=${EXPECTED_VERSION} harnesses=${kani_harness_total} backend=CBMC"
