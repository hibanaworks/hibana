#!/usr/bin/env python3
import difflib
import pathlib
import re
import subprocess
import sys


CLAIM_SURFACE_BEGIN = "HIBANA_STATIC_CLAIM_SURFACE_BEGIN"
CLAIM_SURFACE_END = "HIBANA_STATIC_CLAIM_SURFACE_END"
CLAIM_HEADER = re.compile(r"^@?Hibana\.([^\s:]+) :")
EXAMPLE_DECLARATION = re.compile(r"\bexample\b")
AXIOM_BLOCK = re.compile(
    r"^'([^']+)' (does not depend on any axioms|depends on axioms: \[(.*?)\])"
    r"(?=\n'|\s*\Z)",
    re.MULTILINE | re.DOTALL,
)
NATIVE_AXIOM_OWNER = re.compile(r"^(.+)\._native\.native_decide\.ax_[0-9_]+$")
ALLOWED_AXIOM = re.compile(r"^(?:propext|Quot\.sound)(?:\.\{[^}]+\})?$")
NATIVE_EXAMPLE_MODULES = {
    pathlib.Path("DistributedSemanticsExamples.lean"),
    pathlib.Path("StaticProjectabilityExamples.lean"),
}


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
    pattern = re.compile(r"\b(?:theorem|lemma)\s+([^\s:({]+)")
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


def extract_claim_surface(output: str) -> str:
    begin = f"{CLAIM_SURFACE_BEGIN}\n"
    end = f"\n{CLAIM_SURFACE_END}"
    if output.count(begin) != 1 or output.count(end) != 1:
        raise ValueError("static claim surface markers are missing or duplicated")
    return output.split(begin, 1)[1].split(end, 1)[0] + "\n"


def claim_names(surface: str) -> set[str]:
    names = [
        match.group(1)
        for line in surface.splitlines()
        if (match := CLAIM_HEADER.match(line)) is not None
    ]
    if len(names) != len(set(names)):
        raise ValueError("static claim surface contains duplicate theorem headers")
    return set(names)


