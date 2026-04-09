#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

python3 - <<'PY'
import pathlib
import subprocess

root = pathlib.Path.cwd()
timeout_seconds = 180
commands = [
    [
        "cargo",
        "test",
        "-p",
        "hibana",
        "--test",
        "huge_choreography_compile",
        "--no-run",
        "--features",
        "std",
    ],
    [
        "cargo",
        "test",
        "-p",
        "hibana",
        "--test",
        "huge_choreography_runtime",
        "--no-run",
        "--features",
        "std",
    ],
    [
        "cargo",
        "test",
        "-p",
        "hibana",
        "--test",
        "huge_choreography_resident",
        "--no-run",
        "--features",
        "std",
    ],
]

for command in commands:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=root, check=True, timeout=timeout_seconds)
PY
