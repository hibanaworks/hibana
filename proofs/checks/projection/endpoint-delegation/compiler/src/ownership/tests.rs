use super::*;

const P: EndpointKey = EndpointKey {
    component: 0,
    role: 0,
};
const A: EndpointKey = EndpointKey {
    component: 0,
    role: 1,
};
const B: EndpointKey = EndpointKey {
    component: 0,
    role: 2,
};
const SERVER: EndpointKey = EndpointKey {
    component: 0,
    role: 3,
};
const OTHER_SERVER: EndpointKey = EndpointKey {
    component: 0,
    role: 4,
};
const DB: EndpointKey = EndpointKey {
    component: 0,
    role: 10,
};
const AUTH: EndpointKey = EndpointKey {
    component: 0,
    role: 11,
};

const fn wire(from: EndpointKey, to: EndpointKey, label: u8) -> Source<'static> {
    Source::Send(Message {
        from,
        to,
        label,
        payload: Payload::Wire { schema: 41 },
    })
}

const fn transfer(
    from: EndpointKey,
    to: EndpointKey,
    label: u8,
    endpoint: EndpointKey,
) -> Source<'static> {
    Source::Send(Message {
        from,
        to,
        label,
        payload: Payload::Endpoint(endpoint),
    })
}

const fn send(event: usize) -> ActionId {
    ActionId {
        event,
        side: Side::Send,
    }
}
const fn recv(event: usize) -> ActionId {
    ActionId {
        event,
        side: Side::Receive,
    }
}

const EXCHANGE: Source<'static> = Source::seq(
    &transfer(P, A, 1, DB),
    &Source::seq(
        &wire(DB, SERVER, 2),
        &Source::seq(
            &wire(SERVER, DB, 3),
            &Source::seq(&wire(SERVER, A, 4), &transfer(A, P, 5, DB)),
        ),
    ),
);
const EXCHANGE_EVENTS: usize = EXCHANGE.event_count();
const EXCHANGE_SYMBOLS: usize = 3 * EXCHANGE_EVENTS;
const EXCHANGE_PLAN: Plan<'static, EXCHANGE_SYMBOLS, EXCHANGE_EVENTS> = {
    let mut scratch = Scratch::new();
    let mut output = Plan::new();
    match compile_into(&EXCHANGE, &mut scratch, &mut output) {
        Ok(()) => output,
        Err(_) => panic!("constant ownership elaboration"),
    }
};

#[test]
fn stable_const_projection_preserves_wire_identity_and_infers_initial_issuance() {
    let plan = &EXCHANGE_PLAN;
    assert_eq!(plan.len(), 5);
    assert_eq!(
        plan.issuance(0),
        Some(Issuance {
            endpoint: DB,
            actor: P.role
        })
    );
    let work = plan.binding(1).expect("resource work");
    assert_eq!(work.sender_actor, A.role);
    assert_eq!(work.message.from, DB);
    assert_eq!(work.message.to, SERVER);
    assert_eq!(work.message.label, 2);
    assert_eq!(work.message.payload, Payload::Wire { schema: 41 });
    assert_eq!(plan.action(send(1)).expect("resource view").endpoint, DB);
    assert_eq!(
        plan.action(recv(2)).expect("resource reply view").actor,
        A.role
    );
}

#[test]
fn lane_actor_membership_is_the_union_of_both_complete_ownership_paths() {
    const SOURCE: Source<'static> = Source::seq(
        &Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), None),
        &wire(DB, SERVER, 3),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    let members = plan.possible_actors(2).unwrap();
    for actor in 0..=u8::MAX {
        assert_eq!(
            members[actor as usize / 8] & (1 << (actor % 8)) != 0,
            actor == A.role || actor == B.role || actor == SERVER.role,
        );
    }
    assert_eq!(plan.possible_actors(3), None);
}

#[test]
fn receiving_an_owned_endpoint_teaches_its_holder_before_resource_sends() {
    const SOURCE: Source<'static> = Source::route(
        &Source::seq(&transfer(P, A, 1, DB), &wire(DB, SERVER, 3)),
        &Source::seq(&transfer(P, A, 2, DB), &wire(DB, SERVER, 4)),
        None,
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert!(plan.has_branch_knowledge());
}

