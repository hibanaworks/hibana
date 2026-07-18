use super::{PublicActiveOp, PublicOpEdge, PublicOpLease};

fn symbolic_operation(raw: u8) -> PublicActiveOp {
    PublicActiveOp::ALL[usize::from(raw) % PublicActiveOp::ALL.len()]
}

fn symbolic_edge(raw: u8) -> PublicOpEdge {
    PublicOpEdge::ALL[usize::from(raw) % PublicOpEdge::ALL.len()]
}

#[kani::proof]
fn public_operation_transition_classifier_is_exact() {
    let current = symbolic_operation(kani::any());
    let expected = symbolic_operation(kani::any());
    let edge = symbolic_edge(kani::any());
    let transition = current.transition(edge);

    assert_eq!(
        current.clear_if_current(expected),
        if current == expected {
            PublicActiveOp::Idle
        } else {
            current
        }
    );
    assert_eq!(current.clear_terminal(), PublicActiveOp::Idle);
    assert_eq!(current.fault(), PublicActiveOp::Poisoned);

    if current == PublicActiveOp::Poisoned {
        assert_eq!(transition.lease(), PublicOpLease::Faulted);
        assert_eq!(transition.phase(), PublicActiveOp::Poisoned);
    } else if current == edge.expected() {
        assert_eq!(transition.lease(), PublicOpLease::Held);
        assert_eq!(transition.phase(), edge.next());
    } else {
        assert_eq!(transition.lease(), PublicOpLease::Rejected);
        assert_eq!(transition.phase(), PublicActiveOp::Poisoned);
    }
}
