#!/usr/bin/env python3
"""Compile generated Lean certificates and audit every named theorem's axioms."""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys
from difflib import unified_diff

sys.dont_write_bytecode = True

from check_lean_theorem_inventory import erase_non_code, theorem_names


EXACT_CERTIFICATE_COUNT = 22
PROJECTABILITY_CERTIFICATE_COUNT = 8
VERIFIED_PROTOCOL_CERTIFICATE_COUNT = 8
NATIVE_DECISION_COUNT = 16
GENERATED_THEOREM_COUNT = 506
CONTRACT_THEOREM_COUNT = 48
CLAIM_SURFACE_BEGIN = "HIBANA_GENERATED_CLAIM_SURFACE_BEGIN"
CLAIM_SURFACE_END = "HIBANA_GENERATED_CLAIM_SURFACE_END"

EXACT_CERTIFICATE_THEOREMS = {
    *(f"generatedProjectionRole{role}ExactAccepted" for role in range(4)),
    *(f"generatedResolvedProjectionRole{role}ExactAccepted" for role in range(2)),
    *(f"generatedRejectingProjectionRole{role}ExactAccepted" for role in range(2)),
    *(f"generatedRolledProjectionRole{role}ExactAccepted" for role in range(2)),
    *(f"generatedRolledResolvedProjectionRole{role}ExactAccepted" for role in range(2)),
    *(f"generatedNestedResolvedProjectionRole{role}ExactAccepted" for role in range(2)),
    *(f"generatedNestedRolledProjectionRole{role}ExactAccepted" for role in range(2)),
    *(f"generatedCyclicRollProjectionRole{role}ExactAccepted" for role in range(3)),
    "generatedFullRoleDomainProjectionRole254ExactAccepted",
    "generatedFullRoleDomainProjectionRole255ExactAccepted",
    "generatedLaneMatchingProjectionRole3ExactAccepted",
}

PROJECTABILITY_CERTIFICATE_THEOREMS = {
    "generatedProjectabilityAccepted",
    "generatedResolvedProjectabilityAccepted",
    "generatedRejectingProjectabilityAccepted",
    "generatedRolledProjectabilityAccepted",
    "generatedRolledResolvedProjectabilityAccepted",
    "generatedNestedResolvedProjectabilityAccepted",
    "generatedNestedRolledProjectabilityAccepted",
    "generatedCyclicRollProjectabilityAccepted",
}

VERIFIED_PROTOCOL_CERTIFICATE_THEOREMS = {
    "generatedVerifiedProtocolAccepted",
    "generatedResolvedVerifiedProtocolAccepted",
    "generatedRejectingVerifiedProtocolAccepted",
    "generatedRolledVerifiedProtocolAccepted",
    "generatedRolledResolvedVerifiedProtocolAccepted",
    "generatedNestedResolvedVerifiedProtocolAccepted",
    "generatedNestedRolledVerifiedProtocolAccepted",
    "generatedCyclicRollVerifiedProtocolAccepted",
}

KERNEL_PRODUCTION_THEOREMS = {
    "generatedProductionKernelArtifactAccepted",
    "generated_production_protocol_cases_refine_prepared_kernels",
    "generated_production_codec_coverage_for",
    "generated_production_codec_coverage",
    "generated_production_closing_carrier",
}

NATIVE_PRODUCTION_THEOREMS = {
    "generatedVerifiedProtocolFamilyCapabilitiesAccepted",
    "generated_verified_protocol_family_covers_core_capabilities",
    "generatedStaticDeploymentCertificateAccepted",
    "generated_static_deployment_certificate_refines_exact_family",
    "generatedProductionRoleImagesAccepted",
}

STATIC_DEPLOYMENT_REJECTION_THEOREMS = {
    "generatedMissingStaticDeploymentCertificateRejected",
    "generatedExtraStaticDeploymentCertificateRejected",
    "generatedCorruptStaticDeploymentCertificateRejected",
}