#[test]
fn a_parallel_siblings_receive_does_not_teach_an_independent_resource_send() {
    const SOURCE: Source<'static> = Source::seq(
        &transfer(P, A, 1, DB),
        &Source::route(
            &Source::par(&wire(P, A, 2), &wire(DB, SERVER, 4)),
            &Source::par(&wire(P, A, 3), &wire(DB, SERVER, 5)),
            None,
        ),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert!(!plan.has_branch_knowledge());
}

#[test]
fn a_prior_conditional_holder_can_control_the_next_resource_choice() {
    const SOURCE: Source<'static> = Source::seq(
        &Source::route(
            &Source::seq(&transfer(P, A, 1, DB), &wire(P, B, 2)),
            &Source::seq(&transfer(P, B, 3, DB), &wire(P, A, 4)),
            None,
        ),
        &Source::route(&wire(DB, SERVER, 5), &wire(DB, SERVER, 6), None),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert!(plan.has_branch_knowledge());
}

#[test]
fn candidate_actor_unions_cannot_hide_missing_knowledge_on_an_actual_path() {
    const SOURCE: Source<'static> = Source::seq(
        &Source::route(
            &Source::seq(&transfer(P, A, 1, DB), &transfer(P, B, 2, AUTH)),
            &Source::seq(&transfer(P, B, 3, DB), &transfer(P, A, 4, AUTH)),
            None,
        ),
        &Source::seq(
            &Source::route(&wire(P, DB, 5), &wire(P, AUTH, 6), None),
            &wire(A, SERVER, 7),
        ),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    // Both arms' candidate sets are {P, A, B}. In the prior left path,
    // however, only A learns the next left choice and only B learns right.
    assert_eq!(plan.possible_actors(4), plan.possible_actors(5));
    assert!(!plan.has_branch_knowledge());
}

#[test]
fn recipient_cannot_skip_resource_work_and_wait_on_its_control_endpoint() {
    let plan = &EXCHANGE_PLAN;
    // A's original control facet contains receive(0), receive(3), send(4).
    // The same actor must also execute the DB facet's send(1), receive(2).
    // Omitting these from A's projection permits waiting for SERVER's result
    // before the request producing that result has been sent.
    assert!(plan.precedes(recv(0), send(1)));
    assert!(plan.precedes(send(1), recv(2)));
    assert!(plan.precedes(recv(2), recv(3)));
    assert!(plan.precedes(send(1), recv(3)));
    assert!(plan.precedes(recv(3), send(4)));
    assert!(plan.precedes(recv(2), send(4)));
    assert!(!plan.precedes(recv(3), send(1)));
    assert_ne!(
        plan.action(send(1)).unwrap().endpoint,
        plan.action(recv(3)).unwrap().endpoint
    );
}

#[test]
fn related_components_share_actor_order_without_merging_wire_endpoint_keys() {
    const CROSS_DB: EndpointKey = EndpointKey {
        component: 7,
        role: DB.role,
    };
    const CROSS_SERVER: EndpointKey = EndpointKey {
        component: 7,
        role: SERVER.role,
    };
    const CONTROL_A: EndpointKey = EndpointKey {
        component: 9,
        role: A.role,
    };
    const CONTROL_SERVER: EndpointKey = EndpointKey {
        component: 9,
        role: SERVER.role,
    };
    const SOURCE: Source<'static> = Source::seq(
        &transfer(P, A, 1, CROSS_DB),
        &Source::seq(
            &wire(CROSS_DB, CROSS_SERVER, 2),
            &Source::seq(
                &wire(CONTROL_SERVER, CONTROL_A, 3),
                &transfer(A, P, 4, CROSS_DB),
            ),
        ),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert!(plan.precedes(send(1), recv(2)));
    assert_eq!(
        plan.action(send(1)).unwrap().actor,
        plan.action(recv(2)).unwrap().actor
    );
    assert_ne!(
        plan.action(send(1)).unwrap().endpoint.component,
        plan.action(recv(2)).unwrap().endpoint.component
    );
}

