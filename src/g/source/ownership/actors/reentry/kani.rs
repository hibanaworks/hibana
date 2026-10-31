use super::super::super::Message;
use super::{ActorImage, Body, Checker, Context, EndpointKey, Payload, Query, Source, wire_events};

fn check_outgoing_return<const RETURNED: bool>() {
    let key = EndpointKey {
        component: kani::any(),
        role: kani::any(),
    };
    let owner: u8 = kani::any();
    let borrower = owner.wrapping_add(1);
    let returned = RETURNED;
    let control = |role| EndpointKey { component: 0, role };
    let body = Source::Send(Message {
        from: key,
        to: control(owner.wrapping_add(2)),
        label: 1,
        payload: Payload::Wire { schema: 0 },
    });
    let loan = Source::Send(Message {
        from: control(owner),
        to: control(borrower),
        label: 2,
        payload: Payload::Endpoint(key),
    });
    let back = Source::Send(Message {
        from: control(borrower),
        to: control(owner),
        label: 3,
        payload: Payload::Endpoint(key),
    });
    let suffix = Source::seq(&loan, &back);
    let source = Source::seq(&body, if returned { &suffix } else { &loan });
    let count = source.event_count();
    let rows = [
        [owner.into(), owner.wrapping_add(2).into()],
        [owner.into(), borrower.into()],
        [borrower.into(), owner.into()],
    ];
    let image = ActorImage::<3, 0>::causal_fixture(&rows[..count], &[]);
    let mut messages = [None; 3];
    let mut event = 0;
    while event < count {
        messages[event] = Some(source.message_at(event));
        event += 1;
    }
    let checker = Checker {
        root: &source,
        image: &image,
        query: Query::new(&source, &[], &[]),
        messages: &messages[..count],
    };
    assert_eq!(
        checker.temporary_departures(
            &Body {
                source: &body,
                start: 0,
                key
            },
            owner,
            &Context::new(&[])
        ),
        Ok(returned)
    );
    kani::cover!(key.component == u16::MAX && key.role == u8::MAX);
    kani::cover!(key.component == 0 && key.role == u8::MAX);
    kani::cover!(owner == u8::MAX);
}

#[kani::proof]
#[kani::unwind(5)]
fn no_other_outgoing_endpoint_preserves_a_return_for_every_resource_key() {
    check_outgoing_return::<true>();
}

#[kani::proof]
#[kani::unwind(5)]
fn no_other_outgoing_endpoint_rejects_a_missing_return_for_every_resource_key() {
    check_outgoing_return::<false>();
}

// This family checks the actual causal closure for a non-restoring resource.
// Prefix ownership is supplied by the existing source query in run(), rather
// than symbolically re-expanding that separate ownership algorithm here.
// Three source events and three tree levels need four unwind entries. All
// actor values remain symbolic, and unwinding assertions stay enabled.
const RESOURCE: EndpointKey = EndpointKey {
    component: 0,
    role: 10,
};

const fn wire(from: u8, to: u8) -> Source<'static> {
    Source::Send(Message {
        from: EndpointKey {
            component: 0,
            role: from,
        },
        to: EndpointKey {
            component: 0,
            role: to,
        },
        label: 1,
        payload: Payload::Wire { schema: 0 },
    })
}

const TICK: Source<'static> = wire(2, 1);
const READ: Source<'static> = wire(10, 4);
const PERMIT: Source<'static> = wire(1, 2);
const UNRELATED: Source<'static> = wire(6, 7);
const WORK: Source<'static> = Source::seq(&TICK, &READ);
const PERMITTED: Source<'static> = Source::seq(&PERMIT, &WORK);
const PARALLEL_PERMIT: Source<'static> = Source::par(&PERMIT, &WORK);
const PARALLEL_UNRELATED: Source<'static> = Source::par(&READ, &UNRELATED);

fn actors() -> (u8, u8) {
    (kani::any(), kani::any())
}

#[kani::proof]
#[kani::unwind(4)]
fn a_remote_origin_without_a_fresh_permit_is_rejected() {
    let (holder, controller) = actors();
    let image = ActorImage::<2, 0>::causal_fixture(
        &[[controller.into(), holder.into()], [holder.into(), 4]],
        &[],
    );
    assert_eq!(
        Checker::wire_fixture(&WORK, &image, &wire_events::<2>(&WORK)).origins(
            &Body {
                source: &WORK,
                start: 0,
                key: RESOURCE
            },
            holder,
            &Context::new(&[]),
        ),
        Ok(controller == holder)
    );
}

