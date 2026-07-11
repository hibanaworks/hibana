#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
TEST_TIMEOUT_SECONDS="${HIBANA_MANIFEST_TEST_TIMEOUT_SECONDS:-180}"

source "${ROOT_DIR}/.github/scripts/configure_ui_diagnostics.sh"
source "${ROOT_DIR}/.github/scripts/repo_rustflags.sh"
hibana_enable_repo_tests_cfg
hibana_pin_ui_diagnostic_width
trap hibana_restore_ui_diagnostic_width EXIT

if ! command -v timeout >/dev/null 2>&1; then
  echo "manifest test gate requires GNU timeout" >&2
  exit 1
fi

manifest_test_rows="$(python3 - <<'PY'
import tomllib
from pathlib import Path

root_manifest = Path("Cargo.toml")
root_data = tomllib.loads(root_manifest.read_text())
members = root_data.get("workspace", {}).get("members")
if not isinstance(members, list) or not members:
    raise SystemExit("root Cargo.toml must declare nonempty workspace members")

manifests = []
for member in members:
    if not isinstance(member, str):
        raise SystemExit("workspace member must be a string")
    manifest = root_manifest if member == "." else Path(member) / "Cargo.toml"
    if manifest in manifests:
        raise SystemExit(f"duplicate workspace manifest: {manifest}")
    if not manifest.is_file():
        raise SystemExit(f"missing workspace manifest: {manifest}")
    manifests.append(manifest)

for manifest in manifests:
    data = tomllib.loads(manifest.read_text())
    seen = set()
    for target in data.get("test", []):
        name = target.get("name")
        path = target.get("path")
        if not isinstance(name, str) or not isinstance(path, str):
            raise SystemExit(f"invalid [[test]] entry in {manifest}")
        if name in seen:
            raise SystemExit(f"duplicate [[test]] name in {manifest}: {name}")
        seen.add(name)
        target_path = (manifest.parent / path).resolve()
        if not target_path.is_file():
            raise SystemExit(f"missing [[test]] path in {manifest}: {path}")
        print(f"{manifest}\t{name}")
PY
)"

if [[ -z "${manifest_test_rows}" ]]; then
  echo "manifest test gate discovered no targets" >&2
  exit 1
fi
mapfile -t manifest_tests <<<"${manifest_test_rows}"

for row in "${manifest_tests[@]}"; do
  IFS=$'\t' read -r manifest target <<<"${row}"
  output="$(mktemp "${TMPDIR:-/tmp}/hibana-manifest-test.XXXXXX")"
  if ! timeout "${TEST_TIMEOUT_SECONDS}s" \
    cargo +"${TOOLCHAIN}" test --manifest-path "${manifest}" --test "${target}" \
      >"${output}" 2>&1; then
    cat "${output}" >&2
    rm -f "${output}"
    echo "manifest test gate failed: ${manifest} target=${target}" >&2
    exit 1
  fi
  if ! python3 - "${output}" >/dev/null <<'PY'
import re
import sys
from pathlib import Path

lines = Path(sys.argv[1]).read_text().splitlines()
running = None
result = None
for line in lines:
    if running is None:
        running_match = re.fullmatch(r"running ([0-9]+) tests?", line)
        if running_match:
            running = int(running_match.group(1))
            continue
    result_match = re.match(
        r"test result: ok\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored;",
        line,
    )
    if result_match:
        result = tuple(map(int, result_match.groups()))

if running is None or result is None:
    raise SystemExit("missing outer test harness count")
passed, failed, ignored = result
if running == 0 or passed != running or failed != 0 or ignored != 0:
    raise SystemExit(
        f"outer test harness mismatch: running={running} passed={passed} "
        f"failed={failed} ignored={ignored}"
    )
PY
  then
    cat "${output}" >&2
    rm -f "${output}"
    echo "manifest test gate count mismatch: ${manifest} target=${target}" >&2
    exit 1
  fi
  cat "${output}"
  rm -f "${output}"
done

echo "manifest test gate passed targets=${#manifest_tests[@]}"