AXIOM_BLOCK = re.compile(
    r"^'([^\n]+)' (does not depend on any axioms|depends on axioms: \[(.*?)\])"
    r"(?=\n'|\s*\Z)",
    re.MULTILINE | re.DOTALL,
)
NATIVE_AXIOM = re.compile(r"^.+\._native\.native_decide\.ax_[0-9_]+$")
NATIVE_AXIOM_OWNER = re.compile(r"^(.+)\._native\.native_decide\.ax_[0-9_]+$")
FORBIDDEN_DECLARATION = re.compile(
    r"\b(?:sorry|admit|axiom|constant|opaque|unsafe|example|macro|macro_rules|"
    r"syntax|elab|run_cmd|run_tac)\b"
)
CLAIM_HEADER = re.compile(r"^@?([A-Za-z_][A-Za-z0-9_']*) :")


def classify(theorems: set[str]) -> tuple[set[str], set[str]]:
    exact = {
        name
        for name in theorems
        if re.fullmatch(r"generated.*ProjectionRole\d+ExactAccepted", name)
    }
    projectability = {
        name for name in theorems if re.fullmatch(r"generated.*ProjectabilityAccepted", name)
    }
    verified_protocol = {
        name for name in theorems if re.fullmatch(r"generated.*VerifiedProtocolAccepted", name)
    }
    if len(exact) != EXACT_CERTIFICATE_COUNT:
        raise ValueError(
            f"expected {EXACT_CERTIFICATE_COUNT} exact certificate theorems, found {len(exact)}"
        )
    if len(projectability) != PROJECTABILITY_CERTIFICATE_COUNT:
        raise ValueError(
            "expected "
            f"{PROJECTABILITY_CERTIFICATE_COUNT} projectability theorems, "
            f"found {len(projectability)}"
        )
    if len(verified_protocol) != VERIFIED_PROTOCOL_CERTIFICATE_COUNT:
        raise ValueError(
            "expected "
            f"{VERIFIED_PROTOCOL_CERTIFICATE_COUNT} verified protocol theorems, "
            f"found {len(verified_protocol)}"
        )
    for label, actual, expected in [
        ("exact certificate", exact, EXACT_CERTIFICATE_THEOREMS),
        ("projectability", projectability, PROJECTABILITY_CERTIFICATE_THEOREMS),
        ("verified protocol", verified_protocol, VERIFIED_PROTOCOL_CERTIFICATE_THEOREMS),
    ]:
        if actual != expected:
            raise ValueError(
                f"generated {label} inventory changed: "
                f"missing={sorted(expected - actual)!r} unexpected={sorted(actual - expected)!r}"
            )
    kernel = exact | KERNEL_PRODUCTION_THEOREMS
    native = projectability | verified_protocol | NATIVE_PRODUCTION_THEOREMS
    contract = kernel | native
    if len(contract) != CONTRACT_THEOREM_COUNT:
        raise ValueError(
            f"expected {CONTRACT_THEOREM_COUNT} generated contract theorems, "
            f"found {len(contract)}"
        )
    if not contract <= theorems:
        missing = sorted(contract - theorems)
        raise ValueError(
            f"generated contract theorem inventory changed: missing={missing!r}"
        )
    auxiliary = theorems - contract
    native_auxiliary = {
        name
        for name in auxiliary
        if name.endswith("ProjectabilityEnsuresGlobalProgress")
        or name.endswith("VerifiedProtocolAllRolesRefine")
        or name in STATIC_DEPLOYMENT_REJECTION_THEOREMS
    }
    return kernel | (auxiliary - native_auxiliary), native | native_auxiliary


def expected_native_owners(theorem: str) -> set[str]:
    if theorem in PROJECTABILITY_CERTIFICATE_THEOREMS | VERIFIED_PROTOCOL_CERTIFICATE_THEOREMS:
        return {theorem}
    if theorem == "generatedProductionRoleImagesAccepted":
        return {"generatedVerifiedProtocolAccepted"}
    if theorem in NATIVE_PRODUCTION_THEOREMS:
        return VERIFIED_PROTOCOL_CERTIFICATE_THEOREMS
    if theorem.endswith("ProjectabilityEnsuresGlobalProgress"):
        return {theorem.removesuffix("EnsuresGlobalProgress") + "Accepted"}
    if theorem.endswith("VerifiedProtocolAllRolesRefine"):
        return {theorem.removesuffix("AllRolesRefine") + "Accepted"}
    if theorem in STATIC_DEPLOYMENT_REJECTION_THEOREMS:
        return VERIFIED_PROTOCOL_CERTIFICATE_THEOREMS
    raise ValueError(f"generated theorem is not classified as native: {theorem}")


