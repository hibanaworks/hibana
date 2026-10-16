//! Host-only exporter of source terms and actual Rust projection results.
use hibana_delegation_model::{
    ActionId, Binding, EndpointKey, Issuance, Message, Payload, Plan, Scratch, Side, Source,
    compile_into,
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
            &Source::Seq(&wire(SERVER, A, 4), &transfer(A, P, 5, DB)),
        ),
    ),
);
const DB_ROUND: Source<'static> = Source::Seq(
    &transfer(P, A, 1, DB),
    &Source::Seq(&wire(DB, SERVER, 2), &transfer(A, P, 3, DB)),
);
const AUTH_ROUND: Source<'static> = Source::Seq(
    &transfer(P, B, 4, AUTH),
    &Source::Seq(&wire(AUTH, SERVER, 5), &transfer(B, P, 6, AUTH)),
);
const ALTERNATE_ROUND: Source<'static> = Source::Seq(
    &transfer(P, B, 4, DB),
    &Source::Seq(&wire(DB, SERVER, 5), &transfer(B, P, 6, DB)),
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

const CASES: &[(&str, &Source<'static>)] = &[
    ("handoff-work-control-return", &EXCHANGE),
    (
        "parallel-closed-loans",
        &Source::Roll(&Source::Par(
            &Source::Roll(&DB_ROUND),
            &Source::Roll(&AUTH_ROUND),
        )),
    ),
    (
        "choice-closed",
        &Source::Seq(
            &Source::Route {
                left: &DB_ROUND,
                right: &ALTERNATE_ROUND,
                resolver: Some(701),
            },
            &wire(DB, SERVER, 7),
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
                ),
            ),
        ),
    ),
    (
        "nested-carrier",
        &Source::Seq(
            &transfer(P, B, 1, DB),
            &Source::Seq(&transfer(A, B, 2, AUTH), &transfer(AUTH, P, 3, DB)),
        ),
    ),
    (
        "early-resource-work",
        &Source::Seq(
            &wire(DB, SERVER, 1),
            &Source::Seq(&transfer(P, A, 2, DB), &wire(DB, SERVER, 3)),
        ),
    ),
    (
        "stable-parallel-resource",
        &Source::Seq(
            &transfer(P, A, 1, DB),
            &Source::Seq(
                &Source::Par(&wire(DB, SERVER, 2), &wire(DB, B, 3)),
                &transfer(A, P, 4, DB),
            ),
        ),
    ),
    (
        "same-actor-two-views",
        &Source::Seq(&transfer(P, A, 1, DB), &wire(DB, A, 2)),
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
        &Source::Par(&transfer(P, A, 1, DB), &transfer(P, B, 2, DB)),
    ),
    (
        "parallel-move-and-use",
        &Source::Par(&transfer(P, A, 1, DB), &wire(DB, SERVER, 2)),
    ),
    ("roll-owner-drift", &Source::Roll(&transfer(P, A, 1, DB))),
    (
        "wrong-return-owner",
        &Source::Seq(&transfer(P, A, 1, DB), &transfer(B, P, 2, DB)),
    ),
    (
        "unrooted-issuance",
        &Source::Seq(
            &transfer(DB, AUTH, 1, THIRD),
            &Source::Seq(&transfer(AUTH, THIRD, 2, DB), &transfer(THIRD, DB, 3, AUTH)),
        ),
    ),
    (
        "terminal-choice-different-owners",
        &Source::Route {
            left: &transfer(P, A, 1, DB),
            right: &transfer(P, B, 2, DB),
            resolver: None,
        },
    ),
    (
        "different-owners-unrelated-continuation",
        &Source::Seq(
            &Source::Route {
                left: &transfer(P, A, 1, DB),
                right: &transfer(P, B, 2, DB),
                resolver: Some(701),
            },
            &wire(A, B, 3),
        ),
    ),
    (
        "different-owners-live-continuation",
        &Source::Seq(
            &Source::Route {
                left: &transfer(P, A, 1, DB),
                right: &transfer(P, B, 2, DB),
                resolver: None,
            },
            &wire(DB, SERVER, 3),
        ),
    ),
    (
        "different-owners-next-visit",
        &Source::Roll(&Source::Route {
            left: &transfer(P, A, 1, DB),
            right: &transfer(P, B, 2, DB),
            resolver: None,
        }),
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
                },
            ),
            &wire(SERVER, DB, 4),
        ),
    ),
    (
        "different-owners-later-owned-payload",
        &Source::Seq(
            &Source::Route {
                left: &transfer(P, A, 1, DB),
                right: &transfer(P, B, 2, DB),
                resolver: None,
            },
            &transfer(A, P, 3, DB),
        ),
    ),
    ("self-owned-carrier", &transfer(DB, A, 1, DB)),
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
        Source::Seq(left, right) | Source::Par(left, right) | Source::Route { left, right, .. } => {
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
        Source::Roll(body) => {
            output.push_str(".roll (");
            source(output, body);
            output.push(')');
        }
    }
}

