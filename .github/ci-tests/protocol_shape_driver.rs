//! Shared replay engine for generated tests; contains no application protocol.
use hibana::{
    Endpoint, g,
    runtime::{
        SessionKitStorage,
        ids::SessionId,
        program::{RoleProgram, project},
        resolver::{DecisionArm, ResolverRef},
        transport::EndpointBinding,
    },
};
use in_memory::InMemoryTransport;

#[derive(Clone)]
enum Decisions {
    Constant(u32),
    ByMessage(Vec<std::cell::Cell<Option<bool>>>),
    AfterPrefix {
        choice: u32,
        values: Vec<std::cell::Cell<bool>>,
    },
}
impl Decisions {
    fn choose(&self, id: usize) -> DecisionArm {
        let right = match self {
            Self::Constant(choice) => choice & (1 << id) != 0,
            Self::ByMessage(choices) => choices[id].get().unwrap_or_else(|| {
                panic!("resolver {id} invoked outside the message's source path")
            }),
            Self::AfterPrefix { values, .. } => values[id].get(),
        };
        if right {
            DecisionArm::Right
        } else {
            DecisionArm::Left
        }
    }
    fn prepare(&self, scopes: &[u16], message: Message, probe: bool) {
        if let Self::ByMessage(choices) = self {
            for id in scopes {
                choices[usize::from(*id)].set(None);
            }
            for (id, arm) in message.decisions {
                choices[usize::from(*id)].set(Some(*arm));
            }
        }
        if let Self::AfterPrefix { choice, values } = self {
            for id in scopes {
                values[usize::from(*id)].set(choice & (1 << id) != 0);
            }
            if !probe {
                for (id, right) in message.decisions {
                    values[usize::from(*id)].set(*right);
                }
            }
        }
    }
}

struct Programs {
    p0: RoleProgram<0>,
    p1: RoleProgram<1>,
    p2: RoleProgram<2>,
    p3: Option<RoleProgram<3>>,
    third_actor: bool,
    owner: u8,
    resolver_roles: &'static [u8],
}
#[derive(Clone, Copy, Debug)]
struct Message {
    from: u8,
    to: u8,
    label: u8,
    owned: bool,
    decisions: &'static [(u16, bool)],
}
#[derive(Clone, Copy, Debug)]
struct Command {
    message: Message,
    receive: bool,
    preview: bool,
}
#[derive(Debug, PartialEq)]
enum Outcome {
    Accepted,
    Pending,
    Rejected,
    Unavailable,
}

macro_rules! receive {
    ($endpoint:expr, $message:ty, $command:expr) => {
        async {
            let endpoint = $endpoint;
            if $command.preview {
                let branch = endpoint.offer().await?;
                assert_eq!(branch.label(), $command.message.label);
                drop(branch);
                let branch = endpoint.offer().await?;
                assert_eq!(branch.label(), $command.message.label);
                branch.recv::<$message>().await
            } else {
                endpoint.recv::<$message>().await
            }
        }
    };
}