def parse_axioms(output: str) -> dict[str, set[str]]:
    parsed: dict[str, set[str]] = {}
    for match in AXIOM_BLOCK.finditer(output):
        name = match.group(1)
        if match.group(2) == "does not depend on any axioms":
            axioms: set[str] = set()
        else:
            axioms = {entry.strip() for entry in match.group(3).split(",")}
        if name in parsed:
            raise ValueError(f"duplicate generated axiom report: {name}")
        parsed[name] = axioms
    return parsed


def validate_axioms(
    parsed: dict[str, set[str]], kernel: set[str], native: set[str]
) -> None:
    expected = kernel | native
    if set(parsed) != expected:
        raise ValueError(
            "generated axiom inventory incomplete: "
            f"missing={sorted(expected - set(parsed))!r} "
            f"unexpected={sorted(set(parsed) - expected)!r}"
        )
    for name, axioms in parsed.items():
        unknown = {
            axiom
            for axiom in axioms
            if axiom not in {"propext", "Quot.sound"} and not NATIVE_AXIOM.fullmatch(axiom)
        }
        if unknown:
            raise ValueError(
                f"generated theorem {name} gained forbidden axioms: {sorted(unknown)!r}"
            )
        native_axioms = {axiom for axiom in axioms if NATIVE_AXIOM.fullmatch(axiom)}
        if name in kernel and native_axioms:
            raise ValueError(
                f"kernel-checked generated theorem {name} depends on native_decide: "
                f"{sorted(native_axioms)!r}"
            )
        if name in native and not native_axioms:
            raise ValueError(f"native generated theorem {name} lost its explicit native boundary")
        if name in native:
            actual_owners = {
                owner.group(1)
                for axiom in native_axioms
                if (owner := NATIVE_AXIOM_OWNER.fullmatch(axiom)) is not None
            }
            expected_owners = expected_native_owners(name)
            if actual_owners != expected_owners:
                raise ValueError(
                    f"native generated theorem {name} has the wrong closure dependencies: "
                    f"expected={sorted(expected_owners)!r} actual={sorted(actual_owners)!r}"
                )


def extract_claim_surface(output: str) -> str:
    begin = f"{CLAIM_SURFACE_BEGIN}\n"
    end = f"\n{CLAIM_SURFACE_END}"
    if output.count(begin) != 1 or output.count(end) != 1:
        raise ValueError("generated claim surface markers are missing or duplicated")
    return output.split(begin, 1)[1].split(end, 1)[0] + "\n"


def claim_names(surface: str) -> set[str]:
    names = [
        match.group(1)
        for line in surface.splitlines()
        if (match := CLAIM_HEADER.match(line)) is not None
    ]
    if len(names) != len(set(names)):
        raise ValueError("generated claim surface contains duplicate theorem headers")
    return set(names)


def validate_claim_surface(actual: str, expected: str, theorems: set[str]) -> None:
    actual_names = claim_names(actual)
    if actual_names != theorems:
        raise ValueError(
            "generated claim surface inventory changed: "
            f"missing={sorted(theorems - actual_names)!r} "
            f"unexpected={sorted(actual_names - theorems)!r}"
        )
    if actual != expected:
        difference = "".join(
            unified_diff(
                expected.splitlines(keepends=True),
                actual.splitlines(keepends=True),
                fromfile="generated-claim-surface.txt",
                tofile="actual-generated-claim-surface.txt",
            )
        )
        raise ValueError(f"generated theorem type surface changed:\n{difference}")


def assert_axioms_rejected(
    parsed: dict[str, set[str]], kernel: set[str], native: set[str], message: str
) -> None:
    try:
        validate_axioms(parsed, kernel, native)
    except ValueError:
        return
    raise AssertionError(message)


