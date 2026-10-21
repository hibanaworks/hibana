//! Host-only exporter of source terms and actual Rust projection results.
use hibana_delegation_model::{
    ActionId, Arm, Binding, EndpointKey, Issuance, Message, Payload, Plan, Scope, Scratch,
    Selection, Side, Source, compile_into,
};
use std::{fmt::Write, fs, path::PathBuf};

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
const SERVER: EndpointKey = EndpointKey {
    component: 0,
    role: 3,
};
const DB: EndpointKey = EndpointKey {
    component: 0,
    role: 10,
};
const AUTH: EndpointKey = EndpointKey {
    component: 0,
    role: 11,
};
const THIRD: EndpointKey = EndpointKey {
    component: 0,
    role: 12,
};

const fn wire(from: EndpointKey, to: EndpointKey, label: u8) -> Source<'static> {
    Source::Send(Message {
        from,
        to,
        label,
        payload: Payload::Wire { schema: 41 },
    })
}
const fn transfer(
    from: EndpointKey,
    to: EndpointKey,
    label: u8,
    key: EndpointKey,
) -> Source<'static> {
    Source::Send(Message {
        from,
        to,
        label,
        payload: Payload::Endpoint(key),
    })
}

const EXCHANGE: Source<'static> = Source::Seq(
    &transfer(P, A, 1, DB),
    &Source::Seq(
        &wire(DB, SERVER, 2),
        &Source::Seq(
            &wire(SERVER, DB, 3),
            &Source::Seq(&wire(SERVER, A, 4), &transfer(A, P, 5, DB), 2),
            3,
        ),
        4,
    ),
    5,
);
const DB_ROUND: Source<'static> = Source::Seq(
    &transfer(P, A, 1, DB),
    &Source::Seq(&wire(DB, SERVER, 2), &transfer(A, P, 3, DB), 2),
    3,
);
const AUTH_ROUND: Source<'static> = Source::Seq(
    &transfer(P, B, 4, AUTH),
    &Source::Seq(&wire(AUTH, SERVER, 5), &transfer(B, P, 6, AUTH), 2),
    3,
);
const ALTERNATE_ROUND: Source<'static> = Source::Seq(
    &transfer(P, B, 4, DB),
    &Source::Seq(&wire(DB, SERVER, 5), &transfer(B, P, 6, DB), 2),
    3,
);
const CROSS_DB: EndpointKey = EndpointKey {
    component: 7,
    role: DB.role,
};
const CROSS_SERVER: EndpointKey = EndpointKey {
    component: 7,
    role: SERVER.role,
};
const CONTROL_A: EndpointKey = EndpointKey {
    component: 9,
    role: A.role,
};
const CONTROL_SERVER: EndpointKey = EndpointKey {
    component: 9,
    role: SERVER.role,
};

const PAIRED_LEFT: Source<'static> =
    Source::Seq(&transfer(P, A, 1, DB), &transfer(P, A, 2, AUTH), 2);
const PAIRED_RIGHT: Source<'static> =
    Source::Seq(&transfer(P, B, 3, DB), &transfer(P, B, 4, AUTH), 2);
const PAIRED: Source<'static> = Source::Route {
    left: &PAIRED_LEFT,
    right: &PAIRED_RIGHT,
    resolver: Some(701),
    events: PAIRED_LEFT.event_count() + PAIRED_RIGHT.event_count(),
};

