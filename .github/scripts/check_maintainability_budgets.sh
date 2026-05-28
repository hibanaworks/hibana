#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

FAILED=0

python3 - <<'PY' || FAILED=1
from __future__ import annotations

import os
import re
from pathlib import Path

OWNER_FILE_BUDGET_MAX = int(os.environ.get("OWNER_FILE_BUDGET_MAX", "900"))
OWNER_FILE_BUDGET_MAX_SLACK = int(os.environ.get("OWNER_FILE_BUDGET_MAX_SLACK", "160"))
TEST_LIMIT = int(os.environ.get("TEST_SOURCE_FILE_LINE_LIMIT", "800"))
ALLOW_TEST_SOURCE_DEBT = os.environ.get("ALLOW_TEST_SOURCE_DEBT", "0") == "1"

failed = False
root = Path(".")


def report(message: str) -> None:
    global failed
    print(message, file=os.sys.stderr)
    failed = True


def is_test_like_source(path: str) -> bool:
    parts = Path(path).parts
    name = parts[-1]
    return (
        name == "tests.rs"
        or name.endswith("_tests.rs")
        or "tests" in parts
        or path.startswith("src/test_support/")
        or path.startswith("src/endpoint/kernel/test_support/")
    )


def rs_files_under(*roots: str) -> list[str]:
    files: list[str] = []
    for root_name in roots:
        base = Path(root_name)
        if not base.exists():
            continue
        for path in base.rglob("*.rs"):
            files.append(path.as_posix())
    return sorted(set(files))


all_rs = rs_files_under("src", "tests")
src_rs = [path for path in all_rs if path.startswith("src/")]
prod_rs = [path for path in src_rs if not is_test_like_source(path)]
test_rs = [
    path
    for path in all_rs
    if path.startswith("tests/") or (path.startswith("src/") and is_test_like_source(path))
]
line_counts = {path: len(Path(path).read_text().splitlines()) for path in all_rs}


def owner_files(path: str, mode: str) -> list[str]:
    owner_path = path.rstrip("/")
    if Path(owner_path).is_file():
        return [owner_path] if owner_path in prod_rs else []
    prefix = owner_path + "/"
    if mode == "direct":
        owner = Path(owner_path)
        return [file for file in prod_rs if Path(file).parent == owner]
    return [file for file in prod_rs if file.startswith(prefix)]


def read_manifest(path: str) -> list[tuple[str, int, int, str]]:
    entries: list[tuple[str, int, int, str]] = []
    for raw in Path(path).read_text().splitlines():
        if not raw.strip() or raw.startswith("#"):
            continue
        cols = raw.split("\t")
        if len(cols) < 3:
            report(f"maintainability budget violation: malformed row in {path}: {raw}")
            continue
        mode = cols[3] if len(cols) > 3 and cols[3] else "recursive"
        entries.append((cols[0], int(cols[1]), int(cols[2]), mode))
    return entries


owner_budget_paths: set[str] = set()


def check_owner_budget(path: str, max_lines: int, max_files: int, mode: str) -> None:
    files = owner_files(path, mode)
    lines = sum(line_counts[file] for file in files)
    if lines > max_lines:
        report(
            f"maintainability budget violation: {path} has {lines} production lines (>{max_lines})"
        )
    if len(files) > max_files:
        report(
            f"maintainability budget violation: {path} has {len(files)} production files (>{max_files})"
        )
    if Path(path).is_file() and max_lines > OWNER_FILE_BUDGET_MAX:
        report(
            f"maintainability budget violation: {path} file owner budget {max_lines} exceeds the {OWNER_FILE_BUDGET_MAX}-line hard ceiling; split authority before the owner approaches a 1k-line monolith"
        )
    if Path(path).is_file() and max_lines - lines > OWNER_FILE_BUDGET_MAX_SLACK:
        report(
            f"maintainability budget violation: {path} file owner budget leaves {max_lines - lines} lines of stale slack (>{OWNER_FILE_BUDGET_MAX_SLACK}); keep owner ceilings close enough to expose future sprawl"
        )


