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

#[kani::proof]
#[kani::unwind(8)]
fn roll_reentry_rejects_a_later_equal_outbound_contract_after_inbound() {
    use super::validate_roll_reentry_endpoint_selectors;
    use crate::global::const_dsl::{EffList, ScopeId, ScopeKind};
    let first_label: u8 = kani::any();
    let next_label: u8 = kani::any();
    let first_schema: u32 = kani::any();
    let next_schema: u32 = kani::any();
    let mut source = EffList::<5>::new_partitioned(3, 2, 0);
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: first_label,
        payload_schema: first_schema,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 1,
        to: 0,
        label: 24,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: next_label,
        payload_schema: next_schema,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_roll_scope_mut(ScopeId::new(ScopeKind::Roll, 0), 0, 1);
    assert_eq!(
        validate_roll_reentry_endpoint_selectors(&source),
        first_label != next_label || first_schema != next_schema
    );
}

#[kani::proof]
#[kani::unwind(8)]
fn receive_guarded_reentry_keeps_repeated_outbound_replies_projectable() {
    use super::validate_roll_reentry_endpoint_selectors;
    use crate::global::const_dsl::{EffList, ScopeId, ScopeKind};
    let reply_label: u8 = kani::any();
    let schema: u32 = kani::any();
    let mut source = EffList::<6>::new_partitioned(4, 2, 0);
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: 10,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 1,
        to: 0,
        label: reply_label,
        payload_schema: schema,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: 20,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 1,
        to: 0,
        label: reply_label,
        payload_schema: schema,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_roll_scope_mut(ScopeId::new(ScopeKind::Roll, 0), 0, 2);
    assert!(validate_roll_reentry_endpoint_selectors(&source));
}

#[kani::proof]
#[kani::unwind(8)]
fn role_erased_prefixes_cannot_hide_competing_roll_heads() {
    use super::validate_roll_reentry_endpoint_selectors;
    use crate::global::const_dsl::{EffList, ScopeId, ScopeKind};
    let first_label: u8 = kani::any();
    let next_label: u8 = kani::any();
    let first_schema: u32 = kani::any();
    let next_schema: u32 = kani::any();
    let mut source = EffList::<6>::new_partitioned(4, 2, 0);
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: 10,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 2,
        to: 3,
        label: first_label,
        payload_schema: first_schema,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: 20,
        payload_schema: 0,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 2,
        to: 3,
        label: next_label,
        payload_schema: next_schema,
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_roll_scope_mut(ScopeId::new(ScopeKind::Roll, 0), 0, 2);
    assert_eq!(
        validate_roll_reentry_endpoint_selectors(&source),
        first_label != next_label || first_schema != next_schema
    );
}

#[kani::proof]
#[kani::unwind(8)]
fn branch_ancestry_distinguishes_parallel_work_from_exclusive_choices() {
    use super::branch_paths::BranchPaths;
    use crate::global::const_dsl::{EffList, ReentryMark, ScopeId, ScopeKind};
    let mut source = EffList::<11>::new_partitioned(4, 7, 0);
    let mut label = 0;
    while label < 4 {
        source.push_event_mut(EffAtom {
            from: 0,
            to: 1,
            label,
            payload_schema: 0,
            origin: EventOrigin::User,
            lane: 0,
        });
        label += 1;
    }
    source.push_parallel_scope_mut(ScopeId::new(ScopeKind::Parallel, 1), 0, 1, 2);
    source.push_route_scope_mut(
        ScopeId::new(ScopeKind::Route, 0),
        0,
        2,
        4,
        ReentryMark::SinglePass,
    );
    let paths = BranchPaths::<11>::new(source.scope_markers());
    let left = (kani::any::<u8>() % 4) as usize;
    let right = (kani::any::<u8>() % 4) as usize;
    let expected = if (left < 2) != (right < 2) {
        Some(ScopeKind::Route)
    } else if left < 2 && left != right {
        Some(ScopeKind::Parallel)
    } else {
        None
    };
    assert_eq!(paths.separation(left, right), expected);
}