async fn replay(
    programs: &Programs,
    resolver_count: u16,
    decisions: &Decisions,
    commands: &[Command],
) -> Outcome {
    let sid = SessionId::new(1);
    let network = InMemoryTransport::new(programs.p3.as_ref().map(|_| {
        // SAFETY: this replay owns the resource and its only initial issuer.
        ((sid, 3), unsafe { EndpointBinding::new() })
    }));
    let mut slabs = [
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
    ];
    let [slab0, slab1, slab2] = &mut slabs;
    let mut storage0 = SessionKitStorage::uninit();
    let mut storage1 = SessionKitStorage::uninit();
    let mut storage2 = SessionKitStorage::uninit();
    let rv0 = storage0.init().rendezvous(slab0, network.clone()).unwrap();
    let rv1 = storage1.init().rendezvous(slab1, network.clone()).unwrap();
    let rv2 = storage2.init().rendezvous(slab2, network).unwrap();
    macro_rules! register {
        ($id:literal) => {
            if resolver_count > $id {
                let resolver = ResolverRef::<$id>::decision_state(decisions, |c| Ok(c.choose($id)));
                if programs.resolver_roles[$id] == 3 {
                    let program = programs.p3.as_ref().unwrap();
                    rv0.set_resolver(program, resolver).unwrap();
                    rv1.set_resolver(
                        program,
                        ResolverRef::<$id>::decision_state(decisions, |c| Ok(c.choose($id))),
                    )
                    .unwrap();
                } else if programs.owner == 0 {
                    rv0.set_resolver(&programs.p0, resolver).unwrap();
                } else {
                    rv1.set_resolver(&programs.p1, resolver).unwrap();
                }
            }
        };
    }
    register_resolvers!();
    let mut e0 = rv0.enter(sid, &programs.p0).unwrap();
    let mut e1 = rv1.enter(sid, &programs.p1).unwrap();
    let mut e2 = programs
        .third_actor
        .then(|| rv2.enter(sid, &programs.p2).unwrap());
    // These Options contain actual affine Rust values, not a simulated owner table.
    let mut owner_db = programs.p3.as_ref().map(|program| {
        if programs.owner == 0 {
            rv0.enter(sid, program).unwrap()
        } else {
            rv1.enter(sid, program).unwrap()
        }
    });
    let mut worker_db: Option<Endpoint<'_, 3>> = None;
    let mut outcome = Outcome::Accepted;
    let scopes = (0..resolver_count).collect::<Vec<_>>();
    for (index, command) in commands.iter().enumerate() {
        decisions.prepare(&scopes, command.message, index + 1 == commands.len());
        macro_rules! observe {
            ($operation:expr) => {
                match tokio::time::timeout(core::time::Duration::from_millis(1), $operation).await {
                    Ok(Ok(_)) => Outcome::Accepted,
                    Ok(Err(error)) => {
                        assert_eq!(
                            index + 1,
                            commands.len(),
                            "prefix failed at {index}: {command:?}: {error:?}, trace={:?}",
                            commands
                                .iter()
                                .map(|c| (c.message.label, c.receive, c.preview))
                                .collect::<Vec<_>>()
                        );
                        Outcome::Rejected
                    }
                    Err(_) => Outcome::Pending,
                }
            };
        }
        macro_rules! execute {
            ($label:literal) => {{
                type Unit = g::Msg<$label, ()>;
                type Loan<'a> = g::Msg<$label, Endpoint<'a, 3>>;
                let view = if command.receive {
                    command.message.to
                } else {
                    command.message.from
                };
                match (view, command.receive, command.message.owned) {
                    (0, false, false) => observe!(e0.send::<Unit>(&())),
                    (1, false, false) => observe!(e1.send::<Unit>(&())),
                    (2, false, false) => observe!(e2.as_mut().unwrap().send::<Unit>(&())),
                    (0, true, false) => observe!(receive!(&mut e0, Unit, command)),
                    (1, true, false) => observe!(receive!(&mut e1, Unit, command)),
                    (2, true, false) => observe!(receive!(e2.as_mut().unwrap(), Unit, command)),
                    (3, false, false) => match (&mut owner_db, &mut worker_db) {
                        (Some(db), None) | (None, Some(db)) => observe!(db.send::<Unit>(&())),
                        (None, None) => Outcome::Unavailable,
                        (Some(_), Some(_)) => panic!("duplicate resource values"),
                    },
                    (3, true, false) => match (&mut owner_db, &mut worker_db) {
                        (Some(db), None) | (None, Some(db)) => {
                            observe!(receive!(db, Unit, command))
                        }
                        (None, None) => Outcome::Unavailable,
                        (Some(_), Some(_)) => panic!("duplicate resource values"),
                    },
                    (0, false, true) => match (owner_db.take(), worker_db.take()) {
                        (Some(db), None) | (None, Some(db)) => observe!(e0.send::<Loan<'_>>(db)),
                        (None, None) => Outcome::Unavailable,
                        (Some(_), Some(_)) => panic!("duplicate resource values"),
                    },
                    (1, false, true) => match (owner_db.take(), worker_db.take()) {
                        (Some(db), None) | (None, Some(db)) => observe!(e1.send::<Loan<'_>>(db)),
                        (None, None) => Outcome::Unavailable,
                        (Some(_), Some(_)) => panic!("duplicate resource values"),
                    },
                    (0, true, true) => match tokio::time::timeout(
                        core::time::Duration::from_millis(1),
                        receive!(&mut e0, Loan<'_>, command),
                    )
                    .await
                    {
                        Ok(Ok(db)) => {
                            assert!(owner_db.replace(db).is_none());
                            Outcome::Accepted
                        }
                        Ok(Err(error)) => {
                            assert_eq!(
                                index + 1, commands.len(),
                                "owned prefix failed at {index}: {command:?}: {error:?}"
                            );
                            Outcome::Rejected
                        },
                        Err(_) => Outcome::Pending,
                    },
                    (1, true, true) => match tokio::time::timeout(
                        core::time::Duration::from_millis(1),
                        receive!(&mut e1, Loan<'_>, command),
                    )
                    .await
                    {
                        Ok(Ok(db)) => {
                            assert!(worker_db.replace(db).is_none());
                            Outcome::Accepted
                        }
                        Ok(Err(error)) => {
                            assert_eq!(
                                index + 1, commands.len(),
                                "owned prefix failed at {index}: {command:?}: {error:?}"
                            );
                            Outcome::Rejected
                        },
                        Err(_) => Outcome::Pending,
                    },
                    _ => panic!("unrecognized generated command: {command:?}"),
                }
            }};
        }
        outcome = match command.message.label {
            0 => dispatch_labels!(),
            _ => panic!("generated label outside dispatch inventory"),
        };
        assert!(
            owner_db.is_none() || worker_db.is_none(),
            "duplicated owned endpoint"
        );
        if index + 1 != commands.len() {
            assert_eq!(
                outcome,
                Outcome::Accepted,
                "prefix failed at {index}: {commands:?}"
            );
        }
    }
    outcome
}

struct Scenario<'a> {
    resolver_count: u16,
    id: usize,
    name: &'a str,
    messages: &'a [Message],
    choices: &'a [u32],
    traces: &'a str,
    expected_traces: usize,
    parallel_groups: &'a [&'a [u8]],
    prefix_visits: usize,
}

async fn qualify(programs: Programs, scenario: Scenario<'_>) {
    let Scenario {
        resolver_count,
        id: case,
        name,
        messages,
        choices,
        traces,
        expected_traces,
        parallel_groups,
        prefix_visits,
    } = scenario;
    let mut observations = Vec::new();
    let mut prefixes = std::collections::BTreeMap::new();
    for choice in choices {
        for message in messages {
            for receive in [false, true] {
                let command = Command {
                    message: *message,
                    receive,
                    preview: false,
                };
                let actual = replay(
                    &programs,
                    resolver_count,
                    &Decisions::Constant(*choice),
                    &[command],
                )
                .await;
                observations.push(format!(
                    r#"{{"choice":{choice},"prefix":[],"label":{},"receive":{receive},"accepted":{}}}"#,
                    message.label,
                    actual == Outcome::Accepted
                ));
                if prefix_visits == 1 && actual == Outcome::Accepted {
                    prefixes.insert(
                        vec![2 * u16::from(message.label) + u16::from(receive)],
                        vec![command],
                    );
                }
                // The independent occurrence-model checker decides whether
                // this public call was permitted. Syntactic global order is
                // insufficient: an uninvolved sender can queue a later frame.
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for line in traces.lines() {
        assert!(seen.insert(line), "duplicate generated trace");
        // Choices follow the intended source path for this input message.
        // Nested completed rolls can share the same wire trace with different
        // visit groupings, so syntax-tree unfoldings do not count callbacks.
        let decisions = Decisions::ByMessage(
            (0..resolver_count)
                .map(|_| std::cell::Cell::new(None))
                .collect(),
        );
        let commands = line
            .split(',')
            .flat_map(|label| {
                let (label, preview) = match label.strip_suffix('?') {
                    Some(label) => (label, true),
                    None => (label, false),
                };
                let label = label.parse::<u8>().expect("invalid trace label");
                let message = *messages
                    .iter()
                    .find(|message| message.label == label)
                    .expect("unknown trace label");
                [
                    Command {
                        message,
                        receive: false,
                        preview: false,
                    },
                    Command {
                        message,
                        receive: true,
                        preview,
                    },
                ]
            })
            .collect::<Vec<_>>();
        let concurrent_decisions = decisions.clone();
        let actual = replay(&programs, resolver_count, &decisions, &commands).await;
        assert_eq!(actual, Outcome::Accepted, "shape={name} full trace={line}");
        if prefix_visits != 0 {
            let mut key = Vec::new();
            for (index, command) in commands.iter().enumerate() {
                key.push(2 * u16::from(command.message.label) + u16::from(command.receive));
                prefixes
                    .entry(key.clone())
                    .or_insert_with(|| commands[..=index].to_vec());
            }
        }
        if !parallel_groups.is_empty() {
            // The source tree fixes the fragment membership before any endpoint runs.
            parallel_replay(
                &programs,
                resolver_count,
                &concurrent_decisions,
                messages,
                &commands,
                parallel_groups,
            )
            .await;
        }
    }
    assert_eq!(
        seen.len(),
        expected_traces,
        "missing or additional generated traces"
    );
    let mut checked_prefixes = std::collections::BTreeSet::new();
    while let Some((key, prefix)) = prefixes.pop_first() {
        assert!(
            checked_prefixes.insert(key.clone()),
            "duplicate prefix replay"
        );
        for choice in choices {
            for message in messages {
                for receive in [false, true] {
                    let mut commands = prefix.clone();
                    commands.push(Command {
                        message: *message,
                        receive,
                        preview: false,
                    });
                    let actual = replay(
                        &programs,
                        resolver_count,
                        &Decisions::AfterPrefix {
                            choice: *choice,
                            values: (0..resolver_count)
                                .map(|_| std::cell::Cell::new(false))
                                .collect(),
                        },
                        &commands,
                    )
                    .await;
                    observations.push(format!(
                        r#"{{"choice":{choice},"prefix":{key:?},"label":{},"receive":{receive},"accepted":{}}}"#,
                        message.label, actual == Outcome::Accepted
                    ));
                    if prefix_visits == 1 && key.len() < 4 && actual == Outcome::Accepted {
                        let mut next = key.clone();
                        next.push(2 * u16::from(message.label) + u16::from(receive));
                        if !checked_prefixes.contains(&next) {
                            prefixes.entry(next).or_insert(commands);
                        }
                    }
                }
            }
        }
    }
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("observations");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("{case}.json")),
        format!(
            r#"{{"name":{case},"attempts":[{}]}}"#,
            observations.join(",")
        ),
    )
    .unwrap();
}
