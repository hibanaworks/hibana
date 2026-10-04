#!/usr/bin/env bash
# Structural checks only. Source ownership/partition bookkeeping is not a gate.
set -euo pipefail
cd "$(dirname "$0")/../.."
python3 - <<'PYCODE'
from pathlib import Path
import re
import sys

failed = False
for root in ("src", "tests"):
    for path in sorted(Path(root).rglob("*.rs")):
        if re.fullmatch(r"part[0-9]+\.rs", path.name):
            print(f"source structure: use a descriptive module name: {path}", file=sys.stderr)
            failed = True
        for number, line in enumerate(path.read_text().splitlines(), 1):
            if root == "tests" and re.search(r'#\[path = "\.\./src/test_support/', line):
                print(f"repository tests must not path-import src/test_support: {path}:{number}", file=sys.stderr)
                failed = True
            if re.search(r"^\s*include!\s*\(", line):
                print(f"source structure: use real module boundaries instead of include! shards: {path}:{number}", file=sys.stderr)
                failed = True
raise SystemExit(1 if failed else 0)
PYCODE
echo "source structure check passed"