const CASES: &[(&str, &Source<'static>)] = &[
    (
        "owned-receive-teaches-holder",
        &Source::Route {
            left: &Source::Seq(&transfer(P, A, 1, DB), &wire(DB, SERVER, 3), 2),
            right: &Source::Seq(&transfer(P, A, 2, DB), &wire(DB, SERVER, 4), 2),
            resolver: None,
            events: 4,
        },
    ),
    (
        "parallel-sibling-cannot-teach",
        &Source::Seq(
            &transfer(P, A, 1, DB),
            &Source::Route {
                left: &Source::Par(&wire(P, A, 2), &wire(DB, SERVER, 4), 2),
                right: &Source::Par(&wire(P, A, 3), &wire(DB, SERVER, 5), 2),
                resolver: None,
                events: 4,
            },
            5,
        ),
    ),
    (
        "conditional-holder-controls-later-choice",
        &Source::Seq(
            &Source::Route {
                left: &Source::Seq(&transfer(P, A, 1, DB), &wire(P, B, 2), 2),
                right: &Source::Seq(&transfer(P, B, 3, DB), &wire(P, A, 4), 2),
                resolver: None,
                events: 4,
            },
            &Source::Route {
                left: &wire(DB, SERVER, 5),
                right: &wire(DB, SERVER, 6),
                resolver: None,
                events: 2,
            },
            6,
        ),
    ),
    (
        "equal-unions-different-actual-participants",
        &Source::Seq(
            &Source::Route {
                left: &Source::Seq(&transfer(P, A, 1, DB), &transfer(P, B, 2, AUTH), 2),
                right: &Source::Seq(&transfer(P, B, 3, DB), &transfer(P, A, 4, AUTH), 2),
                resolver: None,
                events: 4,
            },
            &Source::Seq(
                &Source::Route {
                    left: &wire(P, DB, 5),
                    right: &wire(P, AUTH, 6),
                    resolver: None,
                    events: 2,
                },
                &wire(A, SERVER, 7),
                3,
            ),
            7,
        ),
    ),
    ("handoff-work-control-return", &EXCHANGE),
    (
        "parallel-closed-loans",
        &Source::Roll(
            &Source::Par(
                &Source::Roll(&DB_ROUND, DB_ROUND.event_count()),
                &Source::Roll(&AUTH_ROUND, AUTH_ROUND.event_count()),
                DB_ROUND.event_count() + AUTH_ROUND.event_count(),
            ),
            DB_ROUND.event_count() + AUTH_ROUND.event_count(),
        ),
    ),
    (
        "choice-closed",
        &Source::Seq(
            &Source::Route {
                left: &DB_ROUND,
                right: &ALTERNATE_ROUND,
                resolver: Some(701),
                events: DB_ROUND.event_count() + ALTERNATE_ROUND.event_count(),
            },
            &wire(DB, SERVER, 7),
            DB_ROUND.event_count() + ALTERNATE_ROUND.event_count() + 1,
        ),
    ),
    (
        "component-boundary",
        &Source::Seq(
            &transfer(P, A, 1, CROSS_DB),
            &Source::Seq(
                &wire(CROSS_DB, CROSS_SERVER, 2),
                &Source::Seq(
                    &wire(CONTROL_SERVER, CONTROL_A, 3),
                    &transfer(A, P, 4, CROSS_DB),
                    2,
                ),
                3,
            ),
            4,
        ),
    ),
    (
        "nested-carrier",
        &Source::Seq(
            &transfer(P, B, 1, DB),
            &Source::Seq(&transfer(A, B, 2, AUTH), &transfer(AUTH, P, 3, DB), 2),
            3,
        ),
    ),
    (
        "early-resource-work",
        &Source::Seq(
            &wire(DB, SERVER, 1),
            &Source::Seq(&transfer(P, A, 2, DB), &wire(DB, SERVER, 3), 2),
            3,
        ),
    ),
    (
        "stable-parallel-resource",
        &Source::Seq(
            &transfer(P, A, 1, DB),
            &Source::Seq(
                &Source::Par(&wire(DB, SERVER, 2), &wire(DB, B, 3), 2),
                &transfer(A, P, 4, DB),
                3,
            ),
            4,
        ),
    ),
    (
        "same-actor-two-views",
        &Source::Seq(&transfer(P, A, 1, DB), &wire(DB, A, 2), 2),
    ),
    (
        "local-unit",
        &Source::Send(Message {
            from: P,
            to: P,
            label: 1,
            payload: Payload::Wire { schema: 0 },
        }),
    ),
    (
        "parallel-double-grant",
        &Source::Par(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB), 2),
    ),
    (
        "parallel-move-and-use",
        &Source::Par(&transfer(P, A, 1, DB), &wire(DB, SERVER, 2), 2),
    ),
    ("roll-owner-drift", &Source::Roll(&transfer(P, A, 1, DB), 1)),
    (
        "wrong-return-owner",
        &Source::Seq(&transfer(P, A, 1, DB), &transfer(B, P, 2, DB), 2),
    ),
    (
        "unrooted-issuance",
        &Source::Seq(
            &transfer(DB, AUTH, 1, THIRD),
            &Source::Seq(
                &transfer(AUTH, THIRD, 2, DB),
                &transfer(THIRD, DB, 3, AUTH),
                2,
            ),
            3,
        ),
    ),
    (
        "terminal-choice-different-owners",
        &Source::Route {
            left: &transfer(P, A, 1, DB),
            right: &transfer(P, B, 2, DB),
            resolver: None,
            events: 2,
        },
    ),
    (
        "different-owners-unrelated-continuation",
        &Source::Seq(
            &Source::Route {
                left: &transfer(P, A, 1, DB),
                right: &transfer(P, B, 2, DB),
                resolver: Some(701),
                events: 2,
            },
            &wire(A, B, 3),
            3,
        ),
    ),
    (
        "different-owners-live-continuation",
        &Source::Seq(
            &Source::Route {
                left: &transfer(P, A, 1, DB),
                right: &transfer(P, B, 2, DB),
                resolver: None,
                events: 2,
            },
            &wire(DB, SERVER, 3),
            3,
        ),
    ),
    (
        "different-owners-next-visit",
        &Source::Roll(
            &Source::Route {
                left: &transfer(P, A, 1, DB),
                right: &transfer(P, B, 2, DB),
                resolver: None,
                events: 2,
            },
            2,
        ),
    ),
    (
        "different-owners-nested-continuation",
        &Source::Seq(
            &Source::Seq(
                &wire(P, A, 3),
                &Source::Route {
                    left: &transfer(P, A, 1, DB),
                    right: &transfer(P, B, 2, DB),
                    resolver: None,
                    events: 2,
                },
                3,
            ),
            &wire(SERVER, DB, 4),
            4,
        ),
    ),
    (
        "different-owners-later-owned-payload",
        &Source::Seq(
            &Source::Route {
                left: &transfer(P, A, 1, DB),
                right: &transfer(P, B, 2, DB),
                resolver: None,
                events: 2,
            },
            &transfer(A, P, 3, DB),
            3,
        ),
    ),
    ("self-owned-carrier", &transfer(DB, A, 1, DB)),
    (
        "correlated-resources-return-through-current-carrier",
        &Source::Seq(
            &PAIRED,
            &Source::Seq(&transfer(AUTH, P, 5, DB), &wire(DB, SERVER, 6), 2),
            PAIRED.event_count() + 2,
        ),
    ),
    (
        "independent-parallel-owner-choices",
        &Source::Seq(
            &Source::Par(
                &Source::Route {
                    left: &transfer(P, A, 1, DB),
                    right: &transfer(P, B, 2, DB),
                    resolver: None,
                    events: 2,
                },
                &Source::Route {
                    left: &transfer(P, A, 3, AUTH),
                    right: &transfer(P, B, 4, AUTH),
                    resolver: None,
                    events: 2,
                },
                4,
            ),
            &Source::Par(&wire(DB, SERVER, 5), &wire(AUTH, CONTROL_SERVER, 6), 2),
            6,
        ),
    ),
    (
        "conditional-loop-entry-owner",
        &Source::Seq(
            &PAIRED,
            &Source::Roll(
                &Source::Seq(
                    &wire(DB, SERVER, 5),
                    &Source::Seq(&transfer(AUTH, P, 6, DB), &transfer(P, AUTH, 7, DB), 2),
                    3,
                ),
                3,
            ),
            PAIRED.event_count() + 3,
        ),
    ),
    (
        "no-branch-dependent-initial-issuer",
        &Source::Seq(
            &Source::Route {
                left: &transfer(P, A, 1, DB),
                right: &transfer(P, B, 2, DB),
                resolver: None,
                events: 2,
            },
            &transfer(DB, P, 3, AUTH),
            3,
        ),
    ),
];

