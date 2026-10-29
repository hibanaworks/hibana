use super::super::{EndpointKey, Message, Payload};
use super::{
    ActorImage, ActorPair, ActorRef, Arm, Error, ErrorKind, Scope, Scratch, Selection, Source,
    compile_actor_image_into,
};

#[kani::proof]
fn actor_and_decision_references_have_exact_disjoint_domains() {
    let actor: u8 = kani::any();
    let first: usize = kani::any();
    let second: usize = kani::any();
    let left = ActorRef::node(first);
    let right = ActorRef::node(second);
    let valid = first < u16::MAX as usize - ActorRef::NODE_BASE;
    assert_eq!(left.is_some(), valid);
    if let Some(left) = left {
        assert!(!left.same(ActorRef::actor(actor)));
        assert!(left.0 >= 256 && left.0 < u16::MAX);
        assert_eq!(left.0 as usize, first + 256);
        if let Some(right) = right {
            assert_eq!(left.same(right), first == second);
        }
    }
    assert_eq!(ActorRef::actor(actor).0, u16::from(actor));
}

const INNER: Scope = Scope {
    start: 0,
    split: 1,
    stop: 2,
};
const OUTER: Scope = Scope {
    start: 3,
    split: 4,
    stop: 5,
};

// These are actual compiler decisions, with symbolic actors in every check.
fn nested_image(first: u8, second: u8, third: u8) -> ActorImage<1, 2> {
    let mut image = ActorImage::<1, 2>::new();
    let child = image
        .choice(INNER, ActorRef::actor(first), ActorRef::actor(second))
        .unwrap();
    let parent = image.choice(OUTER, child, ActorRef::actor(third)).unwrap();
    image.events[0] = ActorPair {
        sender: parent,
        receiver: ActorRef::actor(255),
    };
    image.event_len = 1;
    image
}

#[kani::proof]
#[kani::unwind(4)]
fn actor_decisions_collapse_equal_alternatives_and_intern_equal_nodes() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let mut image = ActorImage::<1, 2>::new();
    let child = image
        .choice(INNER, ActorRef::actor(first), ActorRef::actor(second))
        .unwrap();
    assert_eq!(image.decision_count(), usize::from(first != second));
    assert_eq!(
        child.0,
        if first == second {
            u16::from(first)
        } else {
            256
        }
    );
    assert!(
        image
            .choice(INNER, ActorRef::actor(first), ActorRef::actor(second))
            .unwrap()
            .same(child)
    );
    assert_eq!(image.decision_count(), usize::from(first != second));
}

#[kani::proof]
// Two decisions and two selections need at most root/child/leaf recursion and
// one exhausted scan. Unwinding assertions remain enabled for this bound.
#[kani::unwind(4)]
fn nested_actor_decisions_follow_every_complete_selection() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let third: u8 = kani::any();
    let image = nested_image(first, second, third);
    assert!(image.decision_count() <= 2);

    let inner_left: bool = kani::any();
    let outer_left: bool = kani::any();
    let path = [
        Selection {
            scope: INNER,
            arm: if inner_left { Arm::Left } else { Arm::Right },
        },
        Selection {
            scope: OUTER,
            arm: if outer_left { Arm::Left } else { Arm::Right },
        },
    ];
    let expected = if outer_left {
        if inner_left { first } else { second }
    } else {
        third
    };
    assert_eq!(image.actors_at(0, &path), Some((expected, 255)));
    assert_eq!(image.actors_at(1, &path), None);
}

#[kani::proof]
#[kani::unwind(4)]
fn possible_actor_membership_contains_exactly_the_canonical_choice_leaves() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let third: u8 = kani::any();
    let actor: u8 = kani::any();
    let image = nested_image(first, second, third);
    let members = image.possible_actors(0);
    let actual = members[actor as usize / 8] & (1 << (actor % 8)) != 0;
    assert_eq!(
        actual,
        actor == first || actor == second || actor == third || actor == 255
    );
}

#[kani::proof]
#[kani::unwind(4)]
fn an_unselected_inner_decision_cannot_change_the_outer_right_actor() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let third: u8 = kani::any();
    let image = nested_image(first, second, third);
    assert_eq!(
        image.actors_at(
            0,
            &[Selection {
                scope: OUTER,
                arm: Arm::Right
            }]
        ),
        Some((third, 255))
    );
}

#[kani::proof]
#[kani::unwind(4)]
fn unresolved_actor_decisions_require_unanimous_alternatives() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let third: u8 = kani::any();
    let image = nested_image(first, second, third);
    assert_eq!(
        image.actors_at(0, &[]),
        if first == second && second == third {
            Some((first, 255))
        } else {
            None
        }
    );
}

#[kani::proof]
#[kani::unwind(16)]
fn insufficient_actor_image_storage_cannot_publish_a_partial_rebuild() {
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
    const SERVICE: EndpointKey = EndpointKey {
        component: 0,
        role: 3,
    };
    const DB: EndpointKey = EndpointKey {
        component: 0,
        role: 10,
    };
    const BEFORE: Source<'static> = Source::Send(Message {
        from: P,
        to: A,
        label: 1,
        payload: Payload::Wire { schema: 0 },
    });
    const LEFT: Source<'static> = Source::Send(Message {
        from: P,
        to: A,
        label: 2,
        payload: Payload::Endpoint(DB),
    });
    const RIGHT: Source<'static> = Source::Send(Message {
        from: P,
        to: B,
        label: 3,
        payload: Payload::Endpoint(DB),
    });
    const WORK: Source<'static> = Source::Send(Message {
        from: DB,
        to: SERVICE,
        label: 4,
        payload: Payload::Wire { schema: 0 },
    });
    const SOURCE: Source<'static> = Source::seq(&Source::route(&LEFT, &RIGHT, None), &WORK);
    let mut scratch = Scratch::<5, 3>::new();
    let mut image = ActorImage::<3, 0>::new();
    assert!(compile_actor_image_into(&BEFORE, &mut scratch, &mut image).is_ok());
    assert_eq!(image.actors_at(0, &[]), Some((0, 1)));
    assert!(matches!(
        compile_actor_image_into(&SOURCE, &mut scratch, &mut image),
        Err(Error {
            kind: ErrorKind::Capacity,
            endpoint: None
        })
    ));
    assert_eq!(image.event_count(), 0);
    assert_eq!(image.decision_count(), 0);
    assert_eq!(image.actors_at(0, &[]), None);
}
