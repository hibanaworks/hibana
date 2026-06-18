#!/usr/bin/env python3
"""Stable source scanner for the curated public Hibana API allowlists."""

from __future__ import annotations

import sys
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory


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
        ("src/g.rs", "src/global/message.rs"),
        ("pub trait Sealed",),
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
            "src/session/types.rs",
            "src/session/cluster/core/dynamic_resolvers.rs",
            "src/session/cluster/error.rs",
            "src/observe/core.rs",
            "src/observe/event.rs",
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
                name = (
                    rest.split("=", 1)[0]
                    .split(":", 1)[0]
                    .split("(", 1)[0]
                    .split("{", 1)[0]
                    .split()[0]
                )
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
            ].split(";", 1)[0].strip()
    return "item"


def public_type_name(statement: str, prefix: str) -> str:
    rest = statement[len(prefix) :]
    return (
        rest.split("=", 1)[0]
        .split(":", 1)[0]
        .split("(", 1)[0]
        .split("{", 1)[0]
        .split()[0]
        .split("<", 1)[0]
    )


def remove_line_comment(line: str) -> str:
    in_string = False
    escape = False
    index = 0
    while index < len(line):
        ch = line[index]
        nxt = line[index + 1] if index + 1 < len(line) else ""
        if in_string:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_string = False
        elif ch == '"':
            in_string = True
        elif ch == "/" and nxt == "/":
            return line[:index]
        index += 1
    return line


def block_text(lines: list[str], start: int) -> str | None:
    depth = 0
    opened = False
    pieces: list[str] = []
    for line in lines[start:]:
        pieces.append(remove_line_comment(line))
        delta, has_open = brace_delta(line)
        if has_open:
            opened = True
        depth += delta
        if opened and depth <= 0:
            text = "\n".join(pieces)
            open_index = text.find("{")
            close_index = text.rfind("}")
            if open_index >= 0 and close_index > open_index:
                return text[open_index + 1 : close_index]
            return ""
    return None


def split_top_level(text: str, delimiter: str) -> list[str]:
    parts: list[str] = []
    start = 0
    paren = brace = bracket = angle = 0
    for index, ch in enumerate(text):
        if ch == "(":
            paren += 1
        elif ch == ")" and paren > 0:
            paren -= 1
        elif ch == "{":
            brace += 1
        elif ch == "}" and brace > 0:
            brace -= 1
        elif ch == "[":
            bracket += 1
        elif ch == "]" and bracket > 0:
            bracket -= 1
        elif ch == "<":
            angle += 1
        elif ch == ">" and angle > 0:
            angle -= 1
        elif (
            ch == delimiter
            and paren == 0
            and brace == 0
            and bracket == 0
            and angle == 0
        ):
            parts.append(text[start:index])
            start = index + 1
    tail = text[start:]
    if tail.strip():
        parts.append(tail)
    return parts


def trim_attributes(text: str) -> str:
    lines = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#[") or stripped.startswith("///"):
            continue
        lines.append(stripped)
    return compact(" ".join(lines))


def parse_named_fields(text: str) -> list[tuple[str, str]]:
    fields: list[tuple[str, str]] = []
    for segment in split_top_level(text, ","):
        field = trim_attributes(segment)
        if ":" not in field:
            continue
        name, ty = field.split(":", 1)
        name = name.strip()
        visibility = ""
        if name.startswith("pub "):
            visibility = "pub "
            name = name[len("pub ") :].strip()
        fields.append((name, compact(f"{visibility}{name}: {ty.strip()}")))
    return fields


def parse_tuple_fields(text: str) -> list[str]:
    fields: list[str] = []
    for segment in split_top_level(text, ","):
        field = trim_attributes(segment)
        if field:
            fields.append(field)
    return fields


def collect_public_struct_shape(statement: str, lines: list[str], start: int) -> list[str]:
    name = public_type_name(statement, "pub struct ")
    items: list[str] = []
    if "(" in statement and ")" in statement and statement.rstrip().endswith(";"):
        fields = statement.split("(", 1)[1].rsplit(")", 1)[0]
        for field_index, field in enumerate(parse_tuple_fields(fields)):
            if field.startswith("pub "):
                items.append(f"{name}::{field_index} pub field {field_index}: {field[4:]}")
        return items
    body = block_text(lines, start)
    if body is None:
        return items
    for field, rendered in parse_named_fields(body):
        if rendered.startswith("pub "):
            items.append(f"{name}::{field} pub field {rendered[4:]}")
    return items


def collect_public_enum_shape(statement: str, lines: list[str], start: int) -> list[str]:
    name = public_type_name(statement, "pub enum ")
    body = block_text(lines, start)
    if body is None:
        return []
    items: list[str] = []
    for segment in split_top_level(body, ","):
        variant = trim_attributes(segment)
        if not variant:
            continue
        variant_name = (
            variant.split("(", 1)[0]
            .split("{", 1)[0]
            .split("=", 1)[0]
            .split()[0]
        )
        if not variant_name:
            continue
        items.append(f"{name}::{variant_name} variant {variant_name}")
        if "(" in variant and ")" in variant:
            fields = variant.split("(", 1)[1].rsplit(")", 1)[0]
            for field_index, field in enumerate(parse_tuple_fields(fields)):
                items.append(
                    f"{name}::{variant_name}::{field_index} field {field_index}: {field}"
                )
        if "{" in variant and "}" in variant:
            fields = variant.split("{", 1)[1].rsplit("}", 1)[0]
            for field, rendered in parse_named_fields(fields):
                items.append(f"{name}::{variant_name}.{field} field {rendered}")
    return items


