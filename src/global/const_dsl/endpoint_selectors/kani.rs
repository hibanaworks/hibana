use super::{EndpointSelector, ObserverPathDecision, observer_path_decision};
use crate::eff::{EffAtom, EventOrigin};

fn symbolic_atom() -> EffAtom {
    EffAtom {
        from: kani::any(),
        to: kani::any(),
        label: kani::any(),
        payload_schema: kani::any(),
        origin: EventOrigin::User,
        lane: kani::any(),
    }
}

fn outbound_selector(atom: EffAtom) -> EndpointSelector {
    EndpointSelector::outbound(atom)
}

#[kani::proof]
fn outbound_selector_identity_is_exact_public_send_contract() {
    let left = symbolic_atom();
    let right = symbolic_atom();
    let left_selector = outbound_selector(left);
    let right_selector = outbound_selector(right);

    assert!(
        left_selector.same(right_selector)
            == (left.from == right.from
                && left.label == right.label
                && left.payload_schema == right.payload_schema)
    );
    kani::cover!(left.to != right.to && left_selector.same(right_selector));
    kani::cover!(left.lane != right.lane && left_selector.same(right_selector));
}

#[kani::proof]
fn inbound_selector_identity_is_exact_compact_event_identity() {
    let left: u16 = kani::any();
    let right: u16 = kani::any();
    let left_selector = EndpointSelector::inbound_evidence(left as usize);
    let right_selector = EndpointSelector::inbound_evidence(right as usize);

    assert!(left_selector.is_some() == (left != u16::MAX));
    assert!(right_selector.is_some() == (right != u16::MAX));
    if let (Some(left_selector), Some(right_selector)) = (left_selector, right_selector) {
        assert!(left_selector.same(right_selector) == (left == right));
    }
    assert!(
        EndpointSelector::inbound_evidence(crate::eff::meta::COMPACT_EVENT_IDENTITY_CAPACITY)
            .is_none()
    );
}

#[kani::proof]
fn observer_path_decision_has_exact_merge_domain() {
    let left = kani::any::<bool>().then_some(EndpointSelector(kani::any()));
    let right = kani::any::<bool>().then_some(EndpointSelector(kani::any()));
    let decision = observer_path_decision(left, right);

    match (left, right) {
        (None, None) => assert!(decision == ObserverPathDecision::Accept),
        (Some(_), None) | (None, Some(_)) => {
            assert!(decision == ObserverPathDecision::Reject);
        }
        (Some(left), Some(right))
            if left.is_inbound_evidence() && right.is_inbound_evidence() && left.same(right) =>
        {
            assert!(decision == ObserverPathDecision::Continue);
        }
        (Some(left), Some(right)) if left.is_inbound_evidence() && right.is_inbound_evidence() => {
            assert!(decision == ObserverPathDecision::Accept);
        }
        (Some(_), Some(_)) => {
            assert!(decision == ObserverPathDecision::Reject);
        }
    }
}