const fn max_events() -> usize {
    let mut count = 0;
    let mut index = 0;
    while index < CASES.len() {
        let current = CASES[index].1.event_count();
        if count < current {
            count = current;
        }
        index += 1;
    }
    count
}
const EVENTS: usize = max_events();
const SYMBOLS: usize = 3 * EVENTS;

fn key(output: &mut String, key: EndpointKey) {
    write!(
        output,
        "{{ component := {}, role := {} }}",
        key.component, key.role
    )
    .unwrap();
}

fn message(output: &mut String, value: Message) {
    output.push_str("{ sender := ");
    key(output, value.from);
    output.push_str(", receiver := ");
    key(output, value.to);
    write!(output, ", label := {}, payload := ", value.label).unwrap();
    match value.payload {
        Payload::Wire { schema } => write!(output, ".wire {schema}").unwrap(),
        Payload::Endpoint(value) => {
            output.push_str(".endpoint ");
            key(output, value);
        }
    }
    output.push_str(" }");
}

fn source(output: &mut String, value: &Source<'_>) {
    match value {
        Source::Send(value) => {
            output.push_str(".send ");
            message(output, *value);
        }
        Source::Seq(left, right, _)
        | Source::Par(left, right, _)
        | Source::Route { left, right, .. } => {
            match value {
                Source::Seq(..) => output.push_str(".seq"),
                Source::Par(..) => output.push_str(".par"),
                Source::Route { resolver, .. } => {
                    output.push_str(".route ");
                    match resolver {
                        Some(value) => write!(output, "(some {value})").unwrap(),
                        None => output.push_str("none"),
                    }
                }
                _ => unreachable!(),
            }
            output.push_str(" (");
            source(output, left);
            output.push_str(") (");
            source(output, right);
            output.push(')');
        }
        Source::Roll(body, _) => {
            output.push_str(".roll (");
            source(output, body);
            output.push(')');
        }
    }
}