def elaborated_claim_surface(root: pathlib.Path, theorems: set[str]) -> str:
    source = (
        "import Hibana.MainTheorems\n\n"
        "set_option pp.universes true\n"
        "set_option pp.explicit true\n\n"
        f'#eval IO.println "{CLAIM_SURFACE_BEGIN}"\n'
        + "\n".join(f"#check @Hibana.{name}" for name in sorted(theorems))
        + f'\n#eval IO.println "{CLAIM_SURFACE_END}"\n'
    )
    completed = subprocess.run(
        ["lake", "env", "lean", "--stdin"],
        cwd=root,
        input=source,
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        sys.stdout.write(completed.stdout)
        sys.stderr.write(completed.stderr)
        raise ValueError("Lean failed to elaborate the complete static claim surface")
    return extract_claim_surface(completed.stdout)


def check_claim_types(
    root: pathlib.Path, snapshot: pathlib.Path, expected_count: int
) -> None:
    declared = declared_theorems(root)
    audited = audited_theorems(root)
    if declared != audited:
        raise ValueError("static claim type audit requires an exact theorem/axiom inventory")
    if len(declared) != expected_count:
        raise ValueError(
            f"expected {expected_count} static theorem types, found {len(declared)}"
        )
    actual = elaborated_claim_surface(root, declared)
    actual_names = claim_names(actual)
    if actual_names != declared:
        raise ValueError(
            "static claim type inventory changed: "
            f"missing={sorted(declared - actual_names)!r} "
            f"unexpected={sorted(actual_names - declared)!r}"
        )
    expected = snapshot.read_text()
    if claim_names(expected) != declared:
        raise ValueError("static claim snapshot does not cover the exact theorem inventory")
    if actual != expected:
        difference = "".join(
            difflib.unified_diff(
                expected.splitlines(keepends=True),
                actual.splitlines(keepends=True),
                fromfile=str(snapshot),
                tofile="actual-static-claim-surface.txt",
            )
        )
        raise ValueError(f"static Lean theorem type surface changed:\n{difference}")


def anonymous_example_name(path: pathlib.Path, ordinal: int) -> str:
    owner = re.sub(r"[^A-Za-z0-9']", "_", str(path.with_suffix("")))
    return f"anonymousRegression_{owner}_{ordinal:03d}"


def name_anonymous_examples(
    relative_path: pathlib.Path, source: str
) -> tuple[str, list[str]]:
    code = erase_non_code(source)
    matches = list(EXAMPLE_DECLARATION.finditer(code))
    names = [
        anonymous_example_name(relative_path, ordinal)
        for ordinal in range(1, len(matches) + 1)
    ]
    transformed = source
    for match, name in reversed(list(zip(matches, names, strict=True))):
        token_start = match.start()
        token_end = token_start + len("example")
        transformed = (
            transformed[:token_start]
            + f"theorem {name}"
            + transformed[token_end:]
        )
    return transformed, names


def parse_axioms(output: str) -> dict[str, set[str]]:
    parsed: dict[str, set[str]] = {}
    for match in AXIOM_BLOCK.finditer(output):
        theorem, status, body = match.groups()
        axioms = set() if status == "does not depend on any axioms" else {
            item.strip() for item in body.split(",") if item.strip()
        }
        if theorem in parsed:
            raise ValueError(f"duplicate anonymous example axiom report: {theorem}")
        parsed[theorem] = axioms
    return parsed


def validate_example_axioms(
    output: str, relative_path: pathlib.Path, names: list[str]
) -> None:
    qualified_names = {f"Hibana.{name}" for name in names}
    parsed = parse_axioms(output)
    if set(parsed) != qualified_names:
        raise ValueError(
            "anonymous example axiom inventory changed: "
            f"missing={sorted(qualified_names - set(parsed))!r} "
            f"unexpected={sorted(set(parsed) - qualified_names)!r}"
        )
    expects_native = relative_path in NATIVE_EXAMPLE_MODULES
    for theorem, axioms in parsed.items():
        extra = {axiom for axiom in axioms if ALLOWED_AXIOM.fullmatch(axiom) is None}
        if not expects_native:
            if extra:
                raise ValueError(
                    f"kernel anonymous example {theorem} gained forbidden axioms: "
                    f"{sorted(extra)!r}"
                )
            continue
        if len(extra) != 1:
            raise ValueError(
                f"native anonymous example {theorem} must own one native decision, "
                f"found {sorted(extra)!r}"
            )
        native_axiom = next(iter(extra))
        owner = NATIVE_AXIOM_OWNER.fullmatch(native_axiom)
        if owner is None or owner.group(1) != theorem:
            raise ValueError(
                f"native anonymous example {theorem} has a foreign decision owner: "
                f"{native_axiom}"
            )


def elaborated_example_surface(root: pathlib.Path) -> tuple[str, set[str]]:
    surface = ""
    all_names: set[str] = set()
    source_root = root / "Hibana"
    for path in sorted(source_root.rglob("*.lean")):
        relative_path = path.relative_to(source_root)
        transformed, names = name_anonymous_examples(relative_path, path.read_text())
        if not names:
            continue
        duplicate = all_names.intersection(names)
        if duplicate:
            raise ValueError(f"duplicate anonymous example audit names: {sorted(duplicate)!r}")
        all_names.update(names)
        audit_source = (
            transformed
            + "\nset_option pp.universes true\n"
            + "set_option pp.explicit true\n\n"
            + f'#eval IO.println "{CLAIM_SURFACE_BEGIN}"\n'
            + "\n".join(f"#check @Hibana.{name}" for name in names)
            + f'\n#eval IO.println "{CLAIM_SURFACE_END}"\n'
            + "\n".join(f"#print axioms Hibana.{name}" for name in names)
            + "\n"
        )
        completed = subprocess.run(
            ["lake", "env", "lean", "--stdin"],
            cwd=root,
            input=audit_source,
            text=True,
            capture_output=True,
            check=False,
        )
        if completed.returncode != 0:
            sys.stdout.write(completed.stdout)
            sys.stderr.write(completed.stderr)
            raise ValueError(f"Lean failed to elaborate anonymous examples in {path}")
        validate_example_axioms(completed.stdout, relative_path, names)
        surface += extract_claim_surface(completed.stdout)
    return surface, all_names


def check_example_types(
    root: pathlib.Path, snapshot: pathlib.Path, expected_count: int
) -> None:
    actual, names = elaborated_example_surface(root)
    if len(names) != expected_count:
        raise ValueError(
            f"expected {expected_count} anonymous example types, found {len(names)}"
        )
    if claim_names(actual) != names:
        raise ValueError("anonymous example elaboration did not preserve the exact inventory")
    expected = snapshot.read_text()
    if claim_names(expected) != names:
        raise ValueError("anonymous example snapshot does not cover the exact inventory")
    if actual != expected:
        difference = "".join(
            difflib.unified_diff(
                expected.splitlines(keepends=True),
                actual.splitlines(keepends=True),
                fromfile=str(snapshot),
                tofile="actual-example-claim-surface.txt",
            )
        )
        raise ValueError(f"Lean anonymous example type surface changed:\n{difference}")


def self_test() -> None:
    declarations = r'''
namespace Hibana
theorem plain : True := by trivial
@[simp] protected theorem unicode_一意? : True := by trivial
theorem
  punctuation!' : True := by trivial
lemma equivalent_name : True := by trivial
private theorem hidden : True := by trivial
private lemma hidden_lemma : True := by trivial
-- theorem line_comment : False := by trivial
/- theorem block_comment : False := by trivial
   /- theorem nested_comment : False := by trivial -/
-/
def quoted : String := "theorem string_literal : False"
end Hibana
'''
    expected = {"plain", "unicode_一意?", "punctuation!'", "equivalent_name"}
    actual = theorem_names(declarations)
    if actual != expected:
        raise AssertionError(f"declaration scanner mismatch: {actual!r}")

    audit = r'''
#print axioms Hibana.plain
#print axioms Hibana.unicode_一意?
#print axioms Hibana.punctuation!'
#print axioms Hibana.equivalent_name
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

    claim_output = (
        f"noise\n{CLAIM_SURFACE_BEGIN}\nHibana.plain : True\n"
        f"@Hibana.parameterized : ∀ {{value : Nat}}, value = value\n"
        f"{CLAIM_SURFACE_END}\nnoise"
    )
    claim_surface = extract_claim_surface(claim_output)
    if claim_names(claim_surface) != {"plain", "parameterized"}:
        raise AssertionError("static claim surface scanner mismatch")

    examples = r'''
namespace Hibana
example : True := by trivial
-- example : False := by trivial
def quoted : String := "example : False"
  /- example : False := by trivial -/
example (value : Nat) : value = value := by rfl
end Hibana
'''
    transformed, names = name_anonymous_examples(pathlib.Path("Examples.lean"), examples)
    if names != [
        "anonymousRegression_Examples_001",
        "anonymousRegression_Examples_002",
    ]:
        raise AssertionError(f"anonymous example names changed: {names!r}")
    if EXAMPLE_DECLARATION.search(erase_non_code(transformed)) is not None:
        raise AssertionError("anonymous example audit left an unnamed obligation")

    native_name = "anonymousRegression_StaticProjectabilityExamples_001"
    native_theorem = f"Hibana.{native_name}"
    native_axiom = f"{native_theorem}._native.native_decide.ax_1_1"
    validate_example_axioms(
        f"'{native_theorem}' depends on axioms: "
        f"[propext, Quot.sound.{{u}}, {native_axiom}]\n",
        pathlib.Path("StaticProjectabilityExamples.lean"),
        [native_name],
    )
    try:
        validate_example_axioms(
            f"'{native_theorem}' depends on axioms: [foreign.ax]\n",
            pathlib.Path("StaticProjectabilityExamples.lean"),
            [native_name],
        )
    except ValueError:
        pass
    else:
        raise AssertionError("anonymous example audit accepted a foreign native axiom")


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        try:
            self_test()
        except (AssertionError, ValueError) as error:
            print(f"Lean theorem inventory self-test failed: {error}", file=sys.stderr)
            return 1
        print("Lean theorem inventory self-test passed")
        return 0
    if sys.argv[1:2] == ["--claim-types"]:
        if len(sys.argv) != 5:
            print(
                "usage: check_lean_theorem_inventory.py --claim-types "
                "PROOF_DIR SNAPSHOT EXPECTED_COUNT",
                file=sys.stderr,
            )
            return 2
        try:
            check_claim_types(
                pathlib.Path(sys.argv[2]),
                pathlib.Path(sys.argv[3]),
                int(sys.argv[4]),
            )
        except (OSError, ValueError) as error:
            print(f"Lean static claim type audit failed: {error}", file=sys.stderr)
            return 1
        print(f"Lean static claim type audit passed theorems={sys.argv[4]}")
        return 0
    if sys.argv[1:2] in (["--example-types"], ["--write-example-types"]):
        if len(sys.argv) != 5:
            print(
                "usage: check_lean_theorem_inventory.py "
                "--example-types|--write-example-types "
                "PROOF_DIR SNAPSHOT EXPECTED_COUNT",
                file=sys.stderr,
            )
            return 2
        root = pathlib.Path(sys.argv[2])
        snapshot = pathlib.Path(sys.argv[3])
        expected_count = int(sys.argv[4])
        try:
            if sys.argv[1] == "--write-example-types":
                surface, names = elaborated_example_surface(root)
                if len(names) != expected_count:
                    raise ValueError(
                        f"expected {expected_count} anonymous example types, "
                        f"found {len(names)}"
                    )
                snapshot.write_text(surface)
            check_example_types(root, snapshot, expected_count)
        except (OSError, ValueError) as error:
            print(f"Lean anonymous example type audit failed: {error}", file=sys.stderr)
            return 1
        print(f"Lean anonymous example type audit passed examples={expected_count}")
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
