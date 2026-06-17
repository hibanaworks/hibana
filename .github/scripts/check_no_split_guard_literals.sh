#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

python3 - <<'PY'
from pathlib import Path
import re
import sys

ROOT = Path.cwd()
SELF = ROOT / ".github/scripts/check_no_split_guard_literals.sh"
SCRIPT_ROOT = ROOT / ".github/scripts"


def quote_spans(line: str, quote: str) -> list[tuple[int, int]]:
    spans = []
    index = 0
    while index < len(line):
        start = line.find(quote, index)
        if start == -1:
            break
        index = start + 1
        escaped = False
        while index < len(line):
            ch = line[index]
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == quote:
                spans.append((start, index + 1))
                index += 1
                break
            index += 1
    return spans


def shell_has_adjacent_literals(line: str) -> bool:
    for quote in ("'", '"'):
        spans = quote_spans(line, quote)
        if any(
            line[left_end:right_start] == ""
            for (_, left_end), (right_start, _) in zip(spans, spans[1:])
        ):
            return True
    return False


PYTHON_ADJACENT_LITERAL = re.compile(r'r?"[^"]*"\s+r?"[^"]*"')


def python_regex_has_adjacent_literals(line: str) -> bool:
    if not any(call in line for call in ("re.search", "re.match", "re.fullmatch", "re.compile")):
        return False
    return bool(PYTHON_ADJACENT_LITERAL.search(line))


offenders: list[str] = []

for path in (ROOT / "tests").rglob("*.rs"):
    relative = path.relative_to(ROOT).as_posix()
    for line_no, line in enumerate(path.read_text().splitlines(), 1):
        if "concat!(" in line:
            offenders.append(f"{relative}:{line_no}: Rust guard source must not hide deny tokens with concat!()")
        if ".concat()" in line:
            offenders.append(f"{relative}:{line_no}: Rust guard source must not hide deny tokens with concat()")

for path in SCRIPT_ROOT.rglob("*"):
    if path == SELF or not path.is_file():
        continue
    relative = path.relative_to(ROOT).as_posix()
    for line_no, line in enumerate(path.read_text(errors="replace").splitlines(), 1):
        if path.suffix != ".py" and shell_has_adjacent_literals(line):
            offenders.append(f"{relative}:{line_no}: shell guard source must not hide deny tokens with adjacent quoted literals")
        if python_regex_has_adjacent_literals(line):
            offenders.append(f"{relative}:{line_no}: Python regex guard must not hide deny tokens with adjacent literals")

if offenders:
    for offender in offenders:
        print(offender, file=sys.stderr)
    raise SystemExit(1)
PY
