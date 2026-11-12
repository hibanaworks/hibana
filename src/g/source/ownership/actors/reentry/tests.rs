use super::super::super::Message;
use super::*;

fn connected<const E: usize>(needed: &mut [u8; E], actors: &[u16; E]) {
    let original = *needed;
    let mut parents = ACTOR_ROOTS;
    let mut seeds = [false; ACTOR_COUNT];
    for event in 0..E {
        if original[event] != 0 {
            Checker::<E, 0>::join_component(
                (&mut parents, &mut seeds),
                ((actors[event] >> 8) as u8, actors[event] as u8),
                original[event] == 3,
            );
        }
    }
    for event in 0..E {
        if original[event] != 0 {
            let root = Checker::<E, 0>::component_root(&mut parents, (actors[event] >> 8) as u8);
            needed[event] = if seeds[root as usize] { 3 } else { 0 };
        }
    }
}

#[test]
fn actor_components_match_independent_reachability_for_every_three_edge_graph() {
    // Four actors, all directed endpoint pairs and all absent/present/seed
    // assignments: 4^6 * 3^3 = 110,592 graphs. Reachability is undirected here.
    for encoding in 0..4096u32 {
        let actors = core::array::from_fn::<_, 3, _>(|event| {
            let sender = (encoding >> (event * 4)) & 3;
            let receiver = (encoding >> (event * 4 + 2)) & 3;
            (sender << 8 | receiver) as u16
        });
        for status in 0..27 {
            let mut state = status;
            let original = core::array::from_fn::<_, 3, _>(|_| {
                let digit = state % 3;
                state /= 3;
                [0, 1, 3][digit]
            });
            let mut reachable = [false; 4];
            for (event, pair) in actors.iter().enumerate() {
                if original[event] == 3 {
                    reachable[(pair >> 8) as usize] = true;
                    reachable[(pair & 255) as usize] = true;
                }
            }
            for _ in 0..4 {
                for (event, pair) in actors.iter().enumerate() {
                    let (a, b) = ((pair >> 8) as usize, (pair & 255) as usize);
                    if original[event] != 0 && (reachable[a] || reachable[b]) {
                        reachable[a] = true;
                        reachable[b] = true;
                    }
                }
            }
            let expected = core::array::from_fn::<_, 3, _>(|event| {
                if original[event] != 0 && reachable[(actors[event] >> 8) as usize] {
                    3
                } else {
                    0
                }
            });
            let mut actual = original;
            connected(&mut actual, &actors);
            assert_eq!(actual, expected, "graph={encoding} states={status}");
        }
    }
}

