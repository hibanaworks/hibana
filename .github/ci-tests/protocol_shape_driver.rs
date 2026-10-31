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

#[test]
fn generated_profile_keeps_debug_and_overflow_checks() {
    assert!(cfg!(debug_assertions));
    assert!(
        std::panic::catch_unwind(|| {
            let maximum = std::hint::black_box(u8::MAX);
            let _ = maximum + std::hint::black_box(1u8);
        })
        .is_err()
    );
}

enum Decisions<'a> {
    Constant(&'a [u16]),
    AfterPrefix {
        choice: &'a [u16],
        prefix: &'a [Vec<u16>],
        values: Vec<std::cell::Cell<bool>>,
    },
}
impl Decisions<'_> {
    fn choose(&self, id: usize) -> DecisionArm {
        let right = match self {
            Self::Constant(choice) => choice.iter().any(|right| usize::from(*right) == id),
            Self::AfterPrefix { values, .. } => values[id].get(),
        };
        if right {
            DecisionArm::Right
        } else {
            DecisionArm::Left
        }
    }
    fn prepare(&self, scopes: &[u16], index: usize, probe: bool) {
        if let Self::AfterPrefix {
            choice,
            prefix,
            values,
        } = self
        {
            let choice = if probe { *choice } else { &prefix[index] };
            for id in scopes {
                values[usize::from(*id)].set(choice.contains(id));
            }
        }
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn resolver_inputs_support_the_full_u16_identifier_range() {
    type Left = g::Msg<250, ()>;
    type Right = g::Msg<251, ()>;
    let global = g::route(g::send::<0, 1, Left>(), g::send::<0, 1, Right>()).resolve::<65535>();
    let p0 = project::<0>(&global);
    let p1 = project::<1>(&global);
    let decisions = Decisions::Constant(&[65535]);
    let network = InMemoryTransport::new(None);
    let mut slab0 = vec![0; 128 * 1024];
    let mut slab1 = vec![0; 128 * 1024];
    let mut storage0 = SessionKitStorage::uninit();
    let mut storage1 = SessionKitStorage::uninit();
    let rv0 = storage0
        .init()
        .rendezvous(&mut slab0, network.clone())
        .unwrap();
    let rv1 = storage1.init().rendezvous(&mut slab1, network).unwrap();
    rv0.set_resolver(
        &p0,
        ResolverRef::<65535>::decision_state(&decisions, |input| Ok(input.choose(65535))),
    )
    .unwrap();
    let mut a = rv0.enter(SessionId::new(1), &p0).unwrap();
    let mut b = rv1.enter(SessionId::new(1), &p1).unwrap();
    tokio::time::timeout(core::time::Duration::from_secs(1), async {
        tokio::try_join!(async { a.send::<Right>(&()).await }, async {
            let branch = b.offer().await?;
            assert_eq!(branch.label(), 251);
            branch.recv::<Right>().await
        })
    })
    .await
    .expect("resolver input stalled")
    .unwrap();
}

struct Programs {
    p0: Option<RoleProgram<0>>,
    p1: Option<RoleProgram<1>>,
    p2: Option<RoleProgram<2>>,
    p3: Option<RoleProgram<3>>,
    resource: bool,
    owner: u8,
    resolver_roles: &'static [u8],
}
#[derive(Clone, Copy, Debug)]
struct Message {
    from: u8,
    to: u8,
    event: u8,
    contract: u8,
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

// Immutable schedule input, generated from the source before any endpoint runs.
// A nested repeat's visit boundaries cannot be recovered from message labels.
#[derive(Debug, PartialEq)]
enum Script {
    Message(u8, usize),
    Seq(Box<Self>, Box<Self>),
    Par(Box<Self>, Box<Self>, bool),
    Branch(Box<Self>, bool),
    Roll([Box<Self>; 2]),
}
impl Script {
    fn from_json(text: &str) -> Result<Self, String> {
        use planned_json::Json;
        fn number(value: &Json) -> Result<usize, String> {
            match value {
                Json::Number(value) => value.parse().map_err(|_| "invalid script number".into()),
                _ => Err("expected script number".into()),
            }
        }
        fn decode(value: &Json) -> Result<Script, String> {
            let Json::Array(fields) = value else {
                return Err("expected script node array".into());
            };
            match fields.as_slice() {
                [Json::Number(kind), label, occurrence] if kind == "0" => Ok(Script::Message(
                    number(label)?
                        .try_into()
                        .map_err(|_| "script label exceeds u8")?,
                    number(occurrence)?,
                )),
                [Json::Number(kind), a, b] if kind == "1" => {
                    Ok(Script::Seq(Box::new(decode(a)?), Box::new(decode(b)?)))
                }
                [Json::Number(kind), a, b, Json::Bool(reverse)] if kind == "2" => Ok(Script::Par(
                    Box::new(decode(a)?),
                    Box::new(decode(b)?),
                    *reverse,
                )),
                [Json::Number(kind), body, Json::Bool(right)] if kind == "3" => {
                    Ok(Script::Branch(Box::new(decode(body)?), *right))
                }
                [Json::Number(kind), a, b] if kind == "4" => {
                    Ok(Script::Roll([Box::new(decode(a)?), Box::new(decode(b)?)]))
                }
                _ => Err("unknown script kind or invalid node fields".into()),
            }
        }
        decode(&planned_json::parse(text)?)
    }
    fn trace(&self) -> Vec<(u8, bool)> {
        match self {
            Self::Message(label, _) => vec![(*label, false)],
            Self::Seq(a, b) | Self::Par(a, b, false) | Self::Roll([a, b]) => {
                let mut trace = a.trace();
                trace.extend(b.trace());
                trace
            }
            Self::Par(a, b, true) => {
                let mut trace = b.trace();
                trace.extend(a.trace());
                trace
            }
            Self::Branch(body, _) => {
                let mut trace = body.trace();
                trace.first_mut().unwrap().1 = true;
                trace
            }
        }
    }
    fn line(&self) -> String {
        self.trace()
            .into_iter()
            .map(|(label, preview)| format!("{label}{}", if preview { "?" } else { "" }))
            .collect::<Vec<_>>()
            .join(",")
    }
    fn first_occurrence(&self) -> usize {
        match self {
            Self::Message(_, occurrence) => *occurrence,
            Self::Seq(a, _) | Self::Par(a, _, _) | Self::Branch(a, _) | Self::Roll([a, _]) => {
                a.first_occurrence()
            }
        }
    }
}

#[test]
fn scoped_scripts_keep_order_choices_and_visit_boundaries() {
    let forward = Script::from_json("[1,[3,[0,1,0],false],[4,[0,1,1],[0,2,2]]]").unwrap();
    let reverse = Script::from_json("[2,[3,[0,1,0],false],[4,[0,1,1],[0,2,2]],true]").unwrap();
    assert_eq!(forward.line(), "1?,1,2");
    assert_eq!(reverse.line(), "1,2,1?");
    assert_eq!(
        Script::from_json("[2,[3,[0,1,0],false],[4,[0,1,1],[0,2,2]],false]")
            .unwrap()
            .line(),
        forward.line()
    );
    assert!(
        matches!(&forward, Script::Seq(choice, _) if matches!(choice.as_ref(), Script::Branch(_, false)))
    );
    assert_eq!(
        Script::Branch(Box::new(Script::Message(1, 0)), true).line(),
        "1?"
    );
    assert_eq!(reverse.first_occurrence(), 0);
    assert_eq!(Script::Message(2, 1).first_occurrence(), 1);
    assert_eq!(
        Script::from_json("[4,[3,[0,1,6],true],[3,[0,1,7],false]]").unwrap(),
        Script::Roll([
            Box::new(Script::Branch(Box::new(Script::Message(1, 6)), true)),
            Box::new(Script::Branch(Box::new(Script::Message(1, 7)), false)),
        ])
    );
}

#[test]
fn scoped_script_decoder_rejects_malformed_plans() {
    for text in [
        "",
        "null",
        "{}",
        "[]",
        "[9]",
        "[0,1]",
        "[0,1,0,2]",
        "[0,256,0]",
        "[0,-1,0]",
        "[0,1,-1]",
        "[0,1,18446744073709551616]",
        "[1,[0,1,0],null]",
        "[2,[0,1,0],[0,2,1],0]",
        "[3,[0,1,0],1]",
        "[4,[0,1,0]]",
        "[0,1,0] trailing",
    ] {
        assert!(
            Script::from_json(text).is_err(),
            "accepted malformed plan {text}"
        );
    }
}

macro_rules! receive {
    ($endpoint:expr, $message:ty, $command:expr) => {
        async {
            let endpoint = $endpoint;
            if $command.preview {
                let branch = endpoint.offer().await?;
                assert_eq!(branch.label(), $command.message.contract);
                drop(branch);
                let branch = endpoint.offer().await?;
                assert_eq!(branch.label(), $command.message.contract);
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
    decisions: &Decisions<'_>,
    commands: &[Command],
) -> Outcome {
    let sid = SessionId::new(1);
    let network = InMemoryTransport::new(programs.resource.then(|| {
        // SAFETY: this replay owns the resource and its only initial issuer.
        ((sid, 3), unsafe { EndpointBinding::new() })
    }));
    let mut slabs = [
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
    ];
    let [slab0, slab1, slab2, slab3] = &mut slabs;
    let mut storage0 = SessionKitStorage::uninit();
    let mut storage1 = SessionKitStorage::uninit();
    let mut storage2 = SessionKitStorage::uninit();
    let mut storage3 = SessionKitStorage::uninit();
    let rv0 = storage0.init().rendezvous(slab0, network.clone()).unwrap();
    let rv1 = storage1.init().rendezvous(slab1, network.clone()).unwrap();
    let rv2 = storage2.init().rendezvous(slab2, network.clone()).unwrap();
    let rv3 = storage3.init().rendezvous(slab3, network).unwrap();
    macro_rules! register {
        ($id:literal) => {
            if resolver_count > $id {
                let resolver = ResolverRef::<$id>::decision_state(decisions, |c| Ok(c.choose($id)));
                if programs.resolver_roles[$id] == 3 && programs.resource {
                    let program = programs.p3.as_ref().unwrap();
                    rv0.set_resolver(program, resolver).unwrap();
                    rv1.set_resolver(
                        program,
                        ResolverRef::<$id>::decision_state(decisions, |c| Ok(c.choose($id))),
                    )
                    .unwrap();
                    rv2.set_resolver(
                        program,
                        ResolverRef::<$id>::decision_state(decisions, |c| Ok(c.choose($id))),
                    )
                    .unwrap();
                } else {
                    match programs.resolver_roles[$id] {
                        0 => rv0
                            .set_resolver(programs.p0.as_ref().unwrap(), resolver)
                            .unwrap(),
                        1 => rv1
                            .set_resolver(programs.p1.as_ref().unwrap(), resolver)
                            .unwrap(),
                        2 => rv2
                            .set_resolver(programs.p2.as_ref().unwrap(), resolver)
                            .unwrap(),
                        3 => rv3
                            .set_resolver(programs.p3.as_ref().unwrap(), resolver)
                            .unwrap(),
                        _ => panic!("resolver role outside grammar"),
                    }
                }
            }
        };
    }
    register_resolvers!();
    let mut e0 = programs
        .p0
        .as_ref()
        .map(|program| rv0.enter(sid, program).unwrap());
    let mut e1 = programs
        .p1
        .as_ref()
        .map(|program| rv1.enter(sid, program).unwrap());
    let mut e2 = programs
        .p2
        .as_ref()
        .map(|program| rv2.enter(sid, program).unwrap());
    // These Options contain actual affine Rust values, not a simulated owner table.
    let mut e3 = if programs.resource {
        None
    } else {
        programs
            .p3
            .as_ref()
            .map(|program| rv3.enter(sid, program).unwrap())
    };
    let mut databases: [Option<Endpoint<'_, 3>>; 3] = [None, None, None];
    if programs.resource {
        let program = programs.p3.as_ref().unwrap();
        databases[usize::from(programs.owner)] = Some(match programs.owner {
            0 => rv0.enter(sid, program).unwrap(),
            1 => rv1.enter(sid, program).unwrap(),
            2 => rv2.enter(sid, program).unwrap(),
            _ => panic!("resource initial owner outside actor domain"),
        });
    }
    let mut outcome = Outcome::Accepted;
    let scopes = (0..resolver_count).collect::<Vec<_>>();
    for (index, command) in commands.iter().enumerate() {
        decisions.prepare(&scopes, index, index + 1 == commands.len());
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
                                .map(|c| (c.message.event, c.receive, c.preview))
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
                    (0, false, false) => observe!(e0.as_mut().unwrap().send::<Unit>(&())),
                    (1, false, false) => observe!(e1.as_mut().unwrap().send::<Unit>(&())),
                    (2, false, false) => observe!(e2.as_mut().unwrap().send::<Unit>(&())),
                    (0, true, false) => observe!(receive!(e0.as_mut().unwrap(), Unit, command)),
                    (1, true, false) => observe!(receive!(e1.as_mut().unwrap(), Unit, command)),
                    (2, true, false) => observe!(receive!(e2.as_mut().unwrap(), Unit, command)),
                    (3, false, false) => match e3
                        .as_mut()
                        .or_else(|| databases.iter_mut().find_map(Option::as_mut))
                    {
                        Some(db) => observe!(db.send::<Unit>(&())),
                        None => Outcome::Unavailable,
                    },
                    (3, true, false) => match e3
                        .as_mut()
                        .or_else(|| databases.iter_mut().find_map(Option::as_mut))
                    {
                        Some(db) => observe!(receive!(db, Unit, command)),
                        None => Outcome::Unavailable,
                    },
                    (0, false, true) => match databases[0].take() {
                        Some(db) => observe!(e0.as_mut().unwrap().send::<Loan<'_>>(db)),
                        None => Outcome::Unavailable,
                    },
                    (1, false, true) => match databases[1].take() {
                        Some(db) => observe!(e1.as_mut().unwrap().send::<Loan<'_>>(db)),
                        None => Outcome::Unavailable,
                    },
                    (2, false, true) => match databases[2].take() {
                        Some(db) => observe!(e2.as_mut().unwrap().send::<Loan<'_>>(db)),
                        None => Outcome::Unavailable,
                    },
                    (0, true, true) => match tokio::time::timeout(
                        core::time::Duration::from_millis(1),
                        receive!(e0.as_mut().unwrap(), Loan<'_>, command),
                    )
                    .await
                    {
                        Ok(Ok(db)) => {
                            assert!(databases[0].replace(db).is_none());
                            Outcome::Accepted
                        }
                        Ok(Err(error)) => {
                            assert_eq!(
                                index + 1,
                                commands.len(),
                                "owned prefix failed at {index}: {command:?}: {error:?}"
                            );
                            Outcome::Rejected
                        }
                        Err(_) => Outcome::Pending,
                    },
                    (1, true, true) => match tokio::time::timeout(
                        core::time::Duration::from_millis(1),
                        receive!(e1.as_mut().unwrap(), Loan<'_>, command),
                    )
                    .await
                    {
                        Ok(Ok(db)) => {
                            assert!(databases[1].replace(db).is_none());
                            Outcome::Accepted
                        }
                        Ok(Err(error)) => {
                            assert_eq!(
                                index + 1,
                                commands.len(),
                                "owned prefix failed at {index}: {command:?}: {error:?}"
                            );
                            Outcome::Rejected
                        }
                        Err(_) => Outcome::Pending,
                    },
                    (2, true, true) => match tokio::time::timeout(
                        core::time::Duration::from_millis(1),
                        receive!(e2.as_mut().unwrap(), Loan<'_>, command),
                    )
                    .await
                    {
                        Ok(Ok(db)) => {
                            assert!(databases[2].replace(db).is_none());
                            Outcome::Accepted
                        }
                        Ok(Err(error)) => {
                            assert_eq!(
                                index + 1,
                                commands.len(),
                                "owned prefix failed at {index}: {command:?}: {error:?}"
                            );
                            Outcome::Rejected
                        }
                        Err(_) => Outcome::Pending,
                    },
                    _ => panic!("unrecognized generated command: {command:?}"),
                }
            }};
        }
        outcome = match command.message.contract {
            0 => dispatch_labels!(),
            _ => panic!("generated message contract outside dispatch inventory"),
        };
        assert!(
            databases.iter().filter(|db| db.is_some()).count() <= 1,
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
    choices: &'a [&'a [u16]],
    traces: &'a str,
    expected_traces: usize,
    prefix_visits: usize,
    completed_plan_words: &'a std::collections::BTreeSet<String>,
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn alias_replay_keeps_the_recorded_resolver_input() {
    type First = g::Msg<1, ()>;
    type Reused = g::Msg<0, ()>;
    let global = g::route(
        g::seq(g::send::<0, 1, First>(), g::send::<0, 1, Reused>()),
        g::send::<0, 1, Reused>(),
    )
    .resolve::<0>();
    let programs = Programs {
        p0: Some(project::<0>(&global)),
        p1: Some(project::<1>(&global)),
        p2: None,
        p3: None,
        resource: false,
        owner: 0,
        resolver_roles: &[0],
    };
    let alias = Message {
        from: 0,
        to: 1,
        event: 1,
        contract: 0,
        owned: false,
        decisions: &[(0, false)],
    };
    let actual = Message {
        event: 2,
        decisions: &[(0, true)],
        ..alias
    };
    let decisions = Decisions::AfterPrefix {
        choice: &[],
        prefix: &[vec![0]],
        values: vec![std::cell::Cell::new(false)],
    };
    assert_eq!(
        replay(
            &programs,
            1,
            &decisions,
            &[
                Command {
                    message: alias,
                    receive: false,
                    preview: false
                },
                Command {
                    message: actual,
                    receive: true,
                    preview: false
                },
            ]
        )
        .await,
        Outcome::Accepted,
    );
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
        prefix_visits,
        completed_plan_words,
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
                    r#"{{"choice":{choice:?},"prefix":[],"prefixChoices":[],"event":{},"receive":{receive},"accepted":{}}}"#,
                    message.event,
                    actual == Outcome::Accepted
                ));
                if prefix_visits == 1 && actual == Outcome::Accepted {
                    prefixes
                        .entry(vec![2 * u16::from(message.event) + u16::from(receive)])
                        .or_insert_with(|| (vec![command], vec![choice.to_vec()]));
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
                    .find(|message| message.event == label)
                    .expect("unknown trace label");
                let commands = [
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
                ];
                commands
                    .into_iter()
                    .filter(move |c| message.from != message.to || !c.receive)
            })
            .collect::<Vec<_>>();
        assert!(
            completed_plan_words.contains(line),
            "shape={name} missing completed source plan={line}"
        );
        if prefix_visits != 0 {
            let mut key = Vec::new();
            let mut inputs = vec![false; usize::from(resolver_count)];
            let mut history = Vec::new();
            for (index, command) in commands.iter().enumerate() {
                key.push(2 * u16::from(command.message.event) + u16::from(command.receive));
                for &(id, right) in command.message.decisions {
                    inputs[usize::from(id)] = right;
                }
                history.push(
                    (0..resolver_count)
                        .filter(|&id| inputs[usize::from(id)])
                        .collect::<Vec<_>>(),
                );
                prefixes
                    .entry(key.clone())
                    .or_insert_with(|| (commands[..=index].to_vec(), history.clone()));
            }
        }
    }
    assert_eq!(
        seen.len(),
        expected_traces,
        "missing or additional generated traces"
    );
    assert_eq!(
        seen,
        completed_plan_words.iter().map(String::as_str).collect(),
        "completed source plans and catalog words differ"
    );
    let mut checked_prefixes = std::collections::BTreeSet::new();
    while let Some((key, (prefix, history))) = prefixes.pop_first() {
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
                            prefix: &history,
                            values: (0..resolver_count)
                                .map(|_| std::cell::Cell::new(false))
                                .collect(),
                        },
                        &commands,
                    )
                    .await;
                    observations.push(format!(
                        r#"{{"choice":{choice:?},"prefix":{key:?},"prefixChoices":{history:?},"event":{},"receive":{receive},"accepted":{}}}"#,
                        message.event, actual == Outcome::Accepted
                    ));
                    if prefix_visits == 1 && key.len() < 4 && actual == Outcome::Accepted {
                        let mut next = key.clone();
                        next.push(2 * u16::from(message.event) + u16::from(receive));
                        if !checked_prefixes.contains(&next) {
                            let mut next_history = history.clone();
                            next_history.push(choice.to_vec());
                            prefixes.entry(next).or_insert((commands, next_history));
                        }
                    }
                }
            }
        }
    }
    let directory = observation_root().join("observations");
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