#[test]
fn independent_parallel_loans_and_nested_rolls_preserve_both_flows() {
    const LEFT: Source<'static> = Source::seq(
        &transfer(P, A, 1, DB),
        &Source::seq(&wire(DB, SERVER, 2), &transfer(A, P, 3, DB)),
    );
    const RIGHT: Source<'static> = Source::seq(
        &transfer(P, B, 4, AUTH),
        &Source::seq(&wire(AUTH, OTHER_SERVER, 5), &transfer(B, P, 6, AUTH)),
    );
    const SOURCE: Source<'static> =
        Source::roll(&Source::par(&Source::roll(&LEFT), &Source::roll(&RIGHT)));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert_eq!(plan.binding(1).unwrap().sender_actor, A.role);
    assert_eq!(plan.binding(4).unwrap().sender_actor, B.role);
    assert!(!plan.precedes(send(0), send(3)));
    assert!(!plan.precedes(send(3), send(0)));
    assert!(plan.precedes(recv(0), send(1)));
    assert!(plan.precedes(recv(3), send(4)));
    assert_eq!(
        plan.issuance(0),
        Some(Issuance {
            endpoint: DB,
            actor: P.role
        })
    );
    assert_eq!(
        plan.issuance(1),
        Some(Issuance {
            endpoint: AUTH,
            actor: P.role
        })
    );
}

#[test]
fn stable_owned_endpoint_allows_independent_parallel_operations() {
    const SOURCE: Source<'static> = Source::seq(
        &transfer(P, A, 1, DB),
        &Source::seq(
            &Source::par(&wire(DB, SERVER, 2), &wire(DB, OTHER_SERVER, 3)),
            &transfer(A, P, 4, DB),
        ),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert_eq!(plan.binding(1).unwrap().sender_actor, A.role);
    assert_eq!(plan.binding(2).unwrap().sender_actor, A.role);
    assert!(!plan.precedes(send(1), send(2)));
    assert!(!plan.precedes(send(2), send(1)));
    assert!(plan.precedes(send(1), send(3)));
    assert!(plan.precedes(send(2), send(3)));
}

#[test]
fn competing_parallel_transfers_cannot_both_own_the_same_endpoint() {
    const SOURCE: Source<'static> = Source::par(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    assert_eq!(
        compile_into(&SOURCE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::ParallelOwnershipConflict
    );
    assert!(plan.is_empty());
    assert_eq!(plan.issuance(0), None);
}

#[test]
fn parallel_handoff_cannot_race_an_operation_on_the_moved_endpoint() {
    const SOURCE: Source<'static> = Source::par(&transfer(P, A, 1, DB), &wire(DB, SERVER, 2));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    assert_eq!(
        compile_into(&SOURCE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::ParallelOwnershipConflict
    );
    assert!(plan.is_empty());
}

#[test]
fn closed_resolved_choice_keeps_branch_local_owner_and_common_return() {
    const LEFT: Source<'static> = Source::seq(
        &transfer(P, A, 1, DB),
        &Source::seq(&wire(DB, SERVER, 2), &transfer(A, P, 3, DB)),
    );
    const RIGHT: Source<'static> = Source::seq(
        &transfer(P, B, 4, DB),
        &Source::seq(&wire(DB, SERVER, 5), &transfer(B, P, 6, DB)),
    );
    const SOURCE: Source<'static> = Source::seq(
        &Source::route(&LEFT, &RIGHT, Some(701)),
        &wire(DB, SERVER, 7),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert_eq!(plan.binding(1).unwrap().sender_actor, A.role);
    assert_eq!(plan.binding(4).unwrap().sender_actor, B.role);
    assert_eq!(plan.binding(6).unwrap().sender_actor, P.role);
    assert!(!plan.precedes(send(0), send(3)));
    assert!(!plan.precedes(send(3), send(0)));
}

#[test]
fn terminal_choice_can_leave_the_endpoint_in_different_hands() {
    const SOURCE: Source<'static> =
        Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), None);
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert_eq!(plan.binding(0).unwrap().receiver_actor, A.role);
    assert_eq!(plan.binding(1).unwrap().receiver_actor, B.role);
    assert_eq!(plan.issuance(0).unwrap().actor, P.role);
    assert!(!plan.precedes(recv(0), recv(1)));
    assert!(!plan.precedes(recv(1), recv(0)));
}

#[test]
fn unrelated_continuation_does_not_require_a_redundant_endpoint_return() {
    const CHOICE: Source<'static> =
        Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), Some(701));
    const SOURCE: Source<'static> = Source::seq(&CHOICE, &wire(A, B, 3));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert_eq!(plan.binding(2).unwrap().sender_actor, A.role);
    assert_eq!(plan.binding(2).unwrap().receiver_actor, B.role);
    assert!(plan.precedes(recv(0), send(2)));
    assert!(plan.precedes(recv(1), recv(2)));
}