#[test]
fn merged_branch_summaries_match_explicit_paths_and_preserve_dependency_closure() {
    let control = |role| EndpointKey { component: 0, role };
    let key = control(4);
    let action = |from, to| {
        Source::Send(Message {
            from,
            to,
            label: 1,
            payload: Payload::Wire { schema: 0 },
        })
    };
    let left = action(key, control(5));
    let right = action(key, control(6));
    let tail = action(control(7), control(8));
    let choice = Source::route(&left, &right, None);
    let scope = Scope {
        start: 0,
        split: 1,
        stop: 2,
    };
    let mut cases = 0;
    for shape in 0..2 {
        let source = pair(&choice, &tail, shape);
        let messages = wire_events::<3>(&source);
        let body = Body {
            source: &source,
            start: 0,
            key,
        };
        // All 4^6 physical endpoint assignments, with either tail endpoint
        // optionally conditional on this same source choice. Include a par
        // sibling so merging cannot give it a preceding arm's knowledge.
        for encoding in 0..4096u32 {
            let pairs = core::array::from_fn::<_, 3, _>(|event| {
                [
                    ((encoding >> (event * 4)) & 3) as u16,
                    ((encoding >> (event * 4 + 2)) & 3) as u16,
                ]
            });
            for conditional in 0..4 {
                let mut bindings = pairs;
                if conditional & 1 != 0 {
                    bindings[2][0] = 256;
                }
                if conditional & 2 != 0 {
                    bindings[2][1] = 257;
                }
                let image = ActorImage::<3, 2>::causal_fixture(
                    &bindings,
                    &[
                        (scope, [pairs[0][0], pairs[1][0]]),
                        (scope, [pairs[0][1], pairs[1][1]]),
                    ],
                );
                let checker = Checker::wire_fixture(&source, &image, &messages);
                let mut expected = [true; 4];
                for (arm, event) in [(Arm::Left, 0), (Arm::Right, 1)] {
                    let selections = [Selection { scope, arm }];
                    let path = Context::new(&selections);
                    let mut actors = [0; 3];
                    for (index, pair) in actors.iter_mut().enumerate() {
                        *pair = (checker.side(index, 0, &path).unwrap() as u16) << 8
                            | checker.side(index, 1, &path).unwrap() as u16;
                    }
                    let mut needed = [0; 3];
                    needed[event] = 3;
                    needed[2] = 1;
                    connected(&mut needed, &actors);
                    let mut closed = needed;
                    assert_eq!(
                        checker.close_dependencies(&source, 0, &mut closed, &path),
                        Ok(())
                    );
                    assert_eq!(closed, needed, "component cover must already be closed");
                    for (owner, accepted) in expected.iter_mut().enumerate() {
                        *accepted &= checker.origins(&body, owner as u8, &path).unwrap();
                    }
                }
                for (owner, expected) in expected.into_iter().enumerate() {
                    assert_eq!(
                        checker.selected_body(
                            &body,
                            &mut [None; 3],
                            (Some(owner as u8), Some(255))
                        ),
                        expected,
                        "shape={shape} graph={encoding} conditional={conditional} owner={owner}"
                    );
                    cases += 1;
                }
            }
        }
    }
    assert_eq!(cases, 131_072);
}

#[test]
fn shared_dependency_demands_match_both_explicit_route_paths() {
    let leaves = core::array::from_fn::<_, 3, _>(|label| {
        Source::Send(Message {
            from: EndpointKey {
                component: 0,
                role: 0,
            },
            to: EndpointKey {
                component: 0,
                role: 1,
            },
            label: label as u8,
            payload: Payload::Wire { schema: 0 },
        })
    });
    let choice = Source::route(&leaves[0], &leaves[1], None);
    let mut compared = 0;
    let mut merged = 0;
    let mut split = 0;
    for shape in 0..4 {
        let prefix = shape >= 2;
        let source = if prefix {
            pair(&leaves[2], &choice, shape % 2)
        } else {
            pair(&choice, &leaves[2], shape)
        };
        let scope = if prefix {
            Scope {
                start: 1,
                split: 2,
                stop: 3,
            }
        } else {
            Scope {
                start: 0,
                split: 1,
                stop: 2,
            }
        };
        let messages = wire_events::<3>(&source);
        // All 3^6 actor endpoints, 4^3 seed masks, four floors and four
        // sequence/parallel placements: 746,496 partial-path closures.
        for encoding in 0..729u32 {
            let mut digits = encoding;
            let bindings = core::array::from_fn::<_, 3, _>(|_| {
                let sender = (digits % 3) as u16;
                digits /= 3;
                let receiver = (digits % 3) as u16;
                digits /= 3;
                [sender, receiver]
            });
            let image = ActorImage::<3, 0>::causal_fixture(&bindings, &[]);
            let checker = Checker::wire_fixture(&source, &image, &messages);
            for mask in 0..64 {
                for floor in 0..=3 {
                    let original = core::array::from_fn::<_, 3, _>(|event| {
                        if event >= floor {
                            ((mask >> (2 * event)) & 3) as u8
                        } else {
                            0
                        }
                    });
                    let mut left = original;
                    let mut right = original;
                    for (arm, needed) in [(Arm::Left, &mut left), (Arm::Right, &mut right)] {
                        let selections = [Selection { scope, arm }];
                        assert_eq!(
                            checker.close_dependencies_after(
                                &source,
                                0,
                                floor,
                                needed,
                                &Context::new(&selections)
                            ),
                            Ok(())
                        );
                    }
                    let mut actual = original;
                    match checker.close_dependencies_after(
                        &source,
                        0,
                        floor,
                        &mut actual,
                        &Context::new(&[]),
                    ) {
                        Ok(()) => {
                            for event in 0..3 {
                                let expected = if event >= scope.start as usize
                                    && event < scope.split as usize
                                {
                                    left[event]
                                } else if event >= scope.split as usize
                                    && event < scope.stop as usize
                                {
                                    right[event]
                                } else {
                                    assert_eq!(left[event], right[event]);
                                    left[event]
                                };
                                assert_eq!(
                                    actual[event], expected,
                                    "shape={shape} graph={encoding} mask={mask} floor={floor}"
                                );
                            }
                            merged += 1;
                        }
                        Err(actual_scope) => {
                            assert_eq!(actual_scope, scope);
                            split += 1;
                        }
                    }
                    compared += 1;
                }
            }
        }
    }
    assert_eq!(compared, 746_496);
    assert!(merged > 0 && split > 0);
}

