use super::{PublicActiveOp, PublicOpLease};
use std::{format, fs, path::PathBuf, string::String, vec::Vec};

const STATES: [PublicActiveOp; 9] = [
    PublicActiveOp::Idle,
    PublicActiveOp::Poisoned,
    PublicActiveOp::Send,
    PublicActiveOp::Recv,
    PublicActiveOp::Offer,
    PublicActiveOp::RouteBranch,
    PublicActiveOp::RestoredRouteBranch,
    PublicActiveOp::BranchRecv,
    PublicActiveOp::BranchSend,
];

#[test]
fn public_operation_transition_classifier_covers_exact_state_product() {
    for current in STATES {
        for expected in STATES {
            let actual = current.transition_lease(expected);
            let exact = if current == PublicActiveOp::Poisoned {
                PublicOpLease::Faulted
            } else if current == expected {
                PublicOpLease::Held
            } else {
                PublicOpLease::Rejected
            };
            assert_eq!(actual, exact);
        }
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

#[test]
#[ignore = "host-only Lean public-operation kernel export"]
fn export_public_operation_kernel_for_lean() {
    let mut rows = Vec::<String>::new();
    for current in STATES {
        for expected in STATES {
            let actual = current.transition_lease(expected);
            rows.push(format!(
                "  -- current={} expected={}\n  {}",
                lean_phase(current),
                lean_phase(expected),
                lean_lease(actual),
            ));
        }
    }
    let generated = format!(
        "import Hibana.MainTheorems\n\n\
         def generatedPublicOperationTable : List Hibana.PublicOperationLease := [\n{}\n]\n\n\
         theorem generatedPublicOperationTableAccepted :\n  \
         Hibana.checkPublicOperationTable generatedPublicOperationTable = true := by\n  decide\n\n\
         theorem generatedPublicOperationTableExact :\n  \
         Hibana.PublicOperationTableExact generatedPublicOperationTable :=\n  \
         Hibana.public_operation_table_certificate_sound\n    \
         generatedPublicOperationTableAccepted\n\n\
         #eval IO.println \"hibana Lean public-operation kernel proof passed states=9 transitions=81\"\n",
        rows.join(",\n"),
    );
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    fs::create_dir_all(&output_dir)
        .expect("create generated Lean public-operation artifact directory");
    fs::write(output_dir.join("PublicOperationGenerated.lean"), generated)
        .expect("write generated Lean public-operation artifact");
}