def self_test() -> None:
    theorems = (
        EXACT_CERTIFICATE_THEOREMS
        | PROJECTABILITY_CERTIFICATE_THEOREMS
        | VERIFIED_PROTOCOL_CERTIFICATE_THEOREMS
        | KERNEL_PRODUCTION_THEOREMS
        | NATIVE_PRODUCTION_THEOREMS
        | {
            "generatedProjectabilityEnsuresGlobalProgress",
            "generatedVerifiedProtocolAllRolesRefine",
        }
        | STATIC_DEPLOYMENT_REJECTION_THEOREMS
    )
    kernel, native = classify(theorems)
    lines = [f"'{name}' does not depend on any axioms" for name in sorted(kernel)]
    for name in sorted(native):
        axioms = ", ".join(
            f"{owner}._native.native_decide.ax_1"
            for owner in sorted(expected_native_owners(name))
        )
        lines.append(f"'{name}' depends on axioms: [{axioms}]")
    output = "\n".join(lines)
    parsed = parse_axioms(output)
    validate_axioms(parsed, kernel, native)

    kernel_contamination = {name: set(axioms) for name, axioms in parsed.items()}
    kernel_name = min(kernel)
    kernel_contamination[kernel_name].add(
        "generatedVerifiedProtocolAccepted._native.native_decide.ax_1"
    )
    assert_axioms_rejected(
        kernel_contamination,
        kernel,
        native,
        "generated axiom audit accepted native contamination",
    )

    wrong_closure = {name: set(axioms) for name, axioms in parsed.items()}
    projectability_name = "generatedProjectabilityAccepted"
    wrong_closure[projectability_name] = {
        "generatedResolvedProjectabilityAccepted._native.native_decide.ax_1"
    }
    assert_axioms_rejected(
        wrong_closure,
        kernel,
        native,
        "generated axiom audit accepted a substituted closure",
    )

    missing_native = {name: set(axioms) for name, axioms in parsed.items()}
    missing_native[projectability_name] = set()
    assert_axioms_rejected(
        missing_native,
        kernel,
        native,
        "generated axiom audit accepted a missing native boundary",
    )

    claim_output = (
        f"noise\n{CLAIM_SURFACE_BEGIN}\nalpha : True\n@beta : False\n"
        f"{CLAIM_SURFACE_END}\nnoise"
    )
    claim_surface = extract_claim_surface(claim_output)
    validate_claim_surface(
        claim_surface,
        "alpha : True\n@beta : False\n",
        {"alpha", "beta"},
    )
    try:
        validate_claim_surface(
            claim_surface,
            "alpha : False\n@beta : False\n",
            {"alpha", "beta"},
        )
    except ValueError:
        pass
    else:
        raise AssertionError("generated claim audit accepted a weakened theorem type")
    if FORBIDDEN_DECLARATION.search("example : True := by trivial") is None:
        raise AssertionError("generated source audit accepted an anonymous proof obligation")
    if FORBIDDEN_DECLARATION.search("private axiom hidden : False") is None:
        raise AssertionError("generated source audit accepted a prefixed custom axiom")
    if FORBIDDEN_DECLARATION.search("run_cmd synthesizeClaim") is None:
        raise AssertionError("generated source audit accepted proof-generating syntax")


def checked_source(generated: pathlib.Path, expected_native_decisions: int) -> str:
    source = generated.read_text()
    code = erase_non_code(source)
    forbidden = FORBIDDEN_DECLARATION.search(code)
    if forbidden is not None:
        raise ValueError(
            "generated Lean contains a forbidden declaration, anonymous proof, "
            f"or proof escape: {forbidden.group(0)!r}"
        )
    if code.count("native_decide") != expected_native_decisions:
        raise ValueError(
            f"expected {expected_native_decisions} explicit native decisions, "
            f"found {code.count('native_decide')}"
        )
    return source


def compile_audit_source(
    source: str, theorems: set[str], proof_dir: pathlib.Path
) -> subprocess.CompletedProcess[str]:
    sorted_theorems = sorted(theorems)
    audit_source = (
        source
        + f'\n#eval IO.println "{CLAIM_SURFACE_BEGIN}"\n'
        + "\n".join(f"#check @{name}" for name in sorted_theorems)
        + f'\n#eval IO.println "{CLAIM_SURFACE_END}"\n'
        + "\n".join(f"#print axioms {name}" for name in sorted_theorems)
    )
    completed = subprocess.run(
        ["lake", "env", "lean", "--stdin"],
        cwd=proof_dir,
        input=audit_source,
        text=True,
        capture_output=True,
        check=False,
    )
    sys.stdout.write(completed.stdout)
    sys.stderr.write(completed.stderr)
    return completed


