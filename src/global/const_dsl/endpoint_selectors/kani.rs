use super::{EndpointSelector, ObserverPathDecision, observer_path_decision};
use crate::eff::{EffAtom, EventOrigin};

#[kani::proof]
fn endpoint_selector_never_confuses_an_owned_endpoint_with_any_wire_schema() {
    let mut wire = symbolic_atom();
    let wire_selector = EndpointSelector::outbound(wire);
    wire.payload = crate::eff::EventPayload::Endpoint(kani::any());
    let owned_selector = EndpointSelector::outbound(wire);
    assert!(wire_selector.is_outbound());
    assert!(owned_selector.is_outbound());
    assert!(!wire_selector.same(owned_selector));
}

fn symbolic_atom() -> EffAtom {
    EffAtom {
        from: kani::any(),
        to: kani::any(),
        label: kani::any(),
        payload: crate::eff::EventPayload::Wire(kani::any()),
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
                && left.payload == right.payload)
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
    use super::roll_reentry_endpoint_conflict;
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
        payload: crate::eff::EventPayload::Wire(first_schema),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 1,
        to: 0,
        label: 24,
        payload: crate::eff::EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: next_label,
        payload: crate::eff::EventPayload::Wire(next_schema),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_roll_scope_mut(ScopeId::new(ScopeKind::Roll, 0), 0, 1);
    assert_eq!(
        roll_reentry_endpoint_conflict(&source, None).is_none(),
        first_label != next_label || first_schema != next_schema
    );
}

#[kani::proof]
#[kani::unwind(8)]
fn receive_guarded_reentry_keeps_repeated_outbound_replies_projectable() {
    use super::roll_reentry_endpoint_conflict;
    use crate::global::const_dsl::{EffList, ScopeId, ScopeKind};
    let reply_label: u8 = kani::any();
    let schema: u32 = kani::any();
    let mut source = EffList::<6>::new_partitioned(4, 2, 0);
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: 10,
        payload: crate::eff::EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 1,
        to: 0,
        label: reply_label,
        payload: crate::eff::EventPayload::Wire(schema),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: 20,
        payload: crate::eff::EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 1,
        to: 0,
        label: reply_label,
        payload: crate::eff::EventPayload::Wire(schema),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_roll_scope_mut(ScopeId::new(ScopeKind::Roll, 0), 0, 2);
    assert!(roll_reentry_endpoint_conflict(&source, None).is_none());
}

#[kani::proof]
#[kani::unwind(8)]
fn role_erased_prefixes_cannot_hide_competing_roll_heads() {
    use super::roll_reentry_endpoint_conflict;
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
        payload: crate::eff::EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 2,
        to: 3,
        label: first_label,
        payload: crate::eff::EventPayload::Wire(first_schema),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 0,
        to: 1,
        label: 20,
        payload: crate::eff::EventPayload::Wire(0),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_event_mut(EffAtom {
        from: 2,
        to: 3,
        label: next_label,
        payload: crate::eff::EventPayload::Wire(next_schema),
        origin: EventOrigin::User,
        lane: 0,
    });
    source.push_roll_scope_mut(ScopeId::new(ScopeKind::Roll, 0), 0, 2);
    assert_eq!(
        roll_reentry_endpoint_conflict(&source, None).is_none(),
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
            payload: crate::eff::EventPayload::Wire(0),
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

#[kani::proof]
#[kani::unwind(5)]
fn roll_head_uses_the_actor_of_each_view_for_every_actor() {
    use super::{branch_paths::BranchPaths, outbound_roll_head};
    use crate::{g::SourceActorImage, global::const_dsl::EffList};
    let sender: u8 = kani::any();
    let receiver: u8 = kani::any();
    let holder: u8 = kani::any();
    let mut source = EffList::<2>::new_partitioned(2, 0, 0);
    let mut event = 0;
    while event < 2 {
        source.push_event_mut(EffAtom {
            from: event * 2,
            to: event * 2 + 1,
            label: event,
            payload: crate::eff::EventPayload::Wire(0),
            origin: EventOrigin::User,
            lane: 0,
        });
        event += 1;
    }
    let image = SourceActorImage::<2, 0>::causal_fixture(
        &[[sender as u16, receiver as u16], [holder as u16, 4]],
        &[],
    );
    let paths = BranchPaths::<2>::new(source.scope_markers());
    assert_eq!(
        outbound_roll_head(&source, Some(image.event_actors()), &paths, 0, 1),
        holder != sender && holder != receiver,
    );
    assert!(outbound_roll_head(&source, None, &paths, 0, 1));
}

#[kani::proof]
#[kani::unwind(10)]
fn a_choice_prefix_guards_the_continuation_exactly_when_both_arms_do() {
    use super::branch_paths::BranchPaths;
    use crate::global::const_dsl::{EffList, ReentryMark, ScopeId, ScopeKind};
    let mut source = EffList::<7>::new_partitioned(3, 4, 0);
    let mut label = 0;
    while label < 3 {
        source.push_event_mut(EffAtom {
            from: 0,
            to: 1,
            label,
            payload: crate::eff::EventPayload::Wire(0),
            origin: EventOrigin::User,
            lane: 0,
        });
        label += 1;
    }
    source.push_route_scope_mut(
        ScopeId::new(ScopeKind::Route, 0),
        0,
        1,
        2,
        ReentryMark::SinglePass,
    );
    let paths = BranchPaths::<7>::new(source.scope_markers());
    let left: bool = kani::any();
    let right: bool = kani::any();
    let mut arms = [0; 7];
    if left {
        paths.record_guard(&mut arms, 0);
    }
    if right {
        paths.record_guard(&mut arms, 1);
    }
    assert_eq!(paths.guards_every_path(&mut arms, 2), left && right);
    assert!(!paths.precedes_on_every_path(0, 2));
    assert!(!paths.precedes_on_every_path(1, 2));
}
