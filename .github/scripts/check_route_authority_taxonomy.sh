#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

python3 - <<'PY'
import pathlib
import re
import sys

root = pathlib.Path.cwd()
authority = root / "src/endpoint/kernel/route_frontier/authority.rs"
source = authority.read_text()

match = re.search(r"enum\s+RouteDecisionSource\s*\{(?P<body>.*?)\}", source, re.S)
if not match:
    print("route authority taxonomy violation: RouteDecisionSource enum missing", file=sys.stderr)
    sys.exit(1)

variants = [
    line.strip().rstrip(",")
    for line in match.group("body").splitlines()
    if line.strip() and not line.strip().startswith("#")
]
if variants != ["Ack", "Resolver", "Poll"]:
    print(
        "route authority taxonomy violation: RouteDecisionSource must be exactly Ack | Resolver | Poll",
        file=sys.stderr,
    )
    print(f"found: {variants}", file=sys.stderr)
    sys.exit(1)

if "from_ack" not in source or "from_resolver" not in source or "from_poll" not in source:
    print("route authority taxonomy violation: token constructors must remain explicit", file=sys.stderr)
    sys.exit(1)

for path in [root / "src", root / "tests", root / "README.md"]:
    text = "\n".join(p.read_text(errors="ignore") for p in ([path] if path.is_file() else path.rglob("*.rs")))
    if "IncomingClassification" in text:
        print("route authority taxonomy violation: IncomingClassification name must not remain", file=sys.stderr)
        sys.exit(1)

print("route authority taxonomy check passed")
PY
