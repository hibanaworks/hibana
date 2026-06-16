#!/usr/bin/env python3
"""Stable source scanner for the curated public Hibana API allowlists."""

from __future__ import annotations

import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


@dataclass(frozen=True)
class Surface:
    allowlist: str
    sources: tuple[str, ...]
    ignore_prefixes: tuple[str, ...] = ()


SURFACES: dict[str, Surface] = {
    "lib": Surface(
        ".github/allowlists/lib-public-api.txt",
        ("src/lib.rs",),
    ),
    "g": Surface(
        ".github/allowlists/g-public-api.txt",
        ("src/g.rs",),
    ),
    "endpoint": Surface(
        ".github/allowlists/endpoint-public-api.txt",
        (
            "src/endpoint/public_types.rs",
            "src/endpoint/ops.rs",
            "src/endpoint/branch.rs",
            "src/endpoint/error.rs",
        ),
    ),
    "runtime": Surface(
        ".github/allowlists/runtime-public-api.txt",
        (
            "src/runtime.rs",
            "src/runtime/buckets.rs",
            "src/runtime/session_kit.rs",
            "src/runtime/fluent.rs",
            "src/runtime_core/config.rs",
            "src/session/cluster/core/dynamic_resolvers.rs",
            "src/session/cluster/error.rs",
            "src/observe/core.rs",
            "src/observe/ids.rs",
            "src/transport.rs",
            "src/transport/labels.rs",
            "src/transport/wire.rs",
        ),
        (
            "pub use buckets::*;",
            "pub use session_kit::",
            "pub use crate::observe::event::",
            "pub use labels::FrameLabel;",
            "pub(crate) enum",
        ),
    ),
}


PUBLIC_PREFIXES = (
    "pub use ",
    "pub mod ",
    "pub struct ",
    "pub enum ",
    "pub type ",
    "pub trait ",
    "pub const fn ",
    "pub fn ",
    "pub const ",
)


def compact(statement: str) -> str:
    return " ".join(statement.split())


def is_public_start(line: str) -> bool:
    return line.startswith(PUBLIC_PREFIXES) and not (
        line.startswith("pub(crate)")
        or line.startswith("pub(super)")
        or line.startswith("pub(in ")
    )


def should_ignore(line: str, surface: Surface) -> bool:
    return any(line.startswith(prefix) for prefix in surface.ignore_prefixes)


def collect_statement(lines: list[str], start: int) -> tuple[str, int]:
    pieces: list[str] = []
    index = start
    is_use = lines[start].strip().startswith("pub use ")
    while index < len(lines):
        stripped = lines[index].strip()
        pieces.append(stripped)
        if is_use:
            if stripped.endswith(";"):
                break
        elif "{" in stripped or stripped.endswith(";"):
            break
        index += 1
    return compact(" ".join(pieces)), index + 1


def public_type_names(lines: list[str]) -> set[str]:
    names: set[str] = set()
    for line in lines:
        stripped = line.strip()
        for prefix in ("pub struct ", "pub enum ", "pub trait ", "pub type "):
            if stripped.startswith(prefix):
                rest = stripped[len(prefix) :]
                name = rest.split("=", 1)[0].split("(", 1)[0].split("{", 1)[0].split()[0]
                names.add(name.split("<", 1)[0])
                break
    return names


def strip_angle_prefix(text: str) -> str:
    if not text.startswith("<"):
        return text
    depth = 0
    for index, ch in enumerate(text):
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth -= 1
            if depth == 0:
                return text[index + 1 :].strip()
    return text


def impl_owner(line: str) -> str | None:
    stripped = line.strip()
    if not stripped.startswith("impl"):
        return None
    rest = strip_angle_prefix(stripped[len("impl") :].strip())
    head = rest.split("{", 1)[0].split("where", 1)[0].strip()
    if " for " in head:
        return None
    owner = head.split("<", 1)[0].split("::", 1)[0].strip()
    return owner or None


def brace_delta(line: str) -> tuple[int, bool]:
    depth = 0
    has_open = False
    in_string = False
    escape = False
    index = 0
    while index < len(line):
        ch = line[index]
        nxt = line[index + 1] if index + 1 < len(line) else ""
        if not in_string and ch == "/" and nxt == "/":
            break
        if in_string:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_string = False
        elif ch == '"':
            in_string = True
        elif ch == "{":
            depth += 1
            has_open = True
        elif ch == "}":
            depth -= 1
        index += 1
    return depth, has_open


def method_owner_at(lines: list[str], index: int, public_names: set[str]) -> tuple[bool, str | None]:
    depth = 0
    pending: str | None = None
    impl_stack: list[tuple[int, str | None]] = []
    for line in lines[:index]:
        parsed_owner = impl_owner(line)
        if parsed_owner is not None:
            pending = parsed_owner if parsed_owner in public_names else ""
        delta, has_open = brace_delta(line)
        depth += delta
        if pending is not None and has_open:
            impl_stack.append((depth, pending or None))
            pending = None
        while impl_stack and depth < impl_stack[-1][0]:
            impl_stack.pop()
    if impl_stack:
        return True, impl_stack[-1][1]
    return False, None


def public_member_name(statement: str) -> str:
    for prefix in ("pub const fn ", "pub fn ", "pub const "):
        if statement.startswith(prefix):
            rest = statement[len(prefix) :]
            return rest.split("(", 1)[0].split("<", 1)[0].split(":", 1)[0].split("=", 1)[
                0
            ].strip()
    return "item"


def collect_surface(surface: Surface) -> list[str]:
    items: list[str] = []
    public_names: set[str] = set()
    for source in surface.sources:
        lines = (ROOT / source).read_text(encoding="utf-8").splitlines()
        public_names.update(public_type_names(lines))
    for source in surface.sources:
        path = ROOT / source
        lines = path.read_text(encoding="utf-8").splitlines()
        index = 0
        while index < len(lines):
            stripped = lines[index].strip()
            if is_public_start(stripped) and not should_ignore(stripped, surface):
                statement, index = collect_statement(lines, index)
                if statement.startswith(("pub const fn ", "pub fn ", "pub const ")):
                    in_impl, owner = method_owner_at(lines, index - 1, public_names)
                    if in_impl:
                        if owner is not None:
                            name = public_member_name(statement)
                            items.append(f"{owner}::{name} {statement}")
                        continue
                items.append(statement)
                continue
            index += 1
    return items


def read_allowlist(path: str) -> list[str]:
    full = ROOT / path
    return [
        line.strip()
        for line in full.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


def report_mismatch(label: str, expected: list[str], actual: list[str]) -> None:
    print(f"public API allowlist drift: {label}", file=sys.stderr)
    if actual != expected:
        print("  expected allowlist:", file=sys.stderr)
        for item in expected:
            marker = " " if item in actual else "-"
            print(f"  {marker} {item}", file=sys.stderr)
        print("  scanned source:", file=sys.stderr)
        for item in actual:
            marker = " " if item in expected else "+"
            print(f"  {marker} {item}", file=sys.stderr)


def main() -> int:
    failed = False
    for label, surface in SURFACES.items():
        actual = collect_surface(surface)
        expected = read_allowlist(surface.allowlist)
        if actual != expected:
            report_mismatch(label, expected, actual)
            failed = True
    if failed:
        return 1
    print("public API allowlist scanner passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