#[test]
fn live_path_dependent_ownership_follows_the_original_route() {
    const CHOICE: Source<'static> =
        Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), None);
    const SOURCE: Source<'static> = Source::seq(&CHOICE, &wire(DB, SERVER, 3));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    let scope = Scope {
        start: 0,
        split: 1,
        stop: 2,
    };
    let left = [Selection {
        scope,
        arm: Arm::Left,
    }];
    let right = [Selection {
        scope,
        arm: Arm::Right,
    }];
    assert_eq!(plan.binding(2), None);
    assert_eq!(plan.binding_in(2, &left).unwrap().sender_actor, A.role);
    assert_eq!(plan.binding_in(2, &right).unwrap().sender_actor, B.role);
    assert_eq!(plan.binding_in(2, &left).unwrap().message.from, DB);
    assert_eq!(plan.binding_in(2, &right).unwrap().message.from, DB);
    assert_eq!(plan.binding_in(0, &right), None);
    assert_eq!(plan.binding_in(1, &left), None);
    assert!(plan.precedes_in(recv(0), send(2), &left));
    assert!(plan.precedes_in(recv(1), send(2), &right));
}

#[test]
fn later_owned_payload_counts_as_a_live_endpoint_use() {
    const CHOICE: Source<'static> =
        Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), None);
    const SOURCE: Source<'static> = Source::seq(&CHOICE, &transfer(A, P, 3, DB));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    assert_eq!(
        compile_into(&SOURCE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::ConflictingInitialOwners
    );
    assert!(plan.is_empty());
}

#[test]
fn next_roll_visit_keeps_a_terminal_branch_endpoint_live() {
    const CHOICE: Source<'static> =
        Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), None);
    const SOURCE: Source<'static> = Source::roll(&CHOICE);
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    assert_eq!(
        compile_into(&SOURCE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::RollChangesOwner
    );
    assert!(plan.is_empty());
}

#[test]
fn nested_terminal_branch_still_checks_the_enclosing_continuation() {
    const CHOICE: Source<'static> =
        Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), None);
    const INNER: Source<'static> = Source::seq(&wire(P, A, 3), &CHOICE);
    const SOURCE: Source<'static> = Source::seq(&INNER, &wire(SERVER, DB, 4));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    let scope = Scope {
        start: 1,
        split: 2,
        stop: 3,
    };
    assert_eq!(plan.binding(3), None);
    assert_eq!(
        plan.binding_in(
            3,
            &[Selection {
                scope,
                arm: Arm::Left
            }]
        )
        .unwrap()
        .receiver_actor,
        A.role
    );
    assert_eq!(
        plan.binding_in(
            3,
            &[Selection {
                scope,
                arm: Arm::Right
            }]
        )
        .unwrap()
        .receiver_actor,
        B.role
    );
}

#[test]
fn roll_cannot_implicitly_reissue_a_transferred_endpoint_on_the_next_visit() {
    const SOURCE: Source<'static> = Source::roll(&transfer(P, A, 1, DB));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    assert_eq!(
        compile_into(&SOURCE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::RollChangesOwner
    );
    assert!(plan.is_empty());
}

#[test]
fn resource_carrier_uses_its_current_holder_without_changing_its_wire_role() {
    const SOURCE: Source<'static> = Source::seq(
        &transfer(P, B, 1, DB),
        &Source::seq(&transfer(A, B, 2, AUTH), &transfer(AUTH, P, 3, DB)),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert_eq!(plan.binding(2).unwrap().sender_actor, B.role);
    assert_eq!(plan.binding(2).unwrap().message.from, AUTH);
    assert!(plan.precedes(recv(1), send(2)));
    assert!(plan.precedes(recv(0), send(2)));
}