#[test]
fn actor_components_include_the_entire_u8_domain_and_preserve_disjoint_components() {
    let actors =
        core::array::from_fn::<_, 255, _>(|event| ((event as u16 + 1) << 8) | event as u16);
    let mut needed = [1; 255];
    needed[254] = 3;
    connected(&mut needed, &actors);
    assert_eq!(needed, [3; 255]);
    let actors = [1 << 8, 2 << 8 | 1, 255 << 8 | 254, 254 << 8 | 253];
    let mut needed = [1, 1, 3, 1];
    connected(&mut needed, &actors);
    assert_eq!(needed, [0, 0, 3, 3]);
}

#[test]
fn only_a_resource_used_by_the_return_can_obstruct_a_fresh_visit() {
    let resource = EndpointKey {
        component: 0,
        role: 10,
    };
    let outgoing = EndpointKey {
        component: 1,
        role: 20,
    };
    let unrelated = EndpointKey {
        component: 2,
        role: 20,
    };
    let control = |role| EndpointKey { component: 0, role };
    let use_resource = Source::Send(Message {
        from: resource,
        to: control(3),
        label: 0,
        payload: Payload::Wire { schema: 0 },
    });
    let give_other = Source::Send(Message {
        from: control(2),
        to: control(3),
        label: 1,
        payload: Payload::Endpoint(outgoing),
    });
    let body = Source::par(&use_resource, &give_other);
    let loan = Source::Send(Message {
        from: control(0),
        to: control(1),
        label: 2,
        payload: Payload::Endpoint(resource),
    });
    let returned = Source::Send(Message {
        from: control(1),
        to: control(0),
        label: 4,
        payload: Payload::Endpoint(resource),
    });
    for kind in 0..3 {
        for sequential in [false, true] {
            for target in [0, 1, 4] {
                let intervening = Source::Send(Message {
                    from: if kind == 0 {
                        unrelated
                    } else if kind == 1 {
                        outgoing
                    } else {
                        control(3)
                    },
                    to: control(target),
                    label: 3,
                    payload: if kind == 2 {
                        Payload::Endpoint(outgoing)
                    } else {
                        Payload::Wire { schema: 0 }
                    },
                });
                let suffix = if sequential {
                    Source::seq(&intervening, &returned)
                } else {
                    Source::par(&intervening, &returned)
                };
                let absent = Source::seq(&loan, &suffix);
                let root = Source::seq(&body, &absent);
                let messages =
                    core::array::from_fn::<_, 5, _>(|event| Some(root.message_at(event)));
                let image = ActorImage::<5, 0>::causal_fixture(
                    &[[0, 3], [2, 3], [0, 1], [3, target as u16], [1, 0]],
                    &[],
                );
                let checker = Checker::new(&root, &image, (Query::new(&root, &[], &[]), &messages));
                assert_eq!(
                    checker.temporary_departures(
                        &Body {
                            source: &body,
                            start: 0,
                            key: resource
                        },
                        0,
                        &Context::new(&[])
                    ),
                    Ok(!(kind != 0 && sequential && target != 4)),
                    "kind={kind} sequential={sequential} target={target}"
                );
            }
        }
    }
}

