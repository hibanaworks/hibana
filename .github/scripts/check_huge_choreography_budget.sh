#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
export TOOLCHAIN="${TOOLCHAIN:-1.95.0}"
bash "${ROOT}/.github/scripts/ensure_rust_toolchain.sh"

python3 - <<'PY'
import os
import pathlib
import subprocess

root = pathlib.Path.cwd()
toolchain = os.environ["TOOLCHAIN"]
timeout_seconds = 180
commands = [
    [
        "cargo",
        f"+{toolchain}",
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
        f"+{toolchain}",
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
        f"+{toolchain}",
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