#[kani::proof]
#[kani::unwind(4)]
fn a_fresh_permit_authorizes_the_dependent_sender() {
    let (holder, controller) = actors();
    let image = ActorImage::<3, 0>::causal_fixture(
        &[
            [holder.into(), controller.into()],
            [controller.into(), holder.into()],
            [holder.into(), 4],
        ],
        &[],
    );
    assert_eq!(
        Checker::wire_fixture(&PERMITTED, &image, &wire_events::<3>(&PERMITTED)).origins(
            &Body {
                source: &PERMITTED,
                start: 0,
                key: RESOURCE
            },
            holder,
            &Context::new(&[]),
        ),
        Ok(true)
    );
}

#[kani::proof]
#[kani::unwind(4)]
fn a_parallel_permit_does_not_authorize_an_independently_enabled_send() {
    let (holder, controller) = actors();
    let image = ActorImage::<3, 0>::causal_fixture(
        &[
            [holder.into(), controller.into()],
            [controller.into(), holder.into()],
            [holder.into(), 4],
        ],
        &[],
    );
    assert_eq!(
        Checker::wire_fixture(
            &PARALLEL_PERMIT,
            &image,
            &wire_events::<3>(&PARALLEL_PERMIT)
        )
        .origins(
            &Body {
                source: &PARALLEL_PERMIT,
                start: 0,
                key: RESOURCE
            },
            holder,
            &Context::new(&[]),
        ),
        Ok(controller == holder)
    );
}

#[kani::proof]
#[kani::unwind(4)]
fn an_unrelated_parallel_sender_needs_no_resource_permit() {
    let (holder, _) = actors();
    let image = ActorImage::<2, 0>::causal_fixture(&[[holder.into(), 4], [6, 7]], &[]);
    assert_eq!(
        Checker::wire_fixture(
            &PARALLEL_UNRELATED,
            &image,
            &wire_events::<2>(&PARALLEL_UNRELATED)
        )
        .origins(
            &Body {
                source: &PARALLEL_UNRELATED,
                start: 0,
                key: RESOURCE
            },
            holder,
            &Context::new(&[]),
        ),
        Ok(true)
    );
}

#[kani::proof]
#[kani::unwind(4)]
fn receipt_causality_comes_from_the_actual_actor_handoff() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    let c: u8 = kani::any();
    let d: u8 = kani::any();
    let image =
        ActorImage::<2, 0>::causal_fixture(&[[a.into(), b.into()], [c.into(), d.into()]], &[]);
    assert_eq!(
        Checker::wire_fixture(&WORK, &image, &wire_events::<2>(&WORK)).causally_before(
            (0, 1),
            (1, 1),
            &Context::new(&[])
        ),
        Ok(b == c || b == d),
    );
}

#[kani::proof]
#[kani::unwind(4)]
fn parallel_preorder_does_not_supply_a_resource_guard_handoff() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    let c: u8 = kani::any();
    let d: u8 = kani::any();
    let image =
        ActorImage::<2, 0>::causal_fixture(&[[a.into(), b.into()], [c.into(), d.into()]], &[]);
    assert_eq!(
        Checker::wire_fixture(
            &PARALLEL_UNRELATED,
            &image,
            &wire_events::<2>(&PARALLEL_UNRELATED)
        )
        .causally_before((0, 1), (1, 1), &Context::new(&[])),
        Ok(false),
    );
}

// Each source form has its own harness so CBMC need not merge four recursive
// source trees. Together they retain every shape and every symbolic input.
fn bounded_predecessors_preserve_every_queried_action<const SHAPE: u8>(floor: usize) {
    let pairs = core::array::from_fn::<_, 2, _>(|_| {
        let sender: u8 = kani::any();
        let receiver: u8 = kani::any();
        [u16::from(sender), u16::from(receiver)]
    });
    let image = ActorImage::<2, 0>::causal_fixture(&pairs, &[]);
    const SEQUENTIAL: Source<'static> = Source::seq(&TICK, &READ);
    const PARALLEL: Source<'static> = Source::par(&TICK, &READ);
    const CHOICE: Source<'static> = Source::route(&TICK, &READ, None);
    const ROLLED: Source<'static> = Source::roll(&SEQUENTIAL);
    let source = const {
        match SHAPE {
            0 => &SEQUENTIAL,
            1 => &PARALLEL,
            2 => &CHOICE,
            3 => &ROLLED,
            _ => panic!("unknown closure source form"),
        }
    };
    let selection = super::Selection {
        scope: super::Scope {
            start: 0,
            split: 1,
            stop: 2,
        },
        arm: if kani::any() {
            super::Arm::Left
        } else {
            super::Arm::Right
        },
    };
    let selections = [selection];
    let path = Context::new(&selections);
    let messages = wire_events::<2>(source);
    let checker = Checker::wire_fixture(source, &image, &messages);
    let seed: u8 = kani::any();
    let mut actual = [0u8; 2];
    for event in 0..2 {
        if event >= floor && Checker::<2, 0>::present(source, 0, event, &path) == Ok(true) {
            actual[event] = (seed >> (2 * event)) & 3;
        }
    }
    let mut reference = actual;
    assert_eq!(
        checker.reference_close_dependencies(source, 0, &mut reference, &path),
        Ok(())
    );
    assert_eq!(
        checker.close_dependencies_after(source, 0, floor, &mut actual, &path),
        Ok(())
    );
    for event in floor..2 {
        assert_eq!(actual[event], reference[event]);
    }
}