#[test]
fn moving_a_carrier_does_not_silently_move_another_endpoint_held_by_the_old_actor() {
    const SOURCE: Source<'static> = Source::seq(
        &transfer(P, A, 1, DB),
        &Source::seq(&transfer(P, B, 2, AUTH), &transfer(AUTH, P, 3, DB)),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    assert_eq!(
        compile_into(&SOURCE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::ConflictingInitialOwners
    );
}

#[test]
fn unresolved_circular_initial_issuance_is_rejected() {
    const THIRD: EndpointKey = EndpointKey {
        component: 0,
        role: 12,
    };
    const SOURCE: Source<'static> = Source::seq(
        &transfer(DB, AUTH, 1, THIRD),
        &Source::seq(&transfer(AUTH, THIRD, 2, DB), &transfer(THIRD, DB, 3, AUTH)),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    assert_eq!(
        compile_into(&SOURCE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::UnrootedInitialOwner
    );
    assert!(plan.is_empty());
}

#[test]
fn initial_resource_operations_are_assigned_to_the_inferred_issuer() {
    const SOURCE: Source<'static> = Source::seq(
        &wire(DB, SERVER, 1),
        &Source::seq(&transfer(P, A, 2, DB), &wire(DB, SERVER, 3)),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert_eq!(plan.binding(0).unwrap().sender_actor, P.role);
    assert_eq!(plan.binding(2).unwrap().sender_actor, A.role);
}

#[test]
fn failed_recompile_does_not_leave_a_previous_accepted_plan_visible() {
    const INVALID: Source<'static> = Source::par(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB));
    let mut scratch = Scratch::<EXCHANGE_SYMBOLS, EXCHANGE_EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&EXCHANGE, &mut scratch, &mut plan).unwrap();
    assert!(!plan.is_empty());
    assert!(compile_into(&INVALID, &mut scratch, &mut plan).is_err());
    assert!(plan.is_empty());
    assert_eq!(plan.binding(0), None);
    assert_eq!(plan.issuance(0), None);
}

#[test]
fn source_derived_storage_limits_fail_without_partial_publication() {
    let mut scratch = Scratch::<EXCHANGE_SYMBOLS, { EXCHANGE_EVENTS - 1 }>::new();
    let mut plan = Plan::new();
    assert_eq!(
        compile_into(&EXCHANGE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::Capacity
    );
    assert!(plan.is_empty());
    let mut scratch = Scratch::<2, EXCHANGE_EVENTS>::new();
    let mut plan = Plan::new();
    assert_eq!(
        compile_into(&EXCHANGE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::Capacity
    );
    assert!(plan.is_empty());
}

#[test]
fn endpoint_cannot_be_the_same_owned_value_as_its_send_or_receive_carrier() {
    const SOURCES: [Source<'static>; 2] = [transfer(DB, A, 1, DB), transfer(P, DB, 2, DB)];
    for source in &SOURCES {
        let mut scratch = Scratch::<3, 1>::new();
        let mut plan = Plan::new();
        assert_eq!(
            compile_into(source, &mut scratch, &mut plan)
                .unwrap_err()
                .kind,
            ErrorKind::EndpointIsItsOwnCarrier
        );
    }
}

#[test]
fn one_actor_can_own_both_message_facets_without_collapsing_the_payload() {
    const SOURCE: Source<'static> = Source::seq(&transfer(P, A, 1, DB), &wire(DB, A, 2));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert_eq!(plan.binding(1).unwrap().sender_actor, A.role);
    assert_eq!(plan.binding(1).unwrap().receiver_actor, A.role);
    assert!(plan.precedes(send(1), recv(1)));
    assert_ne!(
        plan.action(send(1)).unwrap().endpoint,
        plan.action(recv(1)).unwrap().endpoint
    );
    assert_eq!(
        plan.action(send(1)).unwrap().message.payload,
        Payload::Wire { schema: 41 }
    );
    assert_eq!(
        plan.action(ActionId {
            event: 1,
            side: Side::Local
        }),
        None
    );
}

#[test]
fn correlated_resources_can_be_returned_through_their_current_carrier() {
    const LEFT: Source<'static> = Source::seq(&transfer(P, A, 1, DB), &transfer(P, A, 2, AUTH));
    const RIGHT: Source<'static> = Source::seq(&transfer(P, B, 3, DB), &transfer(P, B, 4, AUTH));
    const SOURCE: Source<'static> = Source::seq(
        &Source::route(&LEFT, &RIGHT, Some(701)),
        &Source::seq(&transfer(AUTH, P, 5, DB), &wire(DB, SERVER, 6)),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    let scope = Scope {
        start: 0,
        split: 2,
        stop: 4,
    };
    assert_eq!(plan.binding(4), None);
    for (arm, actor) in [(Arm::Left, A.role), (Arm::Right, B.role)] {
        let path = [Selection { scope, arm }];
        assert_eq!(plan.binding_in(4, &path).unwrap().sender_actor, actor);
        assert_eq!(plan.binding_in(4, &path).unwrap().message.from, AUTH);
        assert!(plan.precedes_in(recv(4), send(5), &path));
    }
    // An unconditional return overwrites the conditional holder in the source.
    assert_eq!(plan.binding(5).unwrap().sender_actor, P.role);
}

#[test]
fn parallel_choices_keep_independent_resource_holders_in_all_four_paths() {
    const LEFT: Source<'static> =
        Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), None);
    const RIGHT: Source<'static> =
        Source::route(&transfer(P, A, 3, AUTH), &transfer(P, B, 4, AUTH), None);
    const SOURCE: Source<'static> = Source::seq(
        &Source::par(&LEFT, &RIGHT),
        &Source::par(&wire(DB, SERVER, 5), &wire(AUTH, OTHER_SERVER, 6)),
    );
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    for (db_arm, db_actor) in [(Arm::Left, A.role), (Arm::Right, B.role)] {
        for (auth_arm, auth_actor) in [(Arm::Left, A.role), (Arm::Right, B.role)] {
            let path = [
                Selection {
                    scope: Scope {
                        start: 0,
                        split: 1,
                        stop: 2,
                    },
                    arm: db_arm,
                },
                Selection {
                    scope: Scope {
                        start: 2,
                        split: 3,
                        stop: 4,
                    },
                    arm: auth_arm,
                },
            ];
            assert_eq!(plan.binding_in(4, &path).unwrap().sender_actor, db_actor);
            assert_eq!(plan.binding_in(5, &path).unwrap().sender_actor, auth_actor);
            assert!(!plan.precedes_in(send(4), send(5), &path));
            assert!(!plan.precedes_in(send(5), send(4), &path));
        }
    }
}

#[test]
fn a_branch_dependent_carrier_does_not_move_an_unrelated_initial_owner() {
    const CHOICE: Source<'static> =
        Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), None);
    const SOURCE: Source<'static> = Source::seq(&CHOICE, &transfer(DB, P, 3, AUTH));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    // AUTH cannot have two different initial owners depending on that route.
    assert_eq!(
        compile_into(&SOURCE, &mut scratch, &mut plan)
            .unwrap_err()
            .kind,
        ErrorKind::ConflictingInitialOwners
    );
    assert!(plan.is_empty());
}

