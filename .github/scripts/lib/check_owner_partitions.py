#!/usr/bin/env python3
from __future__ import annotations

from collections import defaultdict
from pathlib import Path
import sys

LIMIT = 2000
PARTITION_MANIFEST = Path(".github/maintainability/owner_partitions.tsv")
SEMANTIC_MANIFEST = Path(".github/maintainability/owner_semantics.tsv")


def fail(message: str) -> None:
    print(f"maintainability budget violation: {message}", file=sys.stderr)
    raise SystemExit(1)


def is_test_like_source(path: Path) -> bool:
    text = str(path)
    return (
        text.endswith("/tests.rs")
        or text.endswith("_tests.rs")
        or "/tests/" in text
        or text.startswith("src/endpoint/kernel/test_support/")
    )


def is_test_owner(path: Path) -> bool:
    text = str(path)
    return text == "tests" or text.startswith("tests/") or text.startswith(
        "src/endpoint/kernel/test_support"
    )


def owner_files(path: Path) -> set[Path]:
    if not path.exists():
        return set()
    if path.is_file():
        candidates = [path] if path.suffix == ".rs" else []
    else:
        candidates = [p for p in path.rglob("*.rs") if p.is_file()]
    if is_test_owner(path):
        return set(candidates)
    return {p for p in candidates if not is_test_like_source(p)}


def owner_lines(path: Path) -> int:
    return sum(p.read_text(encoding="utf-8").count("\n") + 1 for p in owner_files(path))


def load_partitions() -> dict[Path, list[Path]]:
    if not PARTITION_MANIFEST.exists():
        fail(f"missing owner partition manifest: {PARTITION_MANIFEST}")

    partitions: dict[Path, list[Path]] = defaultdict(list)
    for line_no, raw in enumerate(PARTITION_MANIFEST.read_text().splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        try:
            parent_raw, child_raw = raw.split("\t")
        except ValueError:
            fail(f"{PARTITION_MANIFEST}:{line_no}: expected parent<TAB>child")
        parent = Path(parent_raw)
        child = Path(child_raw)
        if not parent.exists() or not parent.is_dir():
            fail(f"forbidden owner partition parent: {parent}")
        if not child.exists():
            fail(f"forbidden owner partition child: {child}")
        try:
            child.relative_to(parent)
        except ValueError:
            fail(f"owner partition child is outside parent: {parent} -> {child}")
        if child == parent:
            fail(f"owner partition child cannot equal parent: {parent}")
        if not owner_files(child):
            fail(f"owner partition child owns no Rust source: {parent} -> {child}")
        partitions[parent].append(child)
    return partitions


def load_semantic_owners() -> dict[Path, tuple[str, str]]:
    if not SEMANTIC_MANIFEST.exists():
        fail(f"missing semantic owner manifest: {SEMANTIC_MANIFEST}")

    semantic_owners: dict[Path, tuple[str, str]] = {}
    for line_no, raw in enumerate(SEMANTIC_MANIFEST.read_text().splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = raw.split("\t")
        if len(parts) != 3:
            fail(f"{SEMANTIC_MANIFEST}:{line_no}: expected path<TAB>owner<TAB>responsibility")
        owner_path = Path(parts[0])
        owner_name = parts[1].strip()
        responsibility = parts[2].strip()
        path_slug = "-".join(owner_path.parts)
        if not owner_path.exists() or not owner_path.is_dir():
            fail(f"forbidden semantic owner path: {owner_path}")
        if (
            owner_name in {"src", "tests", owner_path.name}
            or owner_name == path_slug
            or owner_name == f"{path_slug}-authority"
            or "-" not in owner_name
        ):
            fail(
                "semantic owner name must describe authority, not mirror the "
                f"directory: {owner_path} -> {owner_name}"
            )
        if len(responsibility.split()) < 4:
            fail(f"semantic owner responsibility is too vague: {owner_path}")
        if owner_path in semantic_owners:
            fail(f"duplicate semantic owner entry: {owner_path}")
        semantic_owners[owner_path] = (owner_name, responsibility)
    return semantic_owners


def checked_dirs() -> list[Path]:
    dirs: list[Path] = []
    for root in [Path("src"), Path("tests")]:
        if not root.exists():
            continue
        dirs.append(root)
        dirs.extend(p for p in root.rglob("*") if p.is_dir())
    return dirs


def main() -> int:
    partitions = load_partitions()
    semantic_owners = load_semantic_owners()
    failed = False

    missing_semantics = sorted(set(partitions).difference(semantic_owners))
    extra_semantics = sorted(set(semantic_owners).difference(partitions))
    if missing_semantics:
        print(
            "maintainability budget violation: every partition parent must name its semantic owner",
            file=sys.stderr,
        )
        for parent in missing_semantics:
            print(f"  missing semantic owner: {parent}", file=sys.stderr)
        failed = True
    if extra_semantics:
        print(
            "maintainability budget violation: semantic owner entries must correspond to partition parents",
            file=sys.stderr,
        )
        for parent in extra_semantics:
            print(f"  forbidden semantic owner: {parent}", file=sys.stderr)
        failed = True

    dirs_to_check = checked_dirs()
    for parent in sorted(dirs_to_check):
        files = owner_files(parent)
        if not files:
            continue
        lines = owner_lines(parent)
        if lines <= LIMIT:
            if parent in partitions:
                print(
                    f"maintainability budget violation: forbidden owner partition for {parent}; owner has {lines} lines (<= {LIMIT})",
                    file=sys.stderr,
                )
                failed = True
            continue
        if parent not in partitions:
            print(
                f"maintainability budget violation: {parent} has {lines} lines and needs explicit child owner partitions",
                file=sys.stderr,
            )
            failed = True
            continue

        covered: dict[Path, Path] = {}
        for child in partitions[parent]:
            for source in owner_files(child):
                existing = covered.setdefault(source, child)
                if existing != child:
                    print(
                        f"maintainability budget violation: {source} is covered twice in {parent}: {existing} and {child}",
                        file=sys.stderr,
                    )
                    failed = True

        missing = sorted(files.difference(covered))
        extra = sorted(set(covered).difference(files))
        if missing:
            print(
                f"maintainability budget violation: {parent} partition misses {len(missing)} source files",
                file=sys.stderr,
            )
            for source in missing[:20]:
                print(f"  missing: {source}", file=sys.stderr)
            failed = True
        if extra:
            print(
                f"maintainability budget violation: {parent} partition covers files outside owner domain",
                file=sys.stderr,
            )
            for source in extra[:20]:
                print(f"  extra: {source}", file=sys.stderr)
            failed = True

    for parent in sorted(partitions):
        if parent not in dirs_to_check:
            print(
                f"maintainability budget violation: owner partition parent is outside checked source/test roots: {parent}",
                file=sys.stderr,
            )
            failed = True

    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