#[derive(Clone)]
struct PathArtifact {
    selections: Vec<Selection>,
    bindings: Vec<(usize, Binding)>,
    order: Vec<(usize, usize)>,
}

fn write_selections(output: &mut String, values: &[Selection]) {
    output.push('[');
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        let arm = match value.arm {
            Arm::Left => "left",
            Arm::Right => "right",
        };
        write!(
            output,
            "{{ scope := {{ start := {}, split := {}, stop := {} }}, arm := .{arm} }}",
            value.scope.start, value.scope.split, value.scope.stop
        )
        .unwrap();
    }
    output.push(']');
}

fn write_path(output: &mut String, path: &PathArtifact) {
    output.push_str("{ selections := ");
    write_selections(output, &path.selections);
    output.push_str(", bindings := [");
    for (index, (event, binding)) in path.bindings.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        write!(output, "({event}, {{ message := ").unwrap();
        message(output, binding.message);
        write!(
            output,
            ", senderActor := {}, receiverActor := {} }})",
            binding.sender_actor, binding.receiver_actor
        )
        .unwrap();
    }
    output.push_str("] }");
}

fn normalization(output: &mut String, issuance: &[Issuance], paths: &[PathArtifact]) {
    output.push_str("{ issuance := [");
    for (index, value) in issuance.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str("{ endpoint := ");
        key(output, value.endpoint);
        write!(output, ", actor := {} }}", value.actor).unwrap();
    }
    output.push_str("], paths := [");
    for (index, path) in paths.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        write_path(output, path);
    }
    output.push_str("] }");
}