#[test]
fn inconsistent_or_foreign_selection_records_never_choose_a_holder() {
    const CHOICE: Source<'static> =
        Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), None);
    const SOURCE: Source<'static> = Source::seq(&CHOICE, &wire(DB, SERVER, 3));
    const EVENTS: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * EVENTS }, EVENTS>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    let scope = Scope {
        start: 0,
        split: 1,
        stop: 2,
    };
    let left = Selection {
        scope,
        arm: Arm::Left,
    };
    let right = Selection {
        scope,
        arm: Arm::Right,
    };
    assert_eq!(plan.binding_in(2, &[left, right]), None);
    assert_eq!(plan.binding_in(2, &[left, left]), None);
    assert_eq!(
        plan.binding_in(
            2,
            &[Selection {
                scope: Scope {
                    start: 0,
                    split: 2,
                    stop: 3
                },
                arm: Arm::Left,
            }]
        ),
        None
    );
}

#[test]
fn rolled_work_preserves_the_conditional_holder_chosen_before_the_loop() {
    const LEFT: Source<'static> = Source::seq(&transfer(P, A, 1, DB), &transfer(P, A, 2, AUTH));
    const RIGHT: Source<'static> = Source::seq(&transfer(P, B, 3, DB), &transfer(P, B, 4, AUTH));
    const BODY: Source<'static> = Source::seq(
        &wire(DB, SERVER, 5),
        &Source::seq(&transfer(AUTH, P, 6, DB), &transfer(P, AUTH, 7, DB)),
    );
    const SOURCE: Source<'static> = Source::seq(
        &Source::route(&LEFT, &RIGHT, Some(701)),
        &Source::roll(&BODY),
    );
    const EVENTS: usize = SOURCE.event_count();
    const SYMBOLS: usize = 3 * EVENTS;
    const PLAN: Plan<'static, SYMBOLS, EVENTS> = {
        let mut scratch = Scratch::new();
        let mut output = Plan::new();
        match compile_into(&SOURCE, &mut scratch, &mut output) {
            Ok(()) => output,
            Err(_) => panic!("conditional ownership invariant in a const projected roll"),
        }
    };
    let scope = Scope {
        start: 0,
        split: 2,
        stop: 4,
    };
    for (arm, actor) in [(Arm::Left, A.role), (Arm::Right, B.role)] {
        let path = [Selection { scope, arm }];
        assert_eq!(PLAN.binding_in(4, &path).unwrap().sender_actor, actor);
        assert_eq!(PLAN.binding_in(5, &path).unwrap().sender_actor, actor);
        assert_eq!(PLAN.binding_in(6, &path).unwrap().receiver_actor, actor);
        assert_eq!(PLAN.binding_in(4, &path).unwrap().message.from, DB);
    }
}

