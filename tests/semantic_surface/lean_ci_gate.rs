use super::common::{read, repo_file_exists};

#[test]
fn lean_ci_gate_audits_every_exported_theorem_and_runs_pinned_artifacts() {
    let proof_gate = read(".github/scripts/check_lean_proofs.sh");
    let theorem_inventory = read(".github/scripts/check_lean_theorem_inventory.py");
    let generated_axiom_audit = read(".github/scripts/check_generated_lean_axioms.py");
    let generated_claim_snapshot = read("proofs/lean/generated-claim-surface.txt");
    let runtime_claim_snapshot = read("proofs/lean/runtime-generated-claim-surface.txt");
    let public_operation_claim_snapshot =
        read("proofs/lean/public-operation-generated-claim-surface.txt");
    let claim_gate = read(".github/scripts/check_lean_claim_surface.sh");
    let claim_source = read("proofs/lean/ClaimSurface.lean");
    let claim_snapshot = read("proofs/lean/claim-surface.txt");
    let all_claim_snapshot = read("proofs/lean/all-claim-surface.txt");
    let example_claim_snapshot = read("proofs/lean/example-claim-surface.txt");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let workflow = read(".github/workflows/quality-gates.yml");

    assert!(
        proof_gate.contains("must remain Core/Std-only")
            && proof_gate.contains("check_lean_theorem_inventory.py")
            && proof_gate.contains("check_lean_theorem_inventory.py\" --self-test")
            && proof_gate.contains("check_generated_lean_axioms.py")
            && proof_gate.contains("check_generated_lean_axioms.py\" --self-test")
            && proof_gate.contains("check_lean_claim_surface.sh")
            && !proof_gate.contains("AxiomAudit.lean")
            && !repo_file_exists("proofs/lean/Hibana/AxiomAudit.lean")
            && proof_gate.contains("native-regressions=32")
            && theorem_inventory.contains("def erase_non_code(source: str)")
            && theorem_inventory.contains("sys.dont_write_bytecode = True")
            && theorem_inventory.contains("def theorem_names(source: str)")
            && theorem_inventory.contains("def validate_static_source(root: pathlib.Path)")
            && theorem_inventory.contains("FORBIDDEN_PROOF_TOKEN")
            && theorem_inventory.contains("FORBIDDEN_PROOF_GENERATOR_TOKEN")
            && theorem_inventory.contains("NATIVE_DECISION_TOKEN")
            && theorem_inventory.contains("ENV_THEOREM_HEADER")
            && theorem_inventory.contains("EXPECTED_NATIVE_EXAMPLE_COUNT = 32")
            && theorem_inventory.contains("forbidden proof token")
            && theorem_inventory.contains(
                "native_decide is outside a native regression module",
            )
            && theorem_inventory.contains(
                "native regression modules cannot export named claims",
            )
            && theorem_inventory.contains("def elaborated_static_audit(")
            && theorem_inventory.contains("def validate_environment_theorem_inventory(")
            && theorem_inventory.contains("name.getPrefix == `Hibana && info.isTheorem")
            && theorem_inventory.contains("def check_static_theorems(")
            && theorem_inventory.contains("def validate_static_axioms(")
            && theorem_inventory.contains("static Lean theorem type surface changed")
            && theorem_inventory.contains("gained a forbidden axiom")
            && theorem_inventory.contains("static theorem axiom closure counts changed")
            && theorem_inventory.contains("def name_anonymous_examples(")
            && theorem_inventory.contains("def elaborated_example_surface(")
            && theorem_inventory.contains("def check_example_types(")
            && theorem_inventory.contains("def validate_example_axioms(")
            && theorem_inventory.contains("must own one native decision")
            && theorem_inventory.contains("has a foreign decision owner")
            && theorem_inventory.contains("Lean anonymous example type surface changed")
            && theorem_inventory.contains("if previous is not None")
            && theorem_inventory.contains("previous.group(1) == \"private\"")
            && theorem_inventory.contains("escaped theorem names are not inventory-safe")
            && theorem_inventory.contains("def self_test()")
            && theorem_inventory.contains("unicode_一意?")
            && theorem_inventory.contains("punctuation!'")
            && theorem_inventory.contains("lemma equivalent_name")
            && generated_axiom_audit.contains("NATIVE_DECISION_COUNT = 16")
            && generated_axiom_audit.contains("GENERATED_THEOREM_COUNT = 506")
            && generated_axiom_audit.contains("CONTRACT_THEOREM_COUNT = 48")
            && generated_axiom_audit.contains("sys.dont_write_bytecode = True")
            && generated_axiom_audit.contains("EXACT_CERTIFICATE_COUNT = 22")
            && generated_axiom_audit.contains("PROJECTABILITY_CERTIFICATE_COUNT = 8")
            && generated_axiom_audit.contains("VERIFIED_PROTOCOL_CERTIFICATE_COUNT = 8")
            && generated_axiom_audit.contains("def validate_axioms(")
            && generated_axiom_audit.contains("NATIVE_AXIOM_OWNER")
            && generated_axiom_audit.contains("kernel-checked generated theorem")
            && generated_axiom_audit.contains("opaque|unsafe|example")
            && generated_axiom_audit.contains("macro_rules|")
            && generated_axiom_audit.contains("run_cmd|run_tac")
            && generated_axiom_audit.contains("gained forbidden axioms")
            && generated_axiom_audit.contains("has the wrong closure dependencies")
            && generated_axiom_audit.contains("EXACT_CERTIFICATE_THEOREMS")
            && generated_axiom_audit.contains("PROJECTABILITY_CERTIFICATE_THEOREMS")
            && generated_axiom_audit.contains("VERIFIED_PROTOCOL_CERTIFICATE_THEOREMS")
            && generated_axiom_audit.contains("CLAIM_SURFACE_BEGIN")
            && generated_axiom_audit.contains("def extract_claim_surface(")
            && generated_axiom_audit.contains("def validate_claim_surface(")
            && generated_axiom_audit.contains("generated theorem type surface changed")
            && generated_axiom_audit.contains("def audit_kernel_artifact(")
            && generated_axiom_audit.contains("kernel artifact theorem inventory changed")
            && generated_claim_snapshot
                .lines()
                .filter(|line| {
                    line.trim_start_matches('@').starts_with("generated")
                        && line.contains(" : ")
                })
                .count()
                == 506
            && generated_claim_snapshot.contains(
                "generatedProductionKernelArtifactAccepted : generatedProductionKernelArtifact.check"
            )
            && generated_claim_snapshot.contains(
                "generated_static_deployment_certificate_refines_exact_family : generatedStaticDeploymentCertificate.Refines"
            )
            && runtime_claim_snapshot
                .lines()
                .filter(|line| line.starts_with("generated") && line.contains(" : "))
                .count()
                == 16
            && runtime_claim_snapshot.contains(
                "generatedCompactingAbortPreservesAuthorityAndCapacity : generatedCompactingAbort.PreservesAuthorityAndCapacity"
            )
            && public_operation_claim_snapshot
                .lines()
                .filter(|line| line.starts_with("generated") && line.contains(" : "))
                .count()
                == 2
            && public_operation_claim_snapshot.contains(
                "generatedPublicOperationTableExact : Hibana.PublicOperationTableExact"
            )
            && claim_gate.contains("EXPECTED_CLAIMS=15")
            && claim_gate.contains("diff -u \"${SNAPSHOT}\" \"${actual}\"")
            && claim_source.matches("#check @Hibana.").count() == 15
            && claim_snapshot.contains(
                "@Hibana.assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement"
            )
            && claim_snapshot
                .contains("@Hibana.assumption_indexed_static_cross_tool_production_refinement")
            && all_claim_snapshot
                .lines()
                .filter(|line| {
                    line.trim_start_matches('@').starts_with("Hibana.")
                        && line.contains(" : ")
                })
                .count()
                == 677
            && all_claim_snapshot.contains(
                "@Hibana.assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement :"
            )
            && proof_gate.contains("all-claim-surface.txt")
            && proof_gate.contains(
                "--static \"${PROOF_DIR}\" \"${STATIC_CLAIM_SNAPSHOT}\" 677 346 239 92",
            )
            && proof_gate.contains("EXPECTED_STATIC_AUDIT_MARKER")
            && proof_gate.contains("theorems=677 both=346 propext=239 free=92")
            && proof_gate.contains("example-claim-surface.txt")
            && proof_gate.contains(
                "--example-types \"${PROOF_DIR}\" \"${EXAMPLE_CLAIM_SNAPSHOT}\" 36",
            )
            && example_claim_snapshot
                .lines()
                .filter(|line| {
                    line.trim_start_matches('@').starts_with("Hibana.anonymousRegression_")
                        && line.contains(" : ")
                })
                .count()
                == 36
            && !proof_gate.contains("axiom_both_count=\"$(awk")
            && !theorem_inventory.contains("def audited_theorems(")
            && proof_gate.contains(
                "traces=14 frames=66 projections=22 exact-descriptors=22 progress=4 projectability=8 distributed-progress=8 verified-protocols=8"
            )
            && proof_gate.contains(
                "production artifact passed transitions=7 operations=6 owners=8 kernel-refinement=external-premise owner-evidence=external-premise codecs=3 family=8 deployments=8 deployment-rejections=3 capabilities=6 agreement=static-exact-family profile=closing"
            )
            && proof_gate.contains("EXPECTED_PRODUCTION_MARKER")
            && proof_gate.contains("EXPECTED_GENERATED_AXIOM_MARKER")
            && proof_gate.contains("EXPECTED_RUNTIME_AUDIT_MARKER")
            && proof_gate.contains("EXPECTED_PUBLIC_OPERATION_AUDIT_MARKER")
            && proof_gate.contains("--kernel \"${RUNTIME_GENERATED}\"")
            && proof_gate.contains("--kernel \"${PUBLIC_OPERATION_GENERATED}\"")
            && proof_gate
                .contains("theorems=506 kernel=466 native=40 contracts=48 obligations=458 native-decisions=16 claims=506")
            && proof_gate.contains("generated-theorems=506 generated-obligations=458")
            && proof_gate.contains("static-theorems=677 anonymous-regressions=36")
            && proof_gate.contains(
                "production-transitions=7 production-operations=6 production-owners=8 verified-codecs=3 verified-family=8 static-deployments=8 deployment-rejections=3 capabilities=6"
            )
            && proof_gate.contains("regions=4 poison=1 generation=1 atomic-failures=4")
            && proof_gate.contains(
                "public-operation kernel proof passed states=9 edges=16 transitions=144",
            )
            && proof_gate.contains("export_production_trace_for_lean")
            && proof_gate.contains("export_runtime_certificates_for_lean")
            && proof_gate.contains("export_public_operation_kernel_for_lean")
            && proof_gate.contains("\"${PUBLIC_OPERATION_GENERATED}\"")
            && proof_gate.contains("[[ ! -s \"${GENERATED}\" ]]")
            && proof_gate.contains("[[ ! -s \"${RUNTIME_GENERATED}\" ]]")
            && proof_gate.contains("[[ ! -s \"${PUBLIC_OPERATION_GENERATED}\" ]]")
            && final_gate.contains("bash ./.github/scripts/check_lean_proofs.sh")
            && final_gate.contains("bash ./.github/scripts/check_unix_carrier_proof.sh")
            && workflow.contains("ELAN_VERSION=\"4.2.3\"")
            && workflow.contains("elan-aarch64-unknown-linux-gnu.tar.gz")
            && workflow
                .contains("cb69af0803b04157bc30201c29c12fca882bb3ad8b43476b8d2d3064810bc3ac")
            && workflow.contains("sha256sum --check -")
            && workflow.contains("leanprover/elan/releases/download/v${ELAN_VERSION}"),
        "final-form CI must audit every Lean theorem and execute pinned nonempty artifacts"
    );
}