#[test]
fn transfer_index_preserves_every_candidate_and_conditional_return() {
    let control = |role| EndpointKey { component: 0, role };
    let keys = [
        control(10),
        EndpointKey {
            component: 1,
            role: 10,
        },
    ];
    let bindings = [[0, 1], [1, 0], [0, 1], [1, 2], [2, 0], [2, 1]];
    let scopes = [
        Scope {
            start: 1,
            split: 2,
            stop: 3,
        },
        Scope {
            start: 4,
            split: 5,
            stop: 6,
        },
    ];
    let mut compared = 0;
    // Each row independently carries either component's Endpoint or ordinary
    // data. The reference scans all rows, including both unselected arms.
    for word in 0..3usize.pow(6) {
        let mut remaining = word;
        let leaves = core::array::from_fn::<_, 6, _>(|event| {
            let payload = match remaining % 3 {
                0 => Payload::Wire { schema: 0 },
                other => Payload::Endpoint(keys[other - 1]),
            };
            remaining /= 3;
            Source::Send(Message {
                from: control(bindings[event][0] as u8),
                to: control(bindings[event][1] as u8),
                label: event as u8,
                payload,
            })
        });
        let left_choice = Source::route(&leaves[1], &leaves[2], None);
        let left = Source::seq(&leaves[0], &left_choice);
        let right_choice = Source::route(&leaves[4], &leaves[5], None);
        let right = Source::par(&leaves[3], &right_choice);
        let root = Source::seq(&left, &right);
        let messages = core::array::from_fn::<_, 6, _>(|event| Some(root.message_at(event)));
        let image = ActorImage::<6, 0>::causal_fixture(&bindings, &[]);
        let checker = Checker::new(&root, &image, (Query::new(&root, &[], &[]), &messages));
        for cut in 0..=6 {
            let expected = (cut..6)
                .filter(|&event| matches!(root.message_at(event).payload, Payload::Endpoint(_)))
                .collect::<std::vec::Vec<_>>();
            let mut indexed = std::vec::Vec::new();
            let mut event = checker.transfers_from(cut);
            while event < 6 {
                indexed.push(event);
                let next = checker.next_transfer[event] as usize;
                assert!(next > event);
                event = next;
            }
            assert_eq!(indexed, expected);
            for key in keys {
                let expected = expected
                    .iter()
                    .copied()
                    .filter(|&event| {
                        matches!(root.message_at(event).payload,
                        Payload::Endpoint(payload) if payload.same(key))
                    })
                    .collect::<std::vec::Vec<_>>();
                let mut indexed = std::vec::Vec::new();
                let mut event = checker.key_transfers_from(key, cut);
                while event < 6 {
                    indexed.push(event);
                    let next = checker.next_key_transfer[event] as usize;
                    assert!(next > event);
                    event = next;
                }
                assert_eq!(indexed, expected);
            }
        }
        for choices in 0..9 {
            let mut selections = std::vec::Vec::new();
            let mut choice = choices;
            for scope in scopes {
                match choice % 3 {
                    1 => selections.push(Selection {
                        scope,
                        arm: Arm::Left,
                    }),
                    2 => selections.push(Selection {
                        scope,
                        arm: Arm::Right,
                    }),
                    _ => (),
                }
                choice /= 3;
            }
            let path = Context::new(&selections);
            for departure in 0..6 {
                for key in keys {
                    for owner in 0..3 {
                        let mut expected = Ok(None);
                        for (event, binding) in bindings.iter().enumerate().skip(departure + 1) {
                            if binding[1] != owner as u16 {
                                continue;
                            }
                            match checker.transfer(event, key, &path) {
                                Ok(Some((_, receiver))) if receiver == owner => {
                                    expected = Ok(Some(event));
                                    break;
                                }
                                Ok(_) => (),
                                Err(scope) => {
                                    expected = Err(scope);
                                    break;
                                }
                            }
                        }
                        assert_eq!(checker.return_after(key, departure, owner, &path), expected);
                        compared += 1;
                    }
                }
            }
        }
    }
    assert_eq!(compared, 236_196);
}