for path, max_lines, max_files, mode in read_manifest(".github/maintainability/owner_budget.tsv"):
    owner_budget_paths.add(path)
    if max_lines > 2000:
        report(
            f"maintainability budget violation: {path} budget covers {max_lines} lines; split owner budgets below 2000 lines instead of freezing a broad subsystem"
        )
        continue
    check_owner_budget(path, max_lines, max_files, mode)

if Path(".github/maintainability/owner_budget_semantics.tsv").exists():
    report(
        "maintainability budget violation: owner_budget_semantics.tsv reintroduces path-mirrored leaf owners; use owner_semantics.tsv for semantic owner boundaries"
    )

aggregate_entries = read_manifest(".github/maintainability/owner_aggregate_budget.tsv")
if not aggregate_entries:
    report(
        "maintainability budget violation: aggregate owner manifest must contain narrow semantic owner budgets; an empty manifest hides aggregate sprawl"
    )
for path, max_lines, max_files, mode in aggregate_entries:
    if max_lines > 2000:
        report(
            f"maintainability budget violation: {path} aggregate budget covers {max_lines} lines; split authority into sub-owner budgets below 2000 lines"
        )
        continue
    check_owner_budget(path, max_lines, max_files, mode)

for file in prod_rs:
    lines = line_counts[file]
    if lines >= 500 and file not in owner_budget_paths:
        report(
            f"maintainability budget violation: {file} has {lines} production lines and needs an explicit owner budget"
        )

test_debt_allowlist = ".github/maintainability/test_source_debt_allowlist.txt"
allowlisted: set[str] = set()
for raw in Path(test_debt_allowlist).read_text().splitlines():
    file = raw.strip()
    if not file or file.startswith("#"):
        continue
    if not ALLOW_TEST_SOURCE_DEBT:
        report(
            f"test fixture budget violation: {test_debt_allowlist} must not contain committed debt entries; split oversized test sources instead: {file}"
        )
    allowlisted.add(file)

for file in test_rs:
    lines = line_counts[file]
    if lines > TEST_LIMIT and file not in allowlisted:
        report(
            f"test fixture budget violation: {file} has {lines} lines (>{TEST_LIMIT}); split it below the per-file budget"
        )

for file in allowlisted:
    path = Path(file)
    if not path.is_file():
        report(f"test fixture budget violation: stale test debt allowlist entry: {file}")
        continue
    lines = line_counts.get(file, len(path.read_text().splitlines()))
    if lines <= TEST_LIMIT:
        report(f"test fixture budget violation: remove resolved test debt allowlist entry: {file}")

part_files = [
    path
    for path in all_rs
    if re.fullmatch(r"part[0-9]+\.rs", Path(path).name)
]
if part_files:
    report("test/source decomposition violation: partN.rs shards hide ownership boundaries; use scenario or owner names")
    for path in part_files:
        print(path, file=os.sys.stderr)

for path in [file for file in all_rs if file.startswith("tests/")]:
    for idx, line in enumerate(Path(path).read_text().splitlines(), 1):
        if re.search(r'#\[path = "\.\./src/test_support/', line):
            report("test support boundary violation: integration tests must not path-import src/test_support fixtures")
            print(f"{path}:{idx}:{line}", file=os.sys.stderr)

include_hits: list[str] = []
for path in all_rs:
    for idx, line in enumerate(Path(path).read_text().splitlines(), 1):
        if re.search(r"^\s*include!\s*\(", line):
            include_hits.append(f"{path}:{idx}:{line}")
if include_hits:
    report("module decomposition violation: Rust source must use real module boundaries instead of include! shards")
    for hit in include_hits:
        print(hit, file=os.sys.stderr)

raise SystemExit(1 if failed else 0)
PY

python3 ./.github/scripts/lib/check_owner_partitions.py || FAILED=1
bash ./.github/scripts/check_semantic_surface_shape_hygiene.sh || FAILED=1

if (( FAILED != 0 )); then
  exit 1
fi

echo "maintainability budget check passed"
