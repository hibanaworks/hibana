use super::{
    SourceActorImage, SourceScope,
    fixtures::{ROUTE, receive_conflict},
};

/// Enumerate the complete three-actor u8 domain in both route correlations.
/// The CI gate runs this explicitly in release with overflow/debug checks on.
#[test]
#[ignore = "33,554,432 production queries; run explicitly with the checked release profile"]
fn route_query_covers_every_u8_actor_triple_and_correlation() {
    let scope = SourceScope {
        start: 1,
        split: 2,
        stop: 3,
    };
    let mut checked = 0u64;
    for receiver in 0..=u8::MAX {
        for first in 0..=u8::MAX {
            for second in 0..=u8::MAX {
                for swap in [false, true] {
                    let holders = if swap {
                        [u16::from(second), u16::from(first)]
                    } else {
                        [u16::from(first), u16::from(second)]
                    };
                    let actors = SourceActorImage::<8, 1>::causal_fixture(
                        &[
                            [0, receiver.into()],
                            [receiver.into(), first.into()],
                            [receiver.into(), second.into()],
                            [256, receiver.into()],
                        ],
                        &[(scope, holders)],
                    );
                    assert_eq!(
                        receive_conflict(&ROUTE, &actors),
                        !swap || first == second,
                        "receiver={receiver} first={first} second={second} swap={swap}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 2 * 256u64.pow(3));
    std::eprintln!("exhaustive route queries checked={checked}");
}
