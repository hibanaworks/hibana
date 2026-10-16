use super::{ActorImage, ActorRef, Checker, Context, Scope};
use crate::g::source::ownership::actors::{ActorChoice, ActorPair};

const PRIOR: Scope = Scope {
    start: 0,
    split: 1,
    stop: 2,
};
const OTHER: Scope = Scope {
    start: 2,
    split: 3,
    stop: 4,
};

// Constructor reduction and reference bounds have separate production checks.
// Partition the query by reference shape, keeping all actor values symbolic.
// Unreduced equal leaves include the constructor's collapsed-choice results.
// Two event/side iterations need three unwind entries. Each reference has one
// decision above leaves; the correlated query adds one context, independent
// equality at most two. Unwinding assertions check these structural bounds.
fn pair_image<const E: usize>(left: [u8; 2], right: [u8; 2], correlated: bool) -> ActorImage<E, 2> {
    let mut image = ActorImage::new();
    image.choices = [
        ActorChoice {
            scope: PRIOR,
            left: ActorRef::actor(left[0]),
            right: ActorRef::actor(left[1]),
        },
        ActorChoice {
            scope: if correlated { PRIOR } else { OTHER },
            left: ActorRef::actor(right[0]),
            right: ActorRef::actor(right[1]),
        },
    ];
    image.choice_len = 2;
    image
}

#[kani::proof]
#[kani::unwind(3)]
fn actor_leaf_equality_is_exact() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let image = ActorImage::<0, 0>::new();
    assert_eq!(
        Checker { image: &image }.equivalent(
            ActorRef::actor(first),
            ActorRef::actor(second),
            &Context::new(&[])
        ),
        first == second
    );
}

#[kani::proof]
#[kani::unwind(3)]
fn an_actor_leaf_equals_a_choice_only_when_both_alternatives_equal_it() {
    let actor: u8 = kani::any();
    let left: u8 = kani::any();
    let right: u8 = kani::any();
    let image = pair_image::<0>([left, right], [left, right], true);
    assert_eq!(
        Checker { image: &image }.equivalent(
            ActorRef::actor(actor),
            ActorRef::node(0).unwrap(),
            &Context::new(&[])
        ),
        actor == left && actor == right
    );
}

#[kani::proof]
#[kani::unwind(3)]
fn an_actor_choice_equals_a_leaf_only_when_both_alternatives_equal_it() {
    let actor: u8 = kani::any();
    let left: u8 = kani::any();
    let right: u8 = kani::any();
    let image = pair_image::<0>([left, right], [left, right], true);
    assert_eq!(
        Checker { image: &image }.equivalent(
            ActorRef::node(0).unwrap(),
            ActorRef::actor(actor),
            &Context::new(&[])
        ),
        actor == left && actor == right
    );
}

#[kani::proof]
#[kani::unwind(3)]
fn actor_equality_keeps_correlated_and_independent_choices_distinct() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let third: u8 = kani::any();
    let fourth: u8 = kani::any();
    let correlated: bool = kani::any();
    let image = pair_image::<0>([first, second], [third, fourth], correlated);
    let checker = Checker { image: &image };
    let expected = if correlated {
        first == third && second == fourth
    } else {
        first == second && first == third && first == fourth
    };
    assert_eq!(
        checker.equivalent(
            ActorRef::node(0).unwrap(),
            ActorRef::node(1).unwrap(),
            &Context::new(&[])
        ),
        expected
    );
}

#[kani::proof]
#[kani::unwind(3)]
fn equal_candidate_unions_cannot_erase_an_actual_participation_difference() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let actor: u8 = kani::any();
    let mut image = pair_image::<2>([first, second], [second, first], true);
    image.events[0] = ActorPair {
        sender: ActorRef::actor(255),
        receiver: ActorRef::node(0).unwrap(),
    };
    image.events[1] = ActorPair {
        sender: ActorRef::actor(255),
        receiver: ActorRef::node(1).unwrap(),
    };
    image.event_len = 2;
    let checker = Checker { image: &image };
    assert_eq!(
        checker.matching_presence(PRIOR, actor, ActorRef::actor(255), &Context::new(&[])),
        actor == 255 || ((actor == first) == (actor == second))
    );
}

#[kani::proof]
#[kani::unwind(3)]
fn leaf_participation_is_exact_including_collapsed_choice_inputs() {
    let first: u8 = kani::any();
    let second: u8 = kani::any();
    let actor: u8 = kani::any();
    let mut image = ActorImage::<2, 0>::new();
    image.events = [
        ActorPair {
            sender: ActorRef::actor(255),
            receiver: ActorRef::actor(first),
        },
        ActorPair {
            sender: ActorRef::actor(255),
            receiver: ActorRef::actor(second),
        },
    ];
    image.event_len = 2;
    assert_eq!(
        Checker { image: &image }.matching_presence(
            PRIOR,
            actor,
            ActorRef::actor(255),
            &Context::new(&[])
        ),
        actor == 255 || ((actor == first) == (actor == second))
    );
}