#[test]
fn a_conditional_return_receiver_still_requires_its_actual_choice() {
    let key = EndpointKey {
        component: 0,
        role: 10,
    };
    let send = |payload| {
        Source::Send(Message {
            from: EndpointKey {
                component: 0,
                role: 0,
            },
            to: EndpointKey {
                component: 0,
                role: 1,
            },
            label: 0,
            payload,
        })
    };
    let prefix = send(Payload::Wire { schema: 0 });
    let returned = send(Payload::Endpoint(key));
    let source = Source::seq(&prefix, &returned);
    let scope = Scope {
        start: 10,
        split: 11,
        stop: 12,
    };
    let image = ActorImage::<2, 1>::causal_fixture(&[[0, 1], [0, 256]], &[(scope, [0, 255])]);
    let messages = [Some(source.message_at(0)), Some(source.message_at(1))];
    let checker = Checker::new(&source, &image, (Query::new(&source, &[], &[]), &messages));
    assert_eq!(
        checker.return_after(key, 0, 0, &Context::new(&[])),
        Err(scope)
    );
    for (arm, receiver) in [(Arm::Left, 0), (Arm::Right, 255)] {
        let choices = [Selection { scope, arm }];
        let path = Context::new(&choices);
        for owner in [0, 255] {
            assert_eq!(
                checker.return_after(key, 0, owner, &path),
                Ok((owner == receiver).then_some(1))
            );
        }
    }
}

fn pair<'a>(left: &'a Source<'a>, right: &'a Source<'a>, shape: u8) -> Source<'a> {
    match shape {
        0 => Source::seq(left, right),
        1 => Source::par(left, right),
        _ => Source::route(left, right, (shape == 3).then_some(7)),
    }
}

#[test]
fn indexed_branch_exploration_preserves_given_choices_and_checks_both_unknown_arms() {
    let control = |role| EndpointKey { component: 0, role };
    let key = control(10);
    let send = |from, to, payload| {
        Source::Send(Message {
            from,
            to,
            label: 1,
            payload,
        })
    };
    let loan = send(control(0), control(1), Payload::Endpoint(key));
    let back = send(control(1), control(0), Payload::Endpoint(key));
    let permit = send(key, control(0), Payload::Wire { schema: 0 });
    let tick = send(control(0), control(1), Payload::Wire { schema: 0 });
    let work = send(key, control(3), Payload::Wire { schema: 0 });
    let uninformed = Source::seq(&tick, &work);
    let informed = Source::seq(&permit, &uninformed);
    let choice = Source::route(&informed, &uninformed, None);
    let suffix = Source::seq(&choice, &back);
    let root = Source::seq(&loan, &suffix);
    let mut facts = Scratch::<8, 8>::new();
    let mut image = ActorImage::<8, 8>::new();
    super::super::compile_actor_image_into(&root, &mut facts, &mut image).unwrap();
    let checker = Checker::new(
        &root,
        &image,
        (
            Query::new(&root, &facts.symbols[..facts.symbol_len], &facts.initial),
            &facts.events[..facts.event_len],
        ),
    );
    let body = Body {
        source: &choice,
        start: 1,
        key,
    };
    let scope = Scope {
        start: 1,
        split: 4,
        stop: 6,
    };
    for (arm, expected) in [(Arm::Left, true), (Arm::Right, false)] {
        let selections = [Selection { scope, arm }];
        assert_eq!(checker.body(&body, &Context::new(&selections)), expected);
    }
    assert!(!checker.body(&body, &Context::new(&[])));
    // Reusing the same checker must not retain a branch chosen by a prior run.
    let selections = [Selection {
        scope,
        arm: Arm::Left,
    }];
    assert!(checker.body(&body, &Context::new(&selections)));
}

