use super::{PublicActiveOp, PublicOpEdge, PublicOpLease};
use std::{format, fs, path::PathBuf, string::String, vec::Vec};

#[test]
fn public_operation_transition_classifier_covers_exact_state_product() {
    for &current in PublicActiveOp::ALL {
        for &edge in PublicOpEdge::ALL {
            let actual = current.transition(edge);
            let exact = if current == PublicActiveOp::Poisoned {
                (PublicOpLease::Faulted, PublicActiveOp::Poisoned)
            } else if current == edge.expected() {
                (PublicOpLease::Held, edge.next())
            } else {
                (PublicOpLease::Rejected, PublicActiveOp::Poisoned)
            };
            assert_eq!((actual.lease(), actual.phase()), exact);
        }
        for &expected in PublicActiveOp::ALL {
            assert_eq!(
                current.clear_if_current(expected),
                if current == expected {
                    PublicActiveOp::Idle
                } else {
                    current
                }
            );
        }
        assert_eq!(current.clear_terminal(), PublicActiveOp::Idle);
        assert_eq!(current.fault(), PublicActiveOp::Poisoned);
    }
}

fn lean_edge(edge: PublicOpEdge) -> &'static str {
    match edge {
        PublicOpEdge::BeginOffer => ".beginOffer",
        PublicOpEdge::ResumeOffer => ".resumeOffer",
        PublicOpEdge::PublishRouteBranch => ".publishRouteBranch",
        PublicOpEdge::FinishOffer => ".finishOffer",
        PublicOpEdge::ParkOffer => ".parkOffer",
        PublicOpEdge::ParkRouteBranch => ".parkRouteBranch",
        PublicOpEdge::BeginSend => ".beginSend",
        PublicOpEdge::BeginBranchSend => ".beginBranchSend",
        PublicOpEdge::FinishSend => ".finishSend",
        PublicOpEdge::FinishBranchSend => ".finishBranchSend",
        PublicOpEdge::ParkBranchSend => ".parkBranchSend",
        PublicOpEdge::BeginRecv => ".beginRecv",
        PublicOpEdge::BeginBranchRecv => ".beginBranchRecv",
        PublicOpEdge::FinishRecv => ".finishRecv",
        PublicOpEdge::FinishBranchRecv => ".finishBranchRecv",
        PublicOpEdge::ParkBranchRecv => ".parkBranchRecv",
    }
}

fn lean_phase(phase: PublicActiveOp) -> &'static str {
    match phase {
        PublicActiveOp::Idle => ".idle",
        PublicActiveOp::Poisoned => ".poisoned",
        PublicActiveOp::Send => ".send",
        PublicActiveOp::Recv => ".recv",
        PublicActiveOp::Offer => ".offer",
        PublicActiveOp::RouteBranch => ".routeBranch",
        PublicActiveOp::RestoredRouteBranch => ".restoredRouteBranch",
        PublicActiveOp::BranchRecv => ".branchRecv",
        PublicActiveOp::BranchSend => ".branchSend",
    }
}

fn lean_lease(lease: PublicOpLease) -> &'static str {
    match lease {
        PublicOpLease::Rejected => ".rejected",
        PublicOpLease::Held => ".held",
        PublicOpLease::Faulted => ".faulted",
    }
}

fn lean_transition(current: PublicActiveOp, edge: PublicOpEdge) -> String {
    let transition = current.transition(edge);
    format!(
        "{{ lease := {}, phase := {} }}",
        lean_lease(transition.lease()),
        lean_phase(transition.phase()),
    )
}

#[test]
#[ignore = "host-only Lean public-operation kernel export"]
fn export_public_operation_kernel_for_lean() {
    let mut rows = Vec::<String>::new();
    for &current in PublicActiveOp::ALL {
        for &edge in PublicOpEdge::ALL {
            rows.push(format!(
                "  -- current={} edge={}\n  {}",
                lean_phase(current),
                lean_edge(edge),
                lean_transition(current, edge),
            ));
        }
    }
    let generated = format!(
        "import Hibana.MainTheorems\n\n\
         def generatedPublicOperationTable : List Hibana.PublicOperationTransition := [\n{}\n]\n\n\
         set_option maxRecDepth 4096 in\n\
         theorem generatedPublicOperationTableAccepted :\n  \
         Hibana.checkPublicOperationTable generatedPublicOperationTable = true := by\n  decide\n\n\
         theorem generatedPublicOperationTableExact :\n  \
         Hibana.PublicOperationTableExact generatedPublicOperationTable :=\n  \
         Hibana.public_operation_table_certificate_sound\n    \
         generatedPublicOperationTableAccepted\n\n\
         #eval IO.println \"hibana Lean public-operation kernel proof passed states=9 edges=16 transitions=144\"\n",
        rows.join(",\n"),
    );
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    fs::create_dir_all(&output_dir)
        .expect("create generated Lean public-operation artifact directory");
    fs::write(output_dir.join("PublicOperationGenerated.lean"), generated)
        .expect("write generated Lean public-operation artifact");
}