def audit(
    generated: pathlib.Path, proof_dir: pathlib.Path, claim_snapshot: pathlib.Path
) -> int:
    source = checked_source(generated, NATIVE_DECISION_COUNT)
    theorems = theorem_names(source)
    expected_surface = claim_snapshot.read_text()
    expected_theorems = claim_names(expected_surface)
    if len(expected_theorems) != GENERATED_THEOREM_COUNT:
        raise ValueError(
            f"generated claim snapshot must name exactly {GENERATED_THEOREM_COUNT} "
            f"theorems, found {len(expected_theorems)}"
        )
    if theorems != expected_theorems:
        raise ValueError(
            "generated theorem inventory changed: "
            f"missing={sorted(expected_theorems - theorems)!r} "
            f"unexpected={sorted(theorems - expected_theorems)!r}"
        )
    kernel, native = classify(theorems)
    completed = compile_audit_source(source, theorems, proof_dir)
    if completed.returncode != 0:
        return completed.returncode
    claim_surface = extract_claim_surface(completed.stdout)
    validate_claim_surface(claim_surface, expected_surface, theorems)
    parsed = parse_axioms(completed.stdout)
    validate_axioms(parsed, kernel, native)
    print(
        "Generated Lean axiom audit passed "
        f"theorems={len(theorems)} kernel={len(kernel)} native={len(native)} "
        f"contracts={CONTRACT_THEOREM_COUNT} obligations="
        f"{len(theorems) - CONTRACT_THEOREM_COUNT} "
        f"native-decisions={NATIVE_DECISION_COUNT} claims={len(theorems)}"
    )
    return 0


def audit_kernel_artifact(
    generated: pathlib.Path,
    proof_dir: pathlib.Path,
    claim_snapshot: pathlib.Path,
    expected_theorem_count: int,
) -> int:
    source = checked_source(generated, 0)
    expected_surface = claim_snapshot.read_text()
    expected_theorems = claim_names(expected_surface)
    if len(expected_theorems) != expected_theorem_count:
        raise ValueError(
            f"kernel artifact claim snapshot must name exactly {expected_theorem_count} "
            f"theorems, found {len(expected_theorems)}"
        )
    actual_theorems = theorem_names(source)
    if actual_theorems != expected_theorems:
        raise ValueError(
            "kernel artifact theorem inventory changed: "
            f"missing={sorted(expected_theorems - actual_theorems)!r} "
            f"unexpected={sorted(actual_theorems - expected_theorems)!r}"
        )
    completed = compile_audit_source(source, actual_theorems, proof_dir)
    if completed.returncode != 0:
        return completed.returncode
    claim_surface = extract_claim_surface(completed.stdout)
    validate_claim_surface(claim_surface, expected_surface, actual_theorems)
    validate_axioms(parse_axioms(completed.stdout), actual_theorems, set())
    print(
        "Generated Lean kernel artifact audit passed "
        f"artifact={generated.stem} theorems={len(actual_theorems)} "
        f"claims={len(actual_theorems)}"
    )
    return 0


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        self_test()
        print("Generated Lean axiom audit self-test passed")
        return 0
    if sys.argv[1:2] == ["--kernel"]:
        if len(sys.argv) != 6:
            print(
                "usage: check_generated_lean_axioms.py --kernel "
                "GENERATED PROOF_DIR CLAIM_SNAPSHOT EXPECTED_THEOREMS",
                file=sys.stderr,
            )
            return 2
        try:
            return audit_kernel_artifact(
                pathlib.Path(sys.argv[2]),
                pathlib.Path(sys.argv[3]),
                pathlib.Path(sys.argv[4]),
                int(sys.argv[5]),
            )
        except (OSError, ValueError) as error:
            print(f"Generated Lean kernel artifact audit failed: {error}", file=sys.stderr)
            return 1
    if len(sys.argv) != 4:
        print(
            "usage: check_generated_lean_axioms.py --self-test | "
            "GENERATED PROOF_DIR CLAIM_SNAPSHOT",
            file=sys.stderr,
        )
        return 2
    try:
        return audit(
            pathlib.Path(sys.argv[1]),
            pathlib.Path(sys.argv[2]),
            pathlib.Path(sys.argv[3]),
        )
    except (OSError, ValueError) as error:
        print(f"Generated Lean axiom audit failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
