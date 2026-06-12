#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

TOOLCHAIN="${TOOLCHAIN:-1.95.0}"

run_package_clean() {
  local label="$1"
  shift

  local log
  log="$(mktemp)"
  trap 'rm -f "${log}"' RETURN

  if ! "$@" >"${log}" 2>&1; then
    cat "${log}" >&2
    echo "package artifact check failed while running: ${label}" >&2
    exit 1
  fi
  if rg -n "warning:" "${log}" >/dev/null; then
    cat "${log}" >&2
    echo "package artifact check detected warnings in: ${label}" >&2
    exit 1
  fi

  cat "${log}"
  rm -f "${log}"
  trap - RETURN
}

PACKAGE_LIST="$(run_package_clean "cargo package --list" \
  env -u RUSTFLAGS cargo +"${TOOLCHAIN}" package --list --allow-dirty)"

for required in Cargo.toml README.md src/lib.rs tests/docs_surface.rs tests/semantic_surface.rs; do
  if ! grep -qx "${required}" <<<"${PACKAGE_LIST}"; then
    echo "package artifact check failed: ${required} must ship in the published crate package" >&2
    exit 1
  fi
done

if rg -n 'tests/support/' src; then
  echo "package artifact check failed: src must not depend on tests/support fixtures" >&2
  exit 1
fi

SOURCE_TEST_FIXTURE_PATTERN='^src/(test_support|endpoint/kernel/test_support)/|^src/.*/tests/|^src/.*/tests\.rs$|^src/.*_tests\.rs$'

if grep -qE "${SOURCE_TEST_FIXTURE_PATTERN}" <<<"${PACKAGE_LIST}"; then
  echo "package artifact check failed: source-tree test fixtures must not ship in the production crate package" >&2
  grep -E "${SOURCE_TEST_FIXTURE_PATTERN}" <<<"${PACKAGE_LIST}" >&2
  exit 1
fi

python3 - <<'PY'
from pathlib import Path
import re
import sys

ROOT = Path(".").resolve()
MOD_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")
PATH_RE = re.compile(r'^\s*#\[path\s*=\s*"([^"]+)"\s*\]')


def is_excluded_fixture(path: Path) -> bool:
    rel = path.as_posix()
    return (
        rel.startswith("src/test_support/")
        or rel.startswith("src/endpoint/kernel/test_support/")
        or "/tests/" in rel
        or rel.endswith("/tests.rs")
        or rel.endswith("_tests.rs")
    )


def default_module_candidates(source: Path, mod_name: str) -> list[Path]:
    if source.name in {"lib.rs", "main.rs", "mod.rs"}:
        base = source.parent
        return [base / f"{mod_name}.rs", base / mod_name / "mod.rs"]
    if source.parent == Path("tests"):
        shared = [source.parent / f"{mod_name}.rs", source.parent / mod_name / "mod.rs"]
        nested = source.with_suffix("")
        return shared + [nested / f"{mod_name}.rs", nested / mod_name / "mod.rs"]
    else:
        base = source.with_suffix("")
    return [base / f"{mod_name}.rs", base / mod_name / "mod.rs"]


violations: list[str] = []
for source in sorted(Path("src").rglob("*.rs")):
    text = source.read_text(encoding="utf-8")
    repo_cfg = False
    path_attr: str | None = None
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("#[cfg(") and "hibana_repo_tests" in stripped:
            repo_cfg = True
            path_attr = None
            continue
        if not repo_cfg:
            continue
        path_match = PATH_RE.match(line)
        if path_match:
            path_attr = path_match.group(1)
            continue
        mod_match = MOD_RE.match(line)
        if mod_match:
            mod_name = mod_match.group(1)
            if path_attr is not None:
                targets = [(source.parent / path_attr).resolve().relative_to(ROOT)]
            else:
                targets = [p for p in default_module_candidates(source, mod_name) if p.exists()]
            if not targets:
                violations.append(f"{source}: repo-only module `{mod_name}` target not found")
            else:
                for target in targets:
                    if not is_excluded_fixture(target):
                        violations.append(
                            f"{source}: repo-only module `{mod_name}` targets package source {target}"
                        )
            repo_cfg = False
            path_attr = None
            continue
        if stripped.startswith("#[") or stripped == "":
            continue
        repo_cfg = False
        path_attr = None

if violations:
    print(
        "package artifact check failed: repo-only test modules must target excluded source-tree fixtures",
        file=sys.stderr,
    )
    for violation in violations:
        print(violation, file=sys.stderr)
    sys.exit(1)
PY