// Each closure traverses two events and at most two recursive source edges.
// Three entries cover those loops and edges, including the terminating check.
#[kani::proof]
#[kani::unwind(3)]
fn bounded_sequential_predecessors_from_zero_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<0>(0);
}

#[kani::proof]
#[kani::unwind(3)]
fn bounded_sequential_predecessors_from_one_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<0>(1);
}

#[kani::proof]
#[kani::unwind(3)]
fn bounded_sequential_predecessors_from_two_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<0>(2);
}

#[kani::proof]
#[kani::unwind(3)]
fn bounded_parallel_predecessors_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<1>(usize::from(kani::any::<u8>() % 3));
}

#[kani::proof]
#[kani::unwind(3)]
fn bounded_selected_predecessors_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<2>(usize::from(kani::any::<u8>() % 3));
}

#[kani::proof]
#[kani::unwind(3)]
fn bounded_rolled_predecessors_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<3>(usize::from(kani::any::<u8>() % 3));
}

fn source_first_transfer_preserves_the_complete_endpoint_key<const SHAPE: u8>() {
    fn key() -> EndpointKey {
        EndpointKey {
            component: kani::any(),
            role: kani::any(),
        }
    }
    fn transfer(key: EndpointKey, owned: bool) -> Source<'static> {
        Source::Send(Message {
            from: RESOURCE,
            to: RESOURCE,
            label: 1,
            payload: if owned {
                Payload::Endpoint(key)
            } else {
                Payload::Wire { schema: 0 }
            },
        })
    }
    let first = key();
    let second = key();
    let target = key();
    let first_owned: bool = kani::any();
    let second_owned: bool = kani::any();
    let left = transfer(first, first_owned);
    let right = transfer(second, second_owned);
    let combined = match SHAPE {
        0 | 3 => Source::seq(&left, &right),
        1 => Source::par(&left, &right),
        2 => Source::route(&left, &right, None),
        _ => panic!("unknown transfer source form"),
    };
    let rolled = Source::roll(&combined);
    let source = if SHAPE == 3 { &rolled } else { &combined };
    let expected = if first_owned && first.same(target) {
        Some(0)
    } else if second_owned && second.same(target) {
        Some(1)
    } else {
        None
    };
    assert_eq!(source.first_transfer(target), expected);
    assert_eq!(source.transfers(target), expected.is_some());
    assert_eq!(source.has_delegation(), first_owned || second_owned);
    assert_eq!(source.event_count(), 2);
    assert_eq!(
        source.predecessor_range(1, 0),
        if SHAPE == 0 || SHAPE == 3 {
            Some((0, 1))
        } else {
            None
        }
    );
}

// The binary sources have depth two; the rolled source has depth three.
// One extra unwind entry closes each recursive traversal. Unwinding assertions
// remain enabled, and every endpoint key and payload-kind input stays symbolic.
#[kani::proof]
#[kani::unwind(3)]
fn source_first_transfer_preserves_complete_key_in_sequence() {
    source_first_transfer_preserves_the_complete_endpoint_key::<0>();
}

#[kani::proof]
#[kani::unwind(3)]
fn source_first_transfer_preserves_complete_key_in_parallel() {
    source_first_transfer_preserves_the_complete_endpoint_key::<1>();
}

#[kani::proof]
#[kani::unwind(3)]
fn source_first_transfer_preserves_complete_key_in_choice() {
    source_first_transfer_preserves_the_complete_endpoint_key::<2>();
}

#[kani::proof]
#[kani::unwind(4)]
fn source_first_transfer_preserves_complete_key_in_roll() {
    source_first_transfer_preserves_the_complete_endpoint_key::<3>();
}
