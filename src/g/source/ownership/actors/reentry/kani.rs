use super::super::super::Message;
use super::{ActorImage, Body, Checker, Context, EndpointKey, Payload, Source};

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
const WORK: Source<'static> = Source::Seq(&TICK, &READ, 2);
const PERMITTED: Source<'static> = Source::Seq(&PERMIT, &WORK, 3);
const PARALLEL_PERMIT: Source<'static> = Source::Par(&PERMIT, &WORK, 3);
const PARALLEL_UNRELATED: Source<'static> = Source::Par(&READ, &UNRELATED, 2);

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
        Checker {
            root: &WORK,
            image: &image
        }
        .origins(
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
        Checker {
            root: &PERMITTED,
            image: &image
        }
        .origins(
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
        Checker {
            root: &PARALLEL_PERMIT,
            image: &image
        }
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
        Checker {
            root: &PARALLEL_UNRELATED,
            image: &image
        }
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
        Checker {
            root: &WORK,
            image: &image
        }
        .causally_before((0, 1), (1, 1), &Context::new(&[])),
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
        Checker {
            root: &PARALLEL_UNRELATED,
            image: &image
        }
        .causally_before((0, 1), (1, 1), &Context::new(&[])),
        Ok(false),
    );
}

// Each source form has its own harness so CBMC need not merge four recursive
// source trees. Together they retain every shape and every symbolic input.
fn bounded_predecessors_preserve_every_queried_action<const SHAPE: u8>() {
    let pairs = core::array::from_fn::<_, 2, _>(|_| {
        let sender: u8 = kani::any();
        let receiver: u8 = kani::any();
        [u16::from(sender), u16::from(receiver)]
    });
    let image = ActorImage::<2, 0>::causal_fixture(&pairs, &[]);
    const SEQUENTIAL: Source<'static> = Source::Seq(&TICK, &READ, 2);
    const PARALLEL: Source<'static> = Source::Par(&TICK, &READ, 2);
    const CHOICE: Source<'static> = Source::Route {
        left: &TICK,
        right: &READ,
        resolver: None,
        events: 2,
    };
    const ROLLED: Source<'static> = Source::Roll(&SEQUENTIAL, 2);
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
    let checker = Checker {
        root: source,
        image: &image,
    };
    let floor = usize::from(kani::any::<u8>() % 3);
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

#[kani::proof]
#[kani::unwind(4)]
fn bounded_sequential_predecessors_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<0>();
}

#[kani::proof]
#[kani::unwind(4)]
fn bounded_parallel_predecessors_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<1>();
}

#[kani::proof]
#[kani::unwind(4)]
fn bounded_selected_predecessors_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<2>();
}

#[kani::proof]
#[kani::unwind(4)]
fn bounded_rolled_predecessors_preserve_every_queried_action() {
    bounded_predecessors_preserve_every_queried_action::<3>();
}

#[kani::proof]
#[kani::unwind(5)]
fn source_first_transfer_preserves_the_complete_endpoint_key() {
    fn key() -> EndpointKey {
        EndpointKey {
            component: kani::any(),
            role: kani::any(),
        }
    }
    fn transfer(key: EndpointKey) -> Source<'static> {
        Source::Send(Message {
            from: RESOURCE,
            to: RESOURCE,
            label: 1,
            payload: Payload::Endpoint(key),
        })
    }
    let first = key();
    let second = key();
    let target = key();
    let left = transfer(first);
    let right = transfer(second);
    let sequential = Source::Seq(&left, &right, 2);
    let parallel = Source::Par(&left, &right, 2);
    let choice = Source::Route {
        left: &left,
        right: &right,
        resolver: None,
        events: 2,
    };
    let rolled = Source::Roll(&sequential, 2);
    let shape: u8 = kani::any();
    let source = match shape & 3 {
        0 => &sequential,
        1 => &parallel,
        2 => &choice,
        _ => &rolled,
    };
    let expected = if first.same(target) {
        Some(0)
    } else if second.same(target) {
        Some(1)
    } else {
        None
    };
    assert_eq!(source.first_transfer(target), expected);
}