#[test]
fn indexed_presence_matches_source_traversal_with_unresolved_and_excluded_routes() {
    let leaves = core::array::from_fn::<_, 4, _>(|label| {
        Source::Send(Message {
            from: EndpointKey {
                component: 0,
                role: 0,
            },
            to: EndpointKey {
                component: 0,
                role: 1,
            },
            label: label as u8,
            payload: Payload::Wire { schema: 0 },
        })
    });
    let scopes = [
        Scope {
            start: 0,
            split: 2,
            stop: 4,
        },
        Scope {
            start: 0,
            split: 1,
            stop: 2,
        },
        Scope {
            start: 2,
            split: 3,
            stop: 4,
        },
    ];
    let mut compared = 0;
    for outer in 0..4 {
        for left in 0..4 {
            for right in 0..4 {
                let left = pair(&leaves[0], &leaves[1], left);
                let right = pair(&leaves[2], &leaves[3], right);
                let root = pair(&left, &right, outer);
                let root = Source::roll(&root);
                let image = ActorImage::<4, 0>::causal_fixture(&[[0, 1]; 4], &[]);
                let messages = wire_events::<4>(&root);
                let checker = Checker::wire_fixture(&root, &image, &messages);
                for choices in 0..27 {
                    let mut word = choices;
                    let mut selections = std::vec::Vec::new();
                    for scope in scopes {
                        match word % 3 {
                            1 => selections.push(Selection {
                                scope,
                                arm: Arm::Left,
                            }),
                            2 => selections.push(Selection {
                                scope,
                                arm: Arm::Right,
                            }),
                            _ => (),
                        }
                        word /= 3;
                    }
                    let path = Context::new(&selections);
                    for event in 0..4 {
                        assert_eq!(
                            checker.present_at(event, &path),
                            Checker::<4, 0>::present(&root, 0, event, &path)
                        );
                        compared += 1;
                    }
                }
            }
        }
    }
    assert_eq!(compared, 6_912);
}

#[test]
fn subtree_closure_preserves_global_offsets_and_ignores_its_external_prefix() {
    let wire = |from, to| {
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
    };
    let first = wire(0, 255);
    let second = wire(255, 1);
    let third = wire(1, 255);
    let fourth = wire(255, 0);
    let prefix = Source::seq(&first, &second);
    for shape in 0..4 {
        let body = pair(&third, &fourth, shape);
        let root = Source::seq(&prefix, &body);
        let image =
            ActorImage::<4, 0>::causal_fixture(&[[0, 255], [255, 1], [1, 255], [255, 0]], &[]);
        let messages = wire_events::<4>(&root);
        let checker = Checker::wire_fixture(&root, &image, &messages);
        for arm in [Arm::Left, Arm::Right] {
            let selections = [Selection {
                scope: Scope {
                    start: 2,
                    split: 3,
                    stop: 4,
                },
                arm,
            }];
            let path = Context::new(&selections);
            for seed in 0u8..16 {
                for floor in 2..=4 {
                    let mut actual = [0u8; 4];
                    for (event, bits) in actual.iter_mut().enumerate().skip(floor) {
                        if Checker::<4, 0>::present(&body, 2, event, &path) == Ok(true) {
                            *bits = (seed >> (2 * (event - 2))) & 3;
                        }
                    }
                    let mut expected = actual;
                    assert_eq!(
                        checker.reference_close_dependencies(&body, 2, &mut expected, &path),
                        Ok(())
                    );
                    assert_eq!(
                        checker.close_dependencies_after(&body, 2, floor, &mut actual, &path),
                        Ok(())
                    );
                    assert_eq!(actual[floor..], expected[floor..]);
                    assert_eq!(actual[..2], [0, 0]);
                }
            }
        }
    }
}

#[test]
fn a_parallel_receiver_that_holds_the_resource_couples_its_remote_origin() {
    let key = EndpointKey {
        component: 0,
        role: 10,
    };
    let wire = |from, to| {
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
    };
    let work = wire(10, 4);
    let control = wire(6, 7);
    let body = Source::par(&work, &control);
    for holder in 0..=u8::MAX {
        let image = ActorImage::<2, 0>::causal_fixture(&[[holder.into(), 4], [6, 7]], &[]);
        assert_eq!(
            Checker::wire_fixture(&body, &image, &wire_events::<2>(&body)).origins(
                &Body {
                    source: &body,
                    start: 0,
                    key
                },
                holder,
                &Context::new(&[])
            ),
            Ok(holder != 7),
            "holder={holder}"
        );
    }
}

