#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="${ROOT_DIR}/proofs/kani/Cargo.toml"
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

verification_log="$(mktemp "${TMPDIR:-/tmp}/hibana-kani-verification.XXXXXX")"
trap 'rm -f "${verification_log}"' EXIT

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
  || "${kani_harness_total}" != "${reported_harness_total}" ]]; then
  echo "Kani gate requires a nonempty complete-harness total" >&2
  exit 1
fi

echo "Kani gate passed version=${EXPECTED_VERSION} harnesses=${kani_harness_total} backend=CBMC"