PACKAGE_LIST_TEXT="${PACKAGE_LIST}" python3 - <<'PY'
from pathlib import Path
import os
import re
import sys

PACKAGE_FILES = set(os.environ["PACKAGE_LIST_TEXT"].splitlines())
MOD_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")
PATH_RE = re.compile(r'^\s*#\[path\s*=\s*"([^"]+)"\s*\]')


def default_module_candidates(source: Path, mod_name: str) -> list[Path]:
    if source.name in {"lib.rs", "main.rs", "mod.rs"}:
        base = source.parent
        return [base / f"{mod_name}.rs", base / mod_name / "mod.rs"]
    if source.parent == Path("tests"):
        shared = [source.parent / f"{mod_name}.rs", source.parent / mod_name / "mod.rs"]
        nested = source.with_suffix("")
        return shared + [nested / f"{mod_name}.rs", nested / mod_name / "mod.rs"]
    else:
        base = source.with_suffix("")
    return [base / f"{mod_name}.rs", base / mod_name / "mod.rs"]


seen: set[Path] = set()
missing: list[str] = []
stack = sorted(Path("tests").glob("*.rs"))
while stack:
    source = stack.pop()
    if source in seen:
        continue
    seen.add(source)
    path_attr: str | None = None
    for line in source.read_text(encoding="utf-8").splitlines():
        path_match = PATH_RE.match(line)
        if path_match:
            path_attr = path_match.group(1)
            continue
        mod_match = MOD_RE.match(line)
        if not mod_match:
            if line.strip() and not line.strip().startswith("#["):
                path_attr = None
            continue
        mod_name = mod_match.group(1)
        if path_attr is not None:
            candidates = [source.parent / path_attr]
        else:
            candidates = [p for p in default_module_candidates(source, mod_name) if p.exists()]
        if not candidates:
            missing.append(f"{source}: module `{mod_name}` target not found")
        for target in candidates:
            rel = target.as_posix()
            if rel not in PACKAGE_FILES:
                missing.append(f"{source}: module `{mod_name}` target missing from package: {rel}")
            elif target.suffix == ".rs":
                stack.append(target)
        path_attr = None

if missing:
    print(
        "package artifact check failed: shipped integration tests must include their module tree",
        file=sys.stderr,
    )
    for item in missing:
        print(item, file=sys.stderr)
    sys.exit(1)
PY

run_package_clean "cargo package --no-verify" \
  env -u RUSTFLAGS cargo +"${TOOLCHAIN}" package --allow-dirty --no-verify

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

CRATE_FILE="$(ls -t target/package/hibana-*.crate | head -n 1)"
tar -xf "${CRATE_FILE}" -C "${TMP_DIR}"
PKG_DIR="$(find "${TMP_DIR}" -maxdepth 1 -type d -name 'hibana-*' | head -n 1)"

run_package_clean "package lib check --features std" \
  env -u RUSTFLAGS RUSTFLAGS="-Dwarnings" \
    cargo +"${TOOLCHAIN}" check --manifest-path "${PKG_DIR}/Cargo.toml" --features std --lib
run_package_clean "package lib test build --features std" \
  env -u RUSTFLAGS RUSTFLAGS="-Dwarnings -Adead_code" \
    cargo +"${TOOLCHAIN}" test --manifest-path "${PKG_DIR}/Cargo.toml" --features std --lib --no-run
run_package_clean "package representative test build --features std" \
  env -u RUSTFLAGS RUSTFLAGS="-Dwarnings -Adead_code" \
    cargo +"${TOOLCHAIN}" test --manifest-path "${PKG_DIR}/Cargo.toml" --features std --test semantic_surface --no-run
run_package_clean "package lib check --no-default-features" \
  env -u RUSTFLAGS RUSTFLAGS="-Dwarnings" \
    cargo +"${TOOLCHAIN}" check --manifest-path "${PKG_DIR}/Cargo.toml" --no-default-features --lib
run_package_clean "package lib test build --no-default-features" \
  env -u RUSTFLAGS RUSTFLAGS="-Dwarnings -Adead_code" \
    cargo +"${TOOLCHAIN}" test --manifest-path "${PKG_DIR}/Cargo.toml" --no-default-features --lib --no-run
run_package_clean "package docs --no-default-features" \
  env -u RUSTFLAGS RUSTFLAGS="-Dwarnings" RUSTDOCFLAGS="-Dwarnings" \
    cargo +"${TOOLCHAIN}" doc --manifest-path "${PKG_DIR}/Cargo.toml" --no-deps --no-default-features
