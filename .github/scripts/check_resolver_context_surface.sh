#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

if rg -n "^[[:space:]]*pub(\\([^)]*\\))?[[:space:]]+(const[[:space:]]+)?fn[[:space:]]+(eff_index|scope_id|scope_trace)[[:space:]]*\\(" src/control/cluster/core.rs; then
  echo "boundary violation: ResolverContext must not expose internal coordinate getters" >&2
  FAILED=1
fi

python3 - <<'PY'
import pathlib
import re
import sys

source = pathlib.Path("src/control/cluster/core.rs").read_text()
anchor = source.find("impl ResolverContext {")
if anchor < 0:
    print("boundary violation: missing ResolverContext impl", file=sys.stderr)
    sys.exit(1)
open_brace = source.find("{", anchor)
depth = 0
end = None
for idx, ch in enumerate(source[open_brace:], start=open_brace):
    if ch == "{":
        depth += 1
    elif ch == "}":
        depth -= 1
        if depth == 0:
            end = idx
            break
if end is None:
    print("boundary violation: unterminated ResolverContext impl", file=sys.stderr)
    sys.exit(1)

body = source[open_brace + 1:end]
for name in [
    "eff_index",
    "scope_id",
    "scope_trace",
    "rv_id",
    "session",
    "lane",
    "tag",
    "metrics",
]:
    if re.search(rf"\b(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?fn\s+{name}\s*\(", body):
        print(
            f"boundary violation: ResolverContext must not expose {name} getter",
            file=sys.stderr,
        )
        sys.exit(1)
PY

if rg -n "^[[:space:]]*pub(\\([^)]*\\))?[[:space:]]+(const[[:space:]]+)?fn[[:space:]]+(scope_id|scope_kind|scope_region)[[:space:]]*\\(" src/endpoint/kernel/core.rs; then
  echo "boundary violation: RouteBranch must not expose scope coordinate helpers" >&2
  FAILED=1
fi

if [[ "${FAILED}" -ne 0 ]]; then
  exit 1
fi

echo "resolver/route surface boundary check passed"
