use super::common::read;

#[test]
fn lean_ci_gate_audits_every_exported_theorem_and_runs_pinned_artifacts() {
    let proof_gate = read(".github/scripts/check_lean_proofs.sh");
    let theorem_inventory = read(".github/scripts/check_lean_theorem_inventory.py");
    let generated_axiom_audit = read(".github/scripts/check_generated_lean_axioms.py");
    let claim_gate = read(".github/scripts/check_lean_claim_surface.sh");
    let claim_source = read("proofs/lean/ClaimSurface.lean");
    let claim_snapshot = read("proofs/lean/claim-surface.txt");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let workflow = read(".github/workflows/quality-gates.yml");

    assert!(
        proof_gate.contains("forbids sorry, admit, and custom axioms")
            && proof_gate.contains("must remain Core/Std-only")
            && proof_gate.contains("check_lean_theorem_inventory.py")
            && proof_gate.contains("check_lean_theorem_inventory.py\" --self-test")
            && proof_gate.contains("check_generated_lean_axioms.py")
            && proof_gate.contains("check_generated_lean_axioms.py\" --self-test")
            && proof_gate.contains("check_lean_claim_surface.sh")
            && proof_gate.contains("AxiomAudit.lean")
            && proof_gate.contains("EXPECTED_NATIVE_REGRESSION_COUNT=32")
            && proof_gate.contains("confines native regression execution to example modules")
            && proof_gate.contains("must not import native regression examples")
            && proof_gate.contains(
                "Native regression modules may contain anonymous examples, not named claims"
            )
            && proof_gate.contains("native-regressions=32")
            && theorem_inventory.contains("def erase_non_code(source: str)")
            && theorem_inventory.contains("def theorem_names(source: str)")
            && theorem_inventory.contains("if previous is not None")
            && theorem_inventory.contains("previous.group(1) == \"private\"")
            && theorem_inventory.contains("escaped theorem names are not inventory-safe")
            && theorem_inventory.contains("def self_test()")
            && theorem_inventory.contains("unicode_一意?")
            && theorem_inventory.contains("punctuation!'")
            && generated_axiom_audit.contains("NATIVE_DECISION_COUNT = 16")
            && generated_axiom_audit.contains("sys.dont_write_bytecode = True")
            && generated_axiom_audit.contains("EXACT_CERTIFICATE_COUNT = 22")
            && generated_axiom_audit.contains("PROJECTABILITY_CERTIFICATE_COUNT = 8")
            && generated_axiom_audit.contains("VERIFIED_PROTOCOL_CERTIFICATE_COUNT = 8")
            && generated_axiom_audit.contains("def validate_axioms(")
            && generated_axiom_audit.contains("NATIVE_AXIOM_OWNER")
            && generated_axiom_audit.contains("kernel-checked generated theorem")
            && generated_axiom_audit.contains("gained forbidden axioms")
            && generated_axiom_audit.contains("has the wrong closure dependencies")
            && generated_axiom_audit.contains("EXACT_CERTIFICATE_THEOREMS")
            && generated_axiom_audit.contains("PROJECTABILITY_CERTIFICATE_THEOREMS")
            && generated_axiom_audit.contains("VERIFIED_PROTOCOL_CERTIFICATE_THEOREMS")
            && claim_gate.contains("EXPECTED_CLAIMS=15")
            && claim_gate.contains("diff -u \"${SNAPSHOT}\" \"${actual}\"")
            && claim_source.matches("#check @Hibana.").count() == 15
            && claim_snapshot.contains(
                "@Hibana.assumption_indexed_epoch_erased_byte_exact_end_to_end_refinement"
            )
            && claim_snapshot
                .contains("@Hibana.assumption_indexed_static_cross_tool_production_refinement")
            && proof_gate.contains("Lean proof gate axiom set changed")
            && proof_gate.contains("axiom_both_count=\"$(awk")
            && proof_gate.contains("count += 1")
            && proof_gate.contains("END { print count + 0 }")
            && proof_gate.contains("Classical.choice")
            && proof_gate.contains("native_decide.ax")
            && proof_gate.contains("readonly EXPECTED_AXIOM_BOTH_COUNT=")
            && proof_gate.contains("readonly EXPECTED_AXIOM_PROPEXT_COUNT=")
            && proof_gate.contains("readonly EXPECTED_AXIOM_FREE_COUNT=")
            && proof_gate.contains("readonly EXPECTED_EXPORTED_THEOREM_COUNT=")
            && proof_gate.contains("!= \"${EXPECTED_AXIOM_BOTH_COUNT}\"")
            && proof_gate.contains("!= \"${EXPECTED_AXIOM_PROPEXT_COUNT}\"")
            && proof_gate.contains("!= \"${EXPECTED_AXIOM_FREE_COUNT}\"")
            && proof_gate.contains("!= \"${EXPECTED_EXPORTED_THEOREM_COUNT}\"")
            && proof_gate.contains(
                "traces=14 frames=66 projections=22 exact-descriptors=22 progress=4 projectability=8 distributed-progress=8 verified-protocols=8"
            )
            && proof_gate.contains(
                "production artifact passed transitions=7 operations=6 owners=8 kernel-refinement=external-premise owner-evidence=external-premise codecs=3 family=8 deployments=8 deployment-rejections=3 capabilities=6 agreement=static-exact-family profile=closing"
            )
            && proof_gate.contains("EXPECTED_PRODUCTION_MARKER")
            && proof_gate.contains("EXPECTED_GENERATED_AXIOM_MARKER")
            && proof_gate.contains("theorems=48 kernel=27 native=21 native-decisions=16")
            && proof_gate.contains(
                "production-transitions=7 production-operations=6 production-owners=8 verified-codecs=3 verified-family=8 static-deployments=8 deployment-rejections=3 capabilities=6"
            )
            && proof_gate.contains("regions=4 poison=1 generation=1 atomic-failures=4")
            && proof_gate.contains("public-operation kernel proof passed states=9 transitions=81")
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
