use super::super::super::Message;
use super::*;

fn pair<'a>(left: &'a Source<'a>, right: &'a Source<'a>, shape: u8) -> Source<'a> {
    match shape {
        0 => Source::seq(left, right),
        1 => Source::par(left, right),
        _ => Source::route(left, right, (shape == 3).then_some(7)),
    }
}

#[test]
fn bounded_closure_matches_all_prior_action_edges_across_mixed_shapes() {
    let key = EndpointKey {
        component: 0,
        role: 0,
    };
    let leaves = core::array::from_fn::<_, 4, _>(|index| {
        Source::Send(Message {
            from: key,
            to: key,
            label: index as u8,
            payload: Payload::Wire { schema: 0 },
        })
    });
    let outer = Scope {
        start: 10,
        split: 11,
        stop: 12,
    };
    let bindings = [
        [[0, 1], [1, 2], [2, 3], [3, 4]],
        [[0, 1], [0, 2], [0, 3], [0, 4]],
        [[0, 1], [2, 1], [3, 1], [4, 1]],
        [[0, 1], [2, 3], [4, 5], [6, 7]],
        [[0, 0], [0, 0], [0, 0], [0, 0]],
        [[255, 0], [0, 255], [255, 0], [0, 255]],
        [[255, 255], [0, 1], [1, 0], [255, 255]],
        [[0, 1], [0, 1], [1, 0], [1, 0]],
        [[256, 257], [257, 256], [256, 1], [1, 257]],
    ];
    let decisions = [(outer, [0, 255]), (outer, [255, 0])];
    let mut comparisons = 0;
    for binding in bindings {
        let image = ActorImage::<4, 2>::causal_fixture(&binding, &decisions);
        for left_shape in 0..4 {
            let left = pair(&leaves[0], &leaves[1], left_shape);
            for right_shape in 0..4 {
                let right = pair(&leaves[2], &leaves[3], right_shape);
                let rolled = Source::roll(&right);
                for root_shape in 0..4 {
                    let root = pair(&left, &rolled, root_shape);
                    let messages = wire_events::<4>(&root);
                    let checker = Checker::wire_fixture(&root, &image, &messages);
                    for choices in 0..16 {
                        let arm = |bit| {
                            if choices & (1 << bit) == 0 {
                                Arm::Left
                            } else {
                                Arm::Right
                            }
                        };
                        let selections = [
                            Selection {
                                scope: Scope {
                                    start: 0,
                                    split: 2,
                                    stop: 4,
                                },
                                arm: arm(0),
                            },
                            Selection {
                                scope: Scope {
                                    start: 0,
                                    split: 1,
                                    stop: 2,
                                },
                                arm: arm(1),
                            },
                            Selection {
                                scope: Scope {
                                    start: 2,
                                    split: 3,
                                    stop: 4,
                                },
                                arm: arm(2),
                            },
                            Selection {
                                scope: outer,
                                arm: arm(3),
                            },
                        ];
                        let path = Context::new(&selections);
                        for seed in [0u16, 1, 2, 4, 8, 16, 32, 64, 128, 255] {
                            for floor in 0..=4 {
                                let mut actual = [0u8; 4];
                                for (event, bits) in actual.iter_mut().enumerate() {
                                    if event >= floor
                                        && Checker::<4, 2>::present(&root, 0, event, &path)
                                            == Ok(true)
                                    {
                                        *bits = ((seed >> (2 * event)) & 3) as u8;
                                    }
                                }
                                let mut reference = actual;
                                let expected = checker.reference_close_dependencies(
                                    &root,
                                    0,
                                    &mut reference,
                                    &path,
                                );
                                let observed = checker.close_dependencies_after(
                                    &root,
                                    0,
                                    floor,
                                    &mut actual,
                                    &path,
                                );
                                assert_eq!(observed, expected);
                                assert_eq!(
                                    actual[floor..],
                                    reference[floor..],
                                    "shapes={left_shape}/{right_shape}/{root_shape} choices={choices} seed={seed}"
                                );
                                comparisons += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(comparisons, 460_800);
}