#[test]
fn participant_coupling_is_transitive_and_does_not_supply_causal_permission() {
    let key = EndpointKey {
        component: 0,
        role: 10,
    };
    let wire = |from, to| {
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
    };
    let work = wire(10, 4);
    let first = wire(5, 6);
    let last = wire(6, 7);
    for sequential in [false, true] {
        let chain = if sequential {
            Source::seq(&first, &last)
        } else {
            Source::par(&first, &last)
        };
        let body = Source::par(&work, &chain);
        for holder in 0..=u8::MAX {
            let image =
                ActorImage::<3, 0>::causal_fixture(&[[holder.into(), 4], [5, 6], [6, 7]], &[]);
            let expected = !(5..=7).contains(&holder) || (sequential && holder == 5);
            assert_eq!(
                Checker::wire_fixture(&body, &image, &wire_events::<3>(&body)).origins(
                    &Body {
                        source: &body,
                        start: 0,
                        key
                    },
                    holder,
                    &Context::new(&[])
                ),
                Ok(expected),
                "holder={holder} sequential={sequential}"
            );
        }
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
        let checker = Checker::new(
            &root,
            &image,
            (Query::new(&root, &[], &[]), &messages[..root.event_count()]),
        );
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
        let checker = Checker::new(
            &root,
            &image,
            (
                Query::new(&root, &facts.symbols[..facts.symbol_len], &facts.initial),
                &facts.events[..facts.event_len],
            ),
        );
        let body = Body {
            source: &body,
            start: 0,
            key,
        };
        let path = Context::new(&[]);
        assert_eq!(checker.owner(key, 0, &path), Ok(0));
        assert_eq!(checker.owner(key, root.event_count(), &path), Ok(0));
        assert_eq!(checker.temporary_departures(&body, 0, &path), Ok(true));
        assert_eq!(checker.run(&body, &path, &mut (None, None)), Ok(true));
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
    let checker = Checker::new(
        &root,
        &image,
        (
            Query::new(&root, &facts.symbols[..facts.symbol_len], &facts.initial),
            &facts.events[..facts.event_len],
        ),
    );
    let body = Body {
        source: &body,
        start: 0,
        key,
    };
    let path = Context::new(&[]);
    assert_eq!(checker.owner(key, 0, &path), Ok(0));
    assert_eq!(checker.owner(key, root.event_count(), &path), Ok(0));
    assert_eq!(checker.temporary_departures(&body, 0, &path), Ok(false));
    assert_eq!(checker.run(&body, &path, &mut (None, None)), Ok(false));
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
                        for first in 0..4 {
                            for second in first..4 {
                                if Checker::<4, 2>::present(&root, 0, first, &path) != Ok(true)
                                    || Checker::<4, 2>::present(&root, 0, second, &path) != Ok(true)
                                {
                                    continue;
                                }
                                for first_side in 0..2 {
                                    for second_side in 0..2 {
                                        let expected =
                                            if first == second && first_side >= second_side {
                                                false
                                            } else {
                                                let mut reference = [0; 4];
                                                reference[second] = 1 << second_side;
                                                checker
                                                    .reference_close_dependencies(
                                                        &root,
                                                        0,
                                                        &mut reference,
                                                        &path,
                                                    )
                                                    .unwrap();
                                                reference[first] & (1 << first_side) != 0
                                            };
                                        assert_eq!(
                                            checker.causally_before(
                                                (first, first_side),
                                                (second, second_side),
                                                &path
                                            ),
                                            Ok(expected)
                                        );
                                    }
                                }
                            }
                        }
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
                                if floor == 0 {
                                    for owner in [0, 1, 255] {
                                        let expected = (0..4).all(|event| {
                                            if reference[event] & 1 == 0 {
                                                return true;
                                            }
                                            let actor = checker.side(event, 0, &path).unwrap();
                                            actor == owner
                                                || (0..event).any(|prior| {
                                                    reference[prior] & 2 != 0
                                                        && root.before(prior, event)
                                                        && checker.side(prior, 1, &path)
                                                            == Ok(actor)
                                                })
                                        });
                                        assert_eq!(
                                            checker.permitted_origins(
                                                &Body {
                                                    source: &root,
                                                    start: 0,
                                                    key
                                                },
                                                owner,
                                                actual,
                                                &path,
                                            ),
                                            Ok(expected),
                                            "owner={owner} shapes={left_shape}/{right_shape}/{root_shape} choices={choices} seed={seed}"
                                        );
                                    }
                                }
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