fn collect_scopes(source: &Source<'_>, start: usize, output: &mut Vec<Scope>) {
    match source {
        Source::Send(_) => {}
        Source::Seq(left, right, _)
        | Source::Par(left, right, _)
        | Source::Route { left, right, .. } => {
            let split = start + left.event_count();
            if matches!(source, Source::Route { .. }) {
                output.push(Scope {
                    start: start.try_into().unwrap(),
                    split: split.try_into().unwrap(),
                    stop: (split + right.event_count()).try_into().unwrap(),
                });
            }
            collect_scopes(left, start, output);
            collect_scopes(right, split, output);
        }
        Source::Roll(body, _) => collect_scopes(body, start, output),
    }
}

fn all_paths(scopes: &[Scope], prefix: &mut Vec<Selection>, output: &mut Vec<Vec<Selection>>) {
    let Some((scope, rest)) = scopes.split_first() else {
        output.push(prefix.clone());
        return;
    };
    for arm in [Arm::Left, Arm::Right] {
        prefix.push(Selection { scope: *scope, arm });
        all_paths(rest, prefix, output);
        prefix.pop();
    }
}

fn edges(output: &mut String, values: &[(usize, usize)]) {
    output.push('[');
    for (index, (left, right)) in values.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        write!(output, "({left}, {right})").unwrap();
    }
    output.push(']');
}

