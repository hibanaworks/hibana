use super::fixtures::{LINEAR, receive_conflict};
use super::{
    ActorMap, Paths, RollBodyRange, SourceActorImage, SourceArm, SourceContext, SourceScope,
    SourceSelection,
};

fn owned_linear_receive_handoff<const WORD: u8>() {
    let receiver = seed::<WORD>();
    let relay_sender = kani::any::<u8>();
    let relay_receiver = kani::any::<u8>();
    let later_sender = kani::any::<u8>();
    let actors = SourceActorImage::<3, 0>::causal_fixture(
        &[
            [0, receiver.into()],
            [relay_sender.into(), relay_receiver.into()],
            [later_sender.into(), receiver.into()],
        ],
        &[],
    );
    let expected =
        later_sender == receiver || (relay_sender == receiver && later_sender == relay_receiver);
    assert_eq!(receive_conflict(&LINEAR, &actors), expected);
}

// Partition only the seed's two high bits. All four partitions are checked;
// their union is the complete u8 domain. Every other actor remains arbitrary
// u8 in every partition. This fixes the bitset word during CBMC construction
// without assumptions, fewer role values or a larger memory allowance.
fn seed<const WORD: u8>() -> u8 {
    assert!(WORD < 4);
    WORD * 64 | (kani::any::<u8>() & 63)
}

macro_rules! seed_words {
    ($check:ident, $word0:ident, $word1:ident, $word2:ident, $word3:ident) => {
        // Five includes exhaustion of every loop in the three-event source.
        // Unwinding and sanity assertions stay enabled in every partition.
        #[kani::proof]
        #[kani::unwind(5)]
        fn $word0() {
            $check::<0>();
        }
        #[kani::proof]
        #[kani::unwind(5)]
        fn $word1() {
            $check::<1>();
        }
        #[kani::proof]
        #[kani::unwind(5)]
        fn $word2() {
            $check::<2>();
        }
        #[kani::proof]
        #[kani::unwind(5)]
        fn $word3() {
            $check::<3>();
        }
    };
}

seed_words!(
    owned_linear_receive_handoff,
    owned_linear_seed_word0,
    owned_linear_seed_word1,
    owned_linear_seed_word2,
    owned_linear_seed_word3
);

#[kani::proof]
#[kani::unwind(5)]
fn repeated_inner_choices_are_independent_while_outer_choices_stay_shared() {
    let inner = SourceScope {
        start: 1,
        split: 2,
        stop: 3,
    };
    let outer = SourceScope {
        start: 0,
        split: 4,
        stop: 5,
    };
    let empty = SourceContext::new(&[]);
    let arm = if kani::any::<bool>() {
        SourceArm::Left
    } else {
        SourceArm::Right
    };
    let other = match arm {
        SourceArm::Left => SourceArm::Right,
        SourceArm::Right => SourceArm::Left,
    };
    let current = empty.select(SourceSelection { scope: inner, arm });
    let next = empty.select(SourceSelection {
        scope: inner,
        arm: other,
    });
    let shared = empty.select(SourceSelection { scope: outer, arm });
    let image = SourceActorImage::<0, 0>::new();
    let map = ActorMap::Owned {
        image: &image,
        events: image.event_actors(),
        paths: Paths {
            shared: &shared,
            current: &current,
            next: &next,
        },
        repeated: Some(RollBodyRange::new(1, 4)),
    };
    assert_eq!(map.selected_route(1, 2, 3, 0), Some(arm));
    assert_eq!(map.selected_route(1, 2, 3, 3), Some(other));
    assert_eq!(map.selected_route(0, 4, 5, 0), Some(arm));
    assert_eq!(map.selected_route(0, 4, 5, 3), Some(arm));
}
