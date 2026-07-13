use super::{PublicActiveOp, PublicOpLease};

fn symbolic_operation(raw: u8) -> PublicActiveOp {
    match raw % 9 {
        0 => PublicActiveOp::Idle,
        1 => PublicActiveOp::Poisoned,
        2 => PublicActiveOp::Send,
        3 => PublicActiveOp::Recv,
        4 => PublicActiveOp::Offer,
        5 => PublicActiveOp::RouteBranch,
        6 => PublicActiveOp::RestoredRouteBranch,
        7 => PublicActiveOp::BranchRecv,
        _ => PublicActiveOp::BranchSend,
    }
}

#[kani::proof]
fn public_operation_transition_classifier_is_exact() {
    let current = symbolic_operation(kani::any());
    let expected = symbolic_operation(kani::any());
    let lease = current.transition_lease(expected);

    if current == PublicActiveOp::Poisoned {
        assert_eq!(lease, PublicOpLease::Faulted);
    } else if current == expected {
        assert_eq!(lease, PublicOpLease::Held);
    } else {
        assert_eq!(lease, PublicOpLease::Rejected);
    }
}