#[test]
fn correlated_resources_share_the_same_compiled_actor_decision() {
    const LEFT: Source<'static> = Source::seq(&transfer(P, A, 1, DB), &transfer(P, A, 2, AUTH));
    const RIGHT: Source<'static> = Source::seq(&transfer(P, B, 3, DB), &transfer(P, B, 4, AUTH));
    const SOURCE: Source<'static> = Source::seq(
        &Source::route(&LEFT, &RIGHT, Some(701)),
        &Source::par(&wire(DB, SERVER, 5), &wire(AUTH, OTHER_SERVER, 6)),
    );
    const E: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * E }, E>::new();
    let mut plan = Plan::new();
    compile_into(&SOURCE, &mut scratch, &mut plan).unwrap();
    assert_eq!(plan.actor_decision_count(), 1);
    assert_eq!(plan.binding(4), None);
    assert_eq!(plan.binding(5), None);
    let scope = Scope {
        start: 0,
        split: 2,
        stop: 4,
    };
    for (arm, actor) in [(Arm::Left, A.role), (Arm::Right, B.role)] {
        let path = [Selection { scope, arm }];
        assert_eq!(plan.binding_in(4, &path).unwrap().sender_actor, actor);
        assert_eq!(plan.binding_in(5, &path).unwrap().sender_actor, actor);
        assert_eq!(plan.binding_in(4, &path).unwrap().message.from, DB);
        assert_eq!(plan.binding_in(5, &path).unwrap().message.from, AUTH);
    }
}

#[test]
fn actor_image_capacity_failure_publishes_neither_events_nor_decisions() {
    const SOURCE: Source<'static> = Source::seq(
        &Source::route(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), Some(701)),
        &wire(DB, SERVER, 3),
    );
    const E: usize = SOURCE.event_count();
    let mut scratch = Scratch::<{ 3 * E }, E>::new();
    let mut image = ActorImage::<E, 0>::new();
    compile_actor_image_into(&wire(P, A, 9), &mut scratch, &mut image).unwrap();
    assert_eq!(image.event_count(), 1);
    assert_eq!(image.actors_at(0, &[]), Some((P.role, A.role)));
    assert_eq!(
        compile_actor_image_into(&SOURCE, &mut scratch, &mut image)
            .unwrap_err()
            .kind,
        ErrorKind::Capacity
    );
    assert_eq!(image.event_count(), 0);
    assert_eq!(image.decision_count(), 0);
    assert_eq!(image.actors_at(0, &[]), None);
}

#[test]
fn direct_actor_ids_keep_the_complete_u8_domain_without_decision_storage() {
    const FROM: EndpointKey = EndpointKey {
        component: 0,
        role: 255,
    };
    const TO: EndpointKey = EndpointKey {
        component: 0,
        role: 254,
    };
    let mut scratch = Scratch::<2, 1>::new();
    let mut image = ActorImage::<1, 0>::new();
    compile_actor_image_into(&wire(FROM, TO, 1), &mut scratch, &mut image).unwrap();
    assert_eq!(image.actors_at(0, &[]), Some((255, 254)));
    assert_eq!(image.actors_at(1, &[]), None);
    assert_eq!(image.decision_count(), 0);
}
