#!/usr/bin/env python3
"""Compile generated Lean certificates and audit every named theorem's axioms."""

from __future__ import annotations

import pathlib
import re
import subprocess
import sys

sys.dont_write_bytecode = True

from check_lean_theorem_inventory import erase_non_code, theorem_names


EXACT_CERTIFICATE_COUNT = 22
PROJECTABILITY_CERTIFICATE_COUNT = 8
VERIFIED_PROTOCOL_CERTIFICATE_COUNT = 8
NATIVE_DECISION_COUNT = 16

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

AXIOM_BLOCK = re.compile(
    r"^'([^']+)' (does not depend on any axioms|depends on axioms: \[(.*?)\])"
    r"(?=\n'|\s*\Z)",
    re.MULTILINE | re.DOTALL,
)
NATIVE_AXIOM = re.compile(r"^.+\._native\.native_decide\.ax_[0-9_]+$")
NATIVE_AXIOM_OWNER = re.compile(r"^(.+)\._native\.native_decide\.ax_[0-9_]+$")
FORBIDDEN_DECLARATION = re.compile(
    r"\b(sorry|admit)\b|^[ \t]*(axiom|constant|opaque|unsafe)\b", re.MULTILINE
)


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
    expected = kernel | native
    if theorems != expected:
        missing = sorted(expected - theorems)
        unexpected = sorted(theorems - expected)
        raise ValueError(
            f"generated theorem inventory changed: missing={missing!r} unexpected={unexpected!r}"
        )
    return kernel, native


def expected_native_owners(theorem: str) -> set[str]:
    if theorem in PROJECTABILITY_CERTIFICATE_THEOREMS | VERIFIED_PROTOCOL_CERTIFICATE_THEOREMS:
        return {theorem}
    if theorem == "generatedProductionRoleImagesAccepted":
        return {"generatedVerifiedProtocolAccepted"}
    if theorem in NATIVE_PRODUCTION_THEOREMS:
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


def audit(generated: pathlib.Path, proof_dir: pathlib.Path) -> int:
    source = generated.read_text()
    code = erase_non_code(source)
    forbidden = FORBIDDEN_DECLARATION.search(code)
    if forbidden is not None:
        raise ValueError(
            f"generated Lean contains forbidden declaration or proof escape: {forbidden.group(0)!r}"
        )
    if code.count("native_decide") != NATIVE_DECISION_COUNT:
        raise ValueError(
            f"expected {NATIVE_DECISION_COUNT} explicit native decisions, "
            f"found {code.count('native_decide')}"
        )
    theorems = theorem_names(source)
    kernel, native = classify(theorems)
    audit_source = source + "\n" + "\n".join(
        f"#print axioms {name}" for name in sorted(theorems)
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
    if completed.returncode != 0:
        return completed.returncode
    parsed = parse_axioms(completed.stdout)
    validate_axioms(parsed, kernel, native)
    print(
        "Generated Lean axiom audit passed "
        f"theorems={len(theorems)} kernel={len(kernel)} native={len(native)} "
        f"native-decisions={NATIVE_DECISION_COUNT}"
    )
    return 0


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        self_test()
        print("Generated Lean axiom audit self-test passed")
        return 0
    if len(sys.argv) != 3:
        print(
            "usage: check_generated_lean_axioms.py --self-test | GENERATED PROOF_DIR",
            file=sys.stderr,
        )
        return 2
    try:
        return audit(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]))
    except (OSError, ValueError) as error:
        print(f"Generated Lean axiom audit failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