fn normalization(output: &mut String, issuance: &[Issuance], bindings: &[Binding]) {
    output.push_str("{ issuance := [");
    for (index, value) in issuance.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str("{ endpoint := ");
        key(output, value.endpoint);
        write!(output, ", actor := {} }}", value.actor).unwrap();
    }
    output.push_str("], bindings := [");
    for (index, value) in bindings.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        output.push_str("{ message := ");
        message(output, value.message);
        write!(
            output,
            ", senderActor := {}, receiverActor := {} }}",
            value.sender_actor, value.receiver_actor
        )
        .unwrap();
    }
    output.push_str("] }");
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
        "import Projection\n\nset_option maxRecDepth 4096\nset_option maxHeartbeats 10000000\nnamespace Hibana.EndpointDelegation.Generated\n\n",
    );
    let mut scratch = Scratch::<SYMBOLS, EVENTS>::new();
    let mut accepted = 0;
    let mut rejected = 0;
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
        let bindings: Vec<_> = (0..plan.len())
            .map(|event| plan.binding(event).unwrap())
            .collect();
        let mut issuance: Vec<_> = (0..SYMBOLS).filter_map(|row| plan.issuance(row)).collect();
        issuance.sort_by_key(|value| (value.endpoint.component, value.endpoint.role));
        writeln!(output, "def rust_{index} : Normalization :=").unwrap();
        normalization(&mut output, &issuance, &bindings);
        writeln!(
            output,
            "\ntheorem case_{index} : normalize source_{index} = some rust_{index} := by decide"
        )
        .unwrap();
        theorems.push(format!("case_{index}"));
        let domain = 3 * (input.event_count() + 1);
        let mut order = Vec::new();
        for before in 0..domain {
            for after in 0..domain {
                if plan.precedes(action(before), action(after)) {
                    order.push((before, after));
                }
            }
        }
        writeln!(
            output,
            "theorem order_{index} : orderEdges source_{index} rust_{index}.bindings ="
        )
        .unwrap();
        edges(&mut output, &order);
        output.push_str(" := by decide\n");
        theorems.push(format!("order_{index}"));
        let mut wrong_actor = bindings.clone();
        wrong_actor[0].sender_actor = wrong_actor[0].sender_actor.wrapping_add(1);
        writeln!(
            output,
            "theorem actor_mutant_{index} : normalize source_{index} ≠ some"
        )
        .unwrap();
        normalization(&mut output, &issuance, &wrong_actor);
        output.push_str(" := by decide\n");
        theorems.push(format!("actor_mutant_{index}"));
        let mut wrong_wire = bindings.clone();
        wrong_wire[0].message.from.component = wrong_wire[0].message.from.component.wrapping_add(1);
        writeln!(
            output,
            "theorem wire_mutant_{index} : normalize source_{index} ≠ some"
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
                "theorem issuer_mutant_{index} : normalize source_{index} ≠ some"
            )
            .unwrap();
            normalization(&mut output, &wrong_issuer, &bindings);
            output.push_str(" := by decide\n");
            theorems.push(format!("issuer_mutant_{index}"));
        }
        if !order.is_empty() {
            let mut missing_edge = order.clone();
            missing_edge.remove(0);
            writeln!(
                output,
                "theorem order_mutant_{index} : orderEdges source_{index} rust_{index}.bindings ≠"
            )
            .unwrap();
            edges(&mut output, &missing_edge);
            output.push_str(" := by decide\n");
            theorems.push(format!("order_mutant_{index}"));
        }
        output.push('\n');
    }
    for name in &theorems {
        writeln!(output, "#print axioms {name}").unwrap();
    }
    output.push_str("\nend Hibana.EndpointDelegation.Generated\n");
    fs::write(destination, output)?;
    println!(
        "delegation model export: cases={} accepted={accepted} rejected={rejected} kernel-theorems={}",
        CASES.len(),
        theorems.len()
    );
    Ok(())
}
