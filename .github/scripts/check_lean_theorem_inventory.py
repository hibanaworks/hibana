#!/usr/bin/env python3
import difflib
import pathlib
import re
import sys


def erase_non_code(source: str) -> str:
    out = list(source)
    index = 0
    block_depth = 0
    in_line_comment = False
    in_string = False
    escaped = False
    while index < len(source):
        pair = source[index : index + 2]
        char = source[index]
        if in_line_comment:
            if char == "\n":
                in_line_comment = False
            else:
                out[index] = " "
            index += 1
            continue
        if block_depth:
            if pair == "/-":
                out[index] = out[index + 1] = " "
                block_depth += 1
                index += 2
            elif pair == "-/":
                out[index] = out[index + 1] = " "
                block_depth -= 1
                index += 2
            else:
                if char != "\n":
                    out[index] = " "
                index += 1
            continue
        if in_string:
            if char != "\n":
                out[index] = " "
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            index += 1
            continue
        if pair == "--":
            out[index] = out[index + 1] = " "
            in_line_comment = True
            index += 2
        elif pair == "/-":
            out[index] = out[index + 1] = " "
            block_depth = 1
            index += 2
        elif char == '"':
            out[index] = " "
            in_string = True
            index += 1
        else:
            index += 1
    if block_depth or in_string:
        raise ValueError("unterminated Lean comment or string")
    return "".join(out)


def theorem_names(source: str) -> set[str]:
    code = erase_non_code(source)
    pattern = re.compile(r"\btheorem\s+([^\s:({]+)")
    names: set[str] = set()
    for match in pattern.finditer(code):
        prefix = code[: match.start()].rstrip()
        previous = re.search(r"([^\s]+)$", prefix)
        if previous is not None and previous.group(1) == "private":
            continue
        name = match.group(1)
        if name.startswith("«"):
            raise ValueError(f"escaped theorem names are not inventory-safe: {name}")
        names.add(name)
    return names


def declared_theorems(root: pathlib.Path) -> set[str]:
    names: set[str] = set()
    for path in sorted((root / "Hibana").rglob("*.lean")):
        try:
            names.update(theorem_names(path.read_text()))
        except ValueError as error:
            raise ValueError(f"{path}: {error}") from error
    return names


def audited_theorems(root: pathlib.Path) -> set[str]:
    source = erase_non_code((root / "Hibana" / "AxiomAudit.lean").read_text())
    return set(re.findall(r"^#print axioms Hibana\.(\S+)\s*$", source, re.MULTILINE))


def self_test() -> None:
    declarations = r'''
namespace Hibana
theorem plain : True := by trivial
@[simp] protected theorem unicode_一意? : True := by trivial
theorem
  punctuation!' : True := by trivial
private theorem hidden : True := by trivial
-- theorem line_comment : False := by trivial
/- theorem block_comment : False := by trivial
   /- theorem nested_comment : False := by trivial -/
-/
def quoted : String := "theorem string_literal : False"
end Hibana
'''
    expected = {"plain", "unicode_一意?", "punctuation!'"}
    actual = theorem_names(declarations)
    if actual != expected:
        raise AssertionError(f"declaration scanner mismatch: {actual!r}")

    audit = r'''
#print axioms Hibana.plain
#print axioms Hibana.unicode_一意?
#print axioms Hibana.punctuation!'
-- #print axioms Hibana.line_comment
/- #print axioms Hibana.block_comment -/
def quoted : String := "#print axioms Hibana.string_literal"
'''
    actual_audit = set(
        re.findall(
            r"^#print axioms Hibana\.(\S+)\s*$", erase_non_code(audit), re.MULTILINE
        )
    )
    if actual_audit != expected:
        raise AssertionError(f"audit scanner mismatch: {actual_audit!r}")


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        try:
            self_test()
        except (AssertionError, ValueError) as error:
            print(f"Lean theorem inventory self-test failed: {error}", file=sys.stderr)
            return 1
        print("Lean theorem inventory self-test passed")
        return 0
    if len(sys.argv) != 2:
        print(
            "usage: check_lean_theorem_inventory.py --self-test | PROOF_DIR",
            file=sys.stderr,
        )
        return 2
    root = pathlib.Path(sys.argv[1])
    try:
        declared = declared_theorems(root)
        audited = audited_theorems(root)
    except (OSError, ValueError) as error:
        print(f"Lean theorem inventory failed: {error}", file=sys.stderr)
        return 1
    if not declared or not audited:
        print("Lean theorem inventory must be nonempty", file=sys.stderr)
        return 1
    if declared == audited:
        print(f"Lean theorem inventory passed theorems={len(declared)}")
        return 0
    diff = difflib.unified_diff(
        sorted(declared),
        sorted(audited),
        fromfile="declared-theorems",
        tofile="axiom-audit",
        lineterm="",
    )
    print("\n".join(diff), file=sys.stderr)
    print("Lean proof gate requires an axiom audit for every exported theorem", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