fn action(encoded: usize) -> ActionId {
    ActionId {
        event: encoded / 3,
        side: match encoded % 3 {
            0 => Side::Send,
            1 => Side::Receive,
            _ => Side::Local,
        },
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("expected output .lean path")?;
    let mut output = String::from(
        "import Knowledge\n\nset_option maxRecDepth 4096\nset_option maxHeartbeats 10000000\nnamespace Hibana.EndpointDelegation.Generated\n\n",
    );
    let mut scratch = Scratch::<SYMBOLS, EVENTS>::new();
    let mut accepted = 0;
    let mut rejected = 0;
    let mut path_count = 0;
    let mut theorems = Vec::new();
    for (index, (name, input)) in CASES.iter().enumerate() {
        writeln!(output, "-- {name}\ndef source_{index} : Source :=").unwrap();
        source(&mut output, input);
        output.push('\n');
        let mut plan = Plan::new();
        if compile_into(input, &mut scratch, &mut plan).is_err() {
            writeln!(
                output,
                "theorem case_{index} : normalize source_{index} = none := by decide\n"
            )
            .unwrap();
            theorems.push(format!("case_{index}"));
            rejected += 1;
            continue;
        }
        accepted += 1;
        let mut scopes = Vec::new();
        collect_scopes(input, 0, &mut scopes);
        let mut paths = Vec::new();
        all_paths(&scopes, &mut Vec::new(), &mut paths);
        let mut artifacts = Vec::new();
        let domain = 3 * (input.event_count() + 1);
        for selections in paths {
            let bindings: Vec<_> = (0..plan.len())
                .filter_map(|event| {
                    plan.binding_in(event, &selections)
                        .map(|binding| (event, binding))
                })
                .collect();
            let mut order = Vec::new();
            for before in 0..domain {
                for after in 0..domain {
                    if plan.precedes_in(action(before), action(after), &selections) {
                        order.push((before, after));
                    }
                }
            }
            artifacts.push(PathArtifact {
                selections,
                bindings,
                order,
            });
        }
        path_count += artifacts.len();
        let mut issuance: Vec<_> = (0..SYMBOLS).filter_map(|row| plan.issuance(row)).collect();
        issuance.sort_by_key(|value| (value.endpoint.component, value.endpoint.role));
        writeln!(output, "def rust_{index} : Normalization :=").unwrap();
        normalization(&mut output, &issuance, &artifacts);
        writeln!(
            output,
            "\ntheorem case_{index} : normalize source_{index} = some rust_{index} := by decide"
        )
        .unwrap();
        theorems.push(format!("case_{index}"));
        writeln!(
            output,
            "theorem knowledge_{index} : branchKnowledge source_{index} rust_{index} = {} := by decide",
            plan.has_branch_knowledge()
        )
        .unwrap();
        theorems.push(format!("knowledge_{index}"));
        for (path_index, artifact) in artifacts.iter().enumerate() {
            writeln!(
                output,
                "def rust_{index}_path_{path_index} : NormalizedPath :="
            )
            .unwrap();
            write_path(&mut output, artifact);
            writeln!(output, "\ntheorem order_{index}_{path_index} : orderEdges source_{index} rust_{index}_path_{path_index}.bindings =").unwrap();
            edges(&mut output, &artifact.order);
            output.push_str(" := by decide\n");
            theorems.push(format!("order_{index}_{path_index}"));
            if !artifact.order.is_empty() {
                let mut missing = artifact.order.clone();
                missing.remove(0);
                writeln!(output, "theorem order_mutant_{index}_{path_index} : orderEdges source_{index} rust_{index}_path_{path_index}.bindings \u{2260}").unwrap();
                edges(&mut output, &missing);
                output.push_str(" := by decide\n");
                theorems.push(format!("order_mutant_{index}_{path_index}"));
            }
        }
        let mut wrong_actor = artifacts.clone();
        wrong_actor[0].bindings[0].1.sender_actor =
            wrong_actor[0].bindings[0].1.sender_actor.wrapping_add(1);
        writeln!(
            output,
            "theorem actor_mutant_{index} : normalize source_{index} \u{2260} some"
        )
        .unwrap();
        normalization(&mut output, &issuance, &wrong_actor);
        output.push_str(" := by decide\n");
        theorems.push(format!("actor_mutant_{index}"));

        let mut wrong_wire = artifacts.clone();
        wrong_wire[0].bindings[0].1.message.from.component = wrong_wire[0].bindings[0]
            .1
            .message
            .from
            .component
            .wrapping_add(1);
        writeln!(
            output,
            "theorem wire_mutant_{index} : normalize source_{index} \u{2260} some"
        )
        .unwrap();
        normalization(&mut output, &issuance, &wrong_wire);
        output.push_str(" := by decide\n");
        theorems.push(format!("wire_mutant_{index}"));

        if !issuance.is_empty() {
            let mut wrong_issuer = issuance.clone();
            wrong_issuer[0].actor = wrong_issuer[0].actor.wrapping_add(1);
            writeln!(
                output,
                "theorem issuer_mutant_{index} : normalize source_{index} \u{2260} some"
            )
            .unwrap();
            normalization(&mut output, &wrong_issuer, &artifacts);
            output.push_str(" := by decide\n");
            theorems.push(format!("issuer_mutant_{index}"));
        }
        if !artifacts[0].selections.is_empty() {
            let mut wrong_selection = artifacts.clone();
            wrong_selection[0].selections[0].arm = match wrong_selection[0].selections[0].arm {
                Arm::Left => Arm::Right,
                Arm::Right => Arm::Left,
            };
            writeln!(
                output,
                "theorem selection_mutant_{index} : normalize source_{index} \u{2260} some"
            )
            .unwrap();
            normalization(&mut output, &issuance, &wrong_selection);
            output.push_str(" := by decide\n");
            theorems.push(format!("selection_mutant_{index}"));
        }
        output.push('\n');
    }
    for name in &theorems {
        writeln!(output, "#print axioms {name}").unwrap();
    }
    output.push_str("\nend Hibana.EndpointDelegation.Generated\n");
    fs::write(destination, output)?;
    println!(
        "delegation model export: cases={} accepted={accepted} rejected={rejected} paths={path_count} kernel-theorems={}",
        CASES.len(),
        theorems.len()
    );
    Ok(())
}
