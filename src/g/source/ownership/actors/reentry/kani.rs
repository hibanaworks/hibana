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
const WORK: Source<'static> = Source::Seq(&TICK, &READ);
const PERMITTED: Source<'static> = Source::Seq(&PERMIT, &WORK);
const PARALLEL_PERMIT: Source<'static> = Source::Par(&PERMIT, &WORK);
const PARALLEL_UNRELATED: Source<'static> = Source::Par(&READ, &UNRELATED);

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