def trait_owner(line: str) -> str | None:
    stripped = line.strip()
    if not stripped.startswith("pub trait "):
        return None
    rest = stripped[len("pub trait ") :]
    name = rest.split(":", 1)[0].split("{", 1)[0].split()[0]
    return name.split("<", 1)[0] or None


def trait_owner_at(lines: list[str], index: int, public_names: set[str]) -> str | None:
    depth = 0
    pending: str | None = None
    trait_stack: list[tuple[int, str | None]] = []
    for line in lines[:index]:
        parsed_owner = trait_owner(line)
        if parsed_owner is not None:
            pending = parsed_owner if parsed_owner in public_names else ""
        delta, has_open = brace_delta(line)
        depth += delta
        if pending is not None and has_open:
            trait_stack.append((depth, pending or None))
            pending = None
        while trait_stack and depth < trait_stack[-1][0]:
            trait_stack.pop()
    if trait_stack:
        return trait_stack[-1][1]
    return None


def is_trait_item_start(line: str) -> bool:
    return line.startswith(("type ", "const ", "fn "))


def trait_item_name(statement: str) -> str:
    for prefix in ("type ", "const ", "fn "):
        if statement.startswith(prefix):
            rest = statement[len(prefix) :]
            return rest.split("(", 1)[0].split("<", 1)[0].split(":", 1)[0].split("=", 1)[
                0
            ].split(";", 1)[0].strip()
    return "item"


def collect_surface(surface: Surface, root: Path = ROOT) -> list[str]:
    items: list[str] = []
    public_names: set[str] = set()
    for source in surface.sources:
        lines = (root / source).read_text(encoding="utf-8").splitlines()
        public_names.update(public_type_names(lines))
    for source in surface.sources:
        path = root / source
        lines = path.read_text(encoding="utf-8").splitlines()
        index = 0
        while index < len(lines):
            stripped = lines[index].strip()
            trait = trait_owner_at(lines, index, public_names)
            if trait is not None and is_trait_item_start(stripped):
                statement, index = collect_statement(lines, index)
                items.append(f"{trait}::{trait_item_name(statement)} {statement}")
                continue
            if is_public_start(stripped) and not should_ignore(stripped, surface):
                start_index = index
                statement, index = collect_statement(lines, index)
                if statement.startswith(("pub const fn ", "pub fn ", "pub const ")):
                    in_impl, owner = method_owner_at(lines, index - 1, public_names)
                    if in_impl:
                        if owner is not None:
                            name = public_member_name(statement)
                            items.append(f"{owner}::{name} {statement}")
                        continue
                items.append(statement)
                if statement.startswith("pub struct "):
                    items.extend(collect_public_struct_shape(statement, lines, start_index))
                elif statement.startswith("pub enum "):
                    items.extend(collect_public_enum_shape(statement, lines, start_index))
                continue
            index += 1
    return items


def read_allowlist(path: str, root: Path = ROOT) -> list[str]:
    full = root / path
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


def write_fixture(root: Path, path: str, text: str) -> None:
    full = root / path
    full.parent.mkdir(parents=True, exist_ok=True)
    full.write_text(text, encoding="utf-8")


def require_self_test(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def run_self_test() -> int:
    with TemporaryDirectory() as tmp:
        root = Path(tmp)
        source = "src/runtime.rs"
        allowlist = ".github/allowlists/runtime-public-api.txt"
        surface = Surface(allowlist, (source,))
        write_fixture(
            root,
            source,
            """
pub enum FixtureEnum {
    Existing,
    Added,
}

pub struct FixtureStruct {
    pub exposed: u8,
    hidden: u8,
}

pub struct FixtureTuple(pub u8, u16);

pub enum FixtureVariantFields {
    Tuple(u16),
    Struct { named: u8 },
}
""".strip()
            + "\n",
        )

        actual = collect_surface(surface, root)
        for required in [
            "FixtureEnum::Added variant Added",
            "FixtureStruct::exposed pub field exposed: u8",
            "FixtureTuple::0 pub field 0: u8",
            "FixtureVariantFields::Tuple::0 field 0: u16",
            "FixtureVariantFields::Struct.named field named: u8",
        ]:
            require_self_test(
                required in actual,
                f"self-test fixture did not expose public surface item: {required}",
            )

        missing_added_variant = [
            item for item in actual if item != "FixtureEnum::Added variant Added"
        ]
        require_self_test(
            actual != missing_added_variant,
            "enum variant fixture must produce allowlist drift when omitted",
        )

        missing_public_fields = [
            item
            for item in actual
            if item
            not in {
                "FixtureStruct::exposed pub field exposed: u8",
                "FixtureTuple::0 pub field 0: u8",
                "FixtureVariantFields::Tuple::0 field 0: u16",
                "FixtureVariantFields::Struct.named field named: u8",
            }
        ]
        require_self_test(
            actual != missing_public_fields,
            "public field fixture must produce allowlist drift when omitted",
        )

        write_fixture(root, allowlist, "\n".join(actual) + "\n")
        require_self_test(
            read_allowlist(allowlist, root) == actual,
            "self-test fixture allowlist round trip failed",
        )

    print("public API allowlist scanner self-test passed")
    return 0


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        return run_self_test()
    if argv:
        print("usage: check_public_api_allowlists.py [--self-test]", file=sys.stderr)
        return 2
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
    raise SystemExit(main(sys.argv[1:]))
