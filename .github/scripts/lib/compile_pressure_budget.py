#!/usr/bin/env python3
import csv
import sys
from pathlib import Path


FIELDS = (
    "label",
    "observed_seconds",
    "observed_rss_mib",
    "seconds_headroom",
    "rss_headroom_mib",
)


def rows(path: Path) -> list[dict[str, str]]:
    if not path.is_file():
        raise SystemExit(f"compile pressure budget violation: missing budget file: {path}")
    parsed = list(
        csv.DictReader(
            (line for line in path.read_text(encoding="utf-8").splitlines() if not line.startswith("#")),
            delimiter="\t",
        )
    )
    if not parsed:
        raise SystemExit("compile pressure budget violation: empty compile pressure budget")
    for row in parsed:
        if tuple(row.keys()) != FIELDS:
            raise SystemExit(f"compile pressure budget violation: invalid columns: {row}")
        label = row["label"]
        for field in FIELDS[1:]:
            value = row[field]
            if not value.isdigit() or int(value) <= 0:
                raise SystemExit(
                    f"compile pressure budget violation: invalid {field} for {label}: {value!r}"
                )
    return parsed


def limit_for(row: dict[str, str], kind: str) -> int:
    if kind == "seconds":
        return int(row["observed_seconds"]) + int(row["seconds_headroom"])
    if kind == "rss_mib":
        return int(row["observed_rss_mib"]) + int(row["rss_headroom_mib"])
    raise SystemExit(f"compile pressure budget violation: unknown limit kind: {kind}")


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        raise SystemExit(
            "usage: compile_pressure_budget.py limit PATH LABEL seconds|rss_mib | max-rss PATH"
        )
    command = argv[1]
    path = Path(argv[2])
    parsed = rows(path)
    if command == "limit":
        if len(argv) != 5:
            raise SystemExit("usage: compile_pressure_budget.py limit PATH LABEL seconds|rss_mib")
        label = argv[3]
        kind = argv[4]
        matches = [row for row in parsed if row["label"] == label]
        if len(matches) != 1:
            raise SystemExit(
                f"compile pressure budget violation: expected one row for {label}, found {len(matches)}"
            )
        print(limit_for(matches[0], kind))
        return 0
    if command == "max-rss":
        if len(argv) != 3:
            raise SystemExit("usage: compile_pressure_budget.py max-rss PATH")
        print(max(limit_for(row, "rss_mib") for row in parsed))
        return 0
    raise SystemExit(f"compile pressure budget violation: unknown command: {command}")


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
