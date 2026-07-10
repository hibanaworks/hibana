use super::common::read;

#[test]
fn lean_proof_gate_is_pinned_fail_closed_and_runtime_free() {
    let manifest = read("Cargo.toml");
    let lakefile = read("proofs/lean/lakefile.toml");
    let lean_toolchain = read("proofs/lean/lean-toolchain");
    let syntax = read("proofs/lean/Hibana/Syntax.lean");
    let event_graph = read("proofs/lean/Hibana/EventGraph.lean");
    let commit = read("proofs/lean/Hibana/Commit.lean");
    let main_theorems = read("proofs/lean/Hibana/MainTheorems.lean");
    let axiom_audit = read("proofs/lean/Hibana/AxiomAudit.lean");
    let exporter = read("src/test_support/lean_proof_export.rs");
    let proof_gate = read(".github/scripts/check_lean_proofs.sh");
    let final_gate = read(".github/scripts/run_final_form_gates.sh");
    let workflow = read(".github/workflows/quality-gates.yml");

    assert_eq!(lean_toolchain.trim(), "leanprover/lean4:v4.30.0");
    assert!(
        lakefile.contains("name = \"Hibana\"")
            && !lakefile.contains("[[require]]")
            && !lakefile.contains("git =")
            && !manifest.contains("proofs/lean"),
        "Lean proofs must remain a Core/Std-only host package outside Cargo and Pico artifacts"
    );
    assert!(
        syntax.contains("inductive RouteArm")
            && syntax.contains("inductive RouteAuthority")
            && syntax.contains("inductive ReentryMode")
            && syntax.contains("| dynamic (resolver : Nat)")
            && syntax.contains("| route (authority : RouteAuthority)")
            && syntax.contains("| roll")
            && exporter.contains("LeanChoreo for g::Roll")
            && exporter.contains("for g::Resolve<g::Route<Left, Right>, RESOLVER_ID>")
            && exporter.contains("Hibana.Choreo.route (.dynamic {RESOLVER_ID})")
            && exporter.contains("generatedRolledChoreo")
            && exporter.contains("&[41, 43, 41, 43]")
            && exporter.contains("&[42, 43, 42, 43]")
            && exporter.contains("generatedRolledLeftTraceRole1")
            && exporter.contains("generatedNestedRolledChoreo")
            && exporter.contains("&[71, 72, 73, 71, 72, 73]")
            && exporter.contains("generatedResolvedLeftTraceRole0")
            && exporter.contains("generatedResolvedRightTraceRole0")
            && exporter.contains("generatedNestedResolvedTraceRole0")
            && exporter.contains("generatedRolledResolvedTraceRole0")
            && exporter.contains("generatedRejectedTraceRole0")
            && exporter.contains("resolver rejection must terminate the production proof trace")
            && exporter.contains("let artifact_count = proof_sources.len();")
            && exporter.contains("passed artifacts={} frames={}")
            && !exporter.contains("passed artifacts=13 frames={}")
            && exporter.contains("env!(\"CARGO_MANIFEST_DIR\")")
            && !exporter.contains("std::env")
            && event_graph.contains("reentry : ReentryMode")
            && !event_graph.contains("insideRoll : Bool")
            && !event_graph.contains("reentrant : Bool")
            && !syntax.contains("| resolve"),
        "route authority must stay canonical while roll and dynamic resolve retain production witnesses"
    );
    assert!(
        commit.contains("def ValidTrace")
            && !commit.contains("TraceCommitsEnabled")
            && commit.contains("eventCandidateBaseState?")
            && commit.contains("eventBaseState?")
            && commit.contains("eventCommitReady")
            && commit.contains("def applyResolver")
            && commit.contains("| resolve (conflict resolver : Nat) (arm : RouteArm)")
            && commit.contains("| reject (conflict resolver : Nat)")
            && !commit.contains(".getD")
            && exporter.contains("Hibana.ValidTrace"),
        "trace soundness must separate candidate frontiers from explicit commit and resolver authority"
    );
    for theorem in [
        "theorem artifact_checker_sound",
        "theorem trace_checker_sound",
        "theorem commit_marks_event_done",
        "theorem selected_arm_unique",
        "theorem reset_roll_preserves_outside_done",
        "theorem reset_roll_clears_done",
        "theorem reset_roll_preserves_outside_selection",
        "theorem reset_roll_clears_selection",
        "theorem reset_route_arm_preserves_selection",
        "theorem reset_route_arm_preserves_outside_done",
        "theorem reset_route_arm_clears_done",
        "theorem local_action_send_recv_duality",
        "theorem local_action_uninvolved_projects_none",
        "theorem local_action_self_send_projects_local",
        "theorem local_action_preserves_label",
        "theorem commit_exact_successor",
        "theorem resolver_reject_never_selects",
        "theorem resolver_select_never_rejects",
        "theorem resolver_selection_exact_authority",
        "theorem selected_route_is_not_resolvable",
        "theorem unresolved_dynamic_route_cannot_commit",
        "theorem unresolved_dynamic_commit_rejected",
        "theorem wrong_resolver_id_rejected",
        "theorem resolver_reuse_without_reset_rejected",
        "theorem resolver_reject_is_terminal",
        "theorem program_trace_checker_sound",
    ] {
        assert!(
            syntax.contains(theorem)
                || event_graph.contains(theorem)
                || commit.contains(theorem)
                || main_theorems.contains(theorem),
            "Lean proof kernel missing {theorem}"
        );
    }
    assert!(
        proof_gate.contains("forbids sorry, admit, and custom axioms")
            && proof_gate.contains("must remain Core/Std-only")
            && proof_gate.contains("Lean proof gate axiom set changed")
            && proof_gate.contains("depends on axioms: [propext, Quot.sound]")
            && proof_gate.contains("Classical.choice")
            && axiom_audit.contains("#print axioms Hibana.program_trace_checker_sound")
            && axiom_audit.contains("#print axioms Hibana.resolver_selection_exact_authority")
            && axiom_audit.contains("#print axioms Hibana.unresolved_dynamic_route_cannot_commit")
            && axiom_audit.contains("#print axioms Hibana.resolver_reject_is_terminal")
            && commit.contains("rollReentryState?")
            && commit.contains("routeReentryState?")
            && proof_gate.contains("artifacts=13 frames=55")
            && proof_gate.contains("export_production_trace_for_lean")
            && final_gate.contains("bash ./.github/scripts/check_lean_proofs.sh")
            && workflow.contains("ELAN_VERSION=\"4.2.3\"")
            && workflow.contains("elan-aarch64-unknown-linux-gnu.tar.gz")
            && workflow
                .contains("cb69af0803b04157bc30201c29c12fca882bb3ad8b43476b8d2d3064810bc3ac")
            && workflow.contains("sha256sum --check -")
            && workflow.contains("leanprover/elan/releases/download/v${ELAN_VERSION}"),
        "final-form CI must execute the pinned nonempty Lean proof and production-trace gate"
    );
}
