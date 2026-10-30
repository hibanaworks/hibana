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
fn an_empty_outgoing_seed_set_still_checks_the_resource_return() {
    let key = EndpointKey {
        component: 0,
        role: 10,
    };
    let control = |role| EndpointKey { component: 0, role };
    let body = Source::Send(Message {
        from: key,
        to: control(3),
        label: 1,
        payload: Payload::Wire { schema: 0 },
    });
    let loan = Source::Send(Message {
        from: control(0),
        to: control(1),
        label: 2,
        payload: Payload::Endpoint(key),
    });
    let returned = Source::Send(Message {
        from: control(1),
        to: control(0),
        label: 3,
        payload: Payload::Endpoint(key),
    });
    for returns in [false, true] {
        let suffix = Source::seq(&loan, &returned);
        let root = Source::seq(&body, if returns { &suffix } else { &loan });
        let mut messages = [None; 3];
        for (event, slot) in messages.iter_mut().enumerate().take(root.event_count()) {
            *slot = Some(root.message_at(event));
        }
        let image = ActorImage::<3, 0>::causal_fixture(
            &[[0, 3], [0, 1], [1, 0]][..root.event_count()],
            &[],
        );
        let checker = Checker {
            root: &root,
            image: &image,
            query: Query::new(&root, &[], &[]),
            messages: &messages[..root.event_count()],
        };
        assert_eq!(
            checker.temporary_departures(
                &Body {
                    source: &body,
                    start: 0,
                    key
                },
                0,
                &Context::new(&[])
            ),
            Ok(returns)
        );
    }
}

#[test]
fn every_small_restoring_trace_matches_the_explicit_departure_check() {
    let key = EndpointKey {
        component: 1,
        role: 10,
    };
    let control = |role| EndpointKey { component: 0, role };
    let body = Source::Send(Message {
        from: key,
        to: control(4),
        label: 1,
        payload: Payload::Wire { schema: 0 },
    });
    let mut compared = 0;
    for word in 1..64 {
        let holders = [0, word & 3, (word >> 2) & 3, (word >> 4) & 3, 0];
        let steps = core::array::from_fn::<_, 4, _>(|event| {
            let (sender, receiver) = (holders[event], holders[event + 1]);
            Source::Send(Message {
                from: control(sender),
                to: control(if sender == receiver { 4 } else { receiver }),
                label: 2 + event as u8,
                payload: if sender == receiver {
                    Payload::Wire { schema: 0 }
                } else {
                    Payload::Endpoint(key)
                },
            })
        });
        let left = Source::seq(&steps[0], &steps[1]);
        let right = Source::seq(&steps[2], &steps[3]);
        let suffix = Source::seq(&left, &right);
        let root = Source::seq(&body, &suffix);
        let mut facts = Scratch::<8, 8>::new();
        let mut image = ActorImage::<8, 8>::new();
        super::super::compile_actor_image_into(&root, &mut facts, &mut image).unwrap();
        let checker = Checker {
            root: &root,
            image: &image,
            query: Query::new(&root, &facts.symbols[..facts.symbol_len], &facts.initial),
            messages: &facts.events[..facts.event_len],
        };
        let body = Body {
            source: &body,
            start: 0,
            key,
        };
        let path = Context::new(&[]);
        assert_eq!(checker.owner(key, 0, &path), Ok(0));
        assert_eq!(checker.owner(key, root.event_count(), &path), Ok(0));
        assert_eq!(checker.temporary_departures(&body, 0, &path), Ok(true));
        assert_eq!(checker.run(&body, &path), Ok(true));
        compared += 1;
    }
    assert_eq!(compared, 63);
}

#[test]
fn a_different_component_with_the_same_role_still_needs_fresh_permission() {
    let key = EndpointKey {
        component: 1,
        role: 10,
    };
    let other = EndpointKey {
        component: 2,
        role: 10,
    };
    let control = |role| EndpointKey { component: 0, role };
    let send = |from, to, label, payload| {
        Source::Send(Message {
            from,
            to,
            label,
            payload,
        })
    };
    let out = send(control(1), control(3), 1, Payload::Endpoint(other));
    let back = send(control(3), control(1), 2, Payload::Endpoint(other));
    let other_work = Source::seq(&out, &back);
    let use_key = send(key, control(4), 3, Payload::Wire { schema: 0 });
    let body = Source::par(&other_work, &use_key);
    let loan = send(control(0), control(2), 4, Payload::Endpoint(key));
    let dependency = send(other, control(2), 5, Payload::Wire { schema: 0 });
    let returned = send(control(2), control(0), 6, Payload::Endpoint(key));
    let continuation = Source::seq(&dependency, &returned);
    let suffix = Source::seq(&loan, &continuation);
    let root = Source::seq(&body, &suffix);
    let mut facts = Scratch::<8, 8>::new();
    let mut image = ActorImage::<8, 8>::new();
    super::super::compile_actor_image_into(&root, &mut facts, &mut image).unwrap();
    let checker = Checker {
        root: &root,
        image: &image,
        query: Query::new(&root, &facts.symbols[..facts.symbol_len], &facts.initial),
        messages: &facts.events[..facts.event_len],
    };
    let body = Body {
        source: &body,
        start: 0,
        key,
    };
    let path = Context::new(&[]);
    assert_eq!(checker.owner(key, 0, &path), Ok(0));
    assert_eq!(checker.owner(key, root.event_count(), &path), Ok(0));
    assert_eq!(checker.temporary_departures(&body, 0, &path), Ok(false));
    assert_eq!(checker.run(&body, &path), Ok(false));
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
