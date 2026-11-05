// Replay generated public operations and test the shared Endpoint storage.
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

#[hibana_runtime::main(in_process::InProcess::default())]
#[test]
async fn resolver_inputs_support_the_full_u16_identifier_range() -> core::result::Result<(), hibana::EndpointError>
{
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
    async {
        async { a.send::<Right>(&()).await };
        async {
            let branch = b.offer().await?;
            assert_eq!(branch.label(), 251);
            branch.recv::<Right>().await
        };
        Ok(())
    }
    .await
    .unwrap();
    Ok(())
}

struct Programs {
    p0: Option<RoleProgram<0>>,
    p1: Option<RoleProgram<1>>,
    p2: Option<RoleProgram<2>>,
    p3: Option<RoleProgram<3>>,
    issuance: &'static [(u8, u8)],
    resolver_roles: &'static [u8],
}
#[derive(Clone, Copy, Debug)]
struct Message {
    from: u8,
    to: u8,
    event: u8,
    contract: u8,
    endpoint: Option<u8>,
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
#[derive(Clone, Debug, PartialEq)]
enum Script {
    Message(u8, usize),
    Seq(Box<Self>, Box<Self>),
    Par(Box<Self>, Box<Self>, bool),
    Branch(Box<Self>, bool),
    Roll([Box<Self>; 2]),
}
impl Script {
    // Reassociate immutable test input in exactly the same way as the source
    // emitter's sequential-prefix split. Occurrence numbers are unchanged.
    fn split_first_message(&self) -> (&Self, Option<Self>) {
        match self {
            Self::Message(..) => (self, None),
            Self::Seq(a, b) => {
                let (first, rest) = a.split_first_message();
                (
                    first,
                    Some(match rest {
                        Some(rest) => Self::Seq(Box::new(rest), b.clone()),
                        None => *b.clone(),
                    }),
                )
            }
            _ => panic!("shared message prefix must be sequential"),
        }
    }
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
    park_before_last: bool,
) -> (Outcome, Option<u8>) {
    let sid = SessionId::new(1);
    let network = InMemoryTransport::new(programs.issuance.iter().map(|&(role, _)| {
        // SAFETY: every declared test resource has one binding in this replay.
        ((sid, role), unsafe { EndpointBinding::new() })
    }));
    let mut slabs = [
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
        vec![0; 128 * 1024],
    ];
    let [slab0, slab1, slab2, slab3] = &mut slabs;
    let resolver_calls = std::cell::Cell::new(0usize);
    let resolver_audits = std::cell::Cell::new(0usize);
    let recorded_decisions = std::cell::Cell::new(Vec::new());
    let resolver_input = (decisions, &resolver_calls);
    let capture = |event: hibana::runtime::tap::TapEvent| {
        if event.id() == hibana::runtime::tap::RESOLVER_AUDIT {
            resolver_audits.set(resolver_audits.get() + 1);
            assert_eq!(resolver_audits.get(), resolver_calls.get(),
                "an audit must describe an actual resolver call, not a later commit");
            let mut decisions = recorded_decisions.take();
            decisions.push(event);
            recorded_decisions.set(decisions);
        }
        if event.id() == hibana::runtime::tap::ROUTE_ARM_SELECTION && event.causal_key() & 255 == 2 {
            let decisions = recorded_decisions.take();
            let prior = decisions.iter().rev().find(|prior| {
                prior.arg0() == event.arg0()
                    && prior.causal_key() >> 8 == event.causal_key() >> 8
                    && prior.arg1() >> 16 == event.arg1() >> 16
            });
            assert!(prior.is_some_and(|prior| u32::from(prior.causal_key() & 255) == event.arg1() & 65535),
                "a resolver selection must name the actor that actually decided");
            recorded_decisions.set(decisions);
        }
    };
    let mut storage0 = SessionKitStorage::uninit();
    let mut storage1 = SessionKitStorage::uninit();
    let mut storage2 = SessionKitStorage::uninit();
    let mut storage3 = SessionKitStorage::uninit();
    let rv0 = storage0.init().rendezvous(slab0, network.clone()).unwrap();
    let rv1 = storage1.init().rendezvous(slab1, network.clone()).unwrap();
    let rv2 = storage2.init().rendezvous(slab2, network.clone()).unwrap();
    let rv3 = storage3.init().rendezvous(slab3, network).unwrap();
    for rv in [&rv0, &rv1, &rv2, &rv3] {
        rv.observe(&capture);
    }
    macro_rules! register {
        ($id:literal) => {
            if resolver_count > $id {
                let role = programs.resolver_roles[$id];
                macro_rules! on_program {
                    ($program:expr) => {
                        for (actor, rv) in [&rv0, &rv1, &rv2, &rv3].into_iter().enumerate() {
                            if actor == usize::from(role)
                                || programs
                                    .issuance
                                    .iter()
                                    .any(|&(resource, _)| resource == role)
                            {
                                rv.set_resolver(
                                    $program.as_ref().unwrap(),
                                    ResolverRef::<$id>::decision_state(&resolver_input, |input| {
                                        input.1.set(input.1.get() + 1);
                                        Ok(input.0.choose($id))
                                    }),
                                )
                                .unwrap();
                            }
                        }
                    };
                }
                match role {
                    0 => on_program!(programs.p0),
                    1 => on_program!(programs.p1),
                    2 => on_program!(programs.p2),
                    3 => on_program!(programs.p3),
                    _ => panic!("resolver role outside grammar"),
                }
            }
        };
    }
    register_resolvers!();
    // Every slot contains an actual affine Endpoint value. Its move is the
    // ownership change; the driver keeps no separate current-holder field.
    let mut views0: [Option<Endpoint<0>>; 4] = [None, None, None, None];
    let mut views1: [Option<Endpoint<1>>; 4] = [None, None, None, None];
    let mut views2: [Option<Endpoint<2>>; 4] = [None, None, None, None];
    let mut views3: [Option<Endpoint<3>>; 4] = [None, None, None, None];
    macro_rules! enter {
        ($role:literal, $program:expr, $views:ident) => {
            if let Some(program) = $program.as_ref() {
                let actor = programs
                    .issuance
                    .iter()
                    .find_map(|&(resource, actor)| (resource == $role).then_some(actor))
                    .unwrap_or($role);
                $views[usize::from(actor)] = Some(
                    [&rv0, &rv1, &rv2, &rv3][usize::from(actor)]
                        .enter(sid, program)
                        .unwrap(),
                );
            }
        };
    }
    enter!(0, programs.p0, views0);
    enter!(1, programs.p1, views1);
    enter!(2, programs.p2, views2);
    enter!(3, programs.p3, views3);
    let mut outcome = Outcome::Accepted;
    let mut parked_label = None;
    let scopes = (0..resolver_count).collect::<Vec<_>>();
    for (index, command) in commands.iter().enumerate() {
        decisions.prepare(&scopes, index, index + 1 == commands.len());
        if park_before_last && index + 1 == commands.len() {
            macro_rules! park {
                ($endpoint:expr) => {{
                    let offered = in_process::timeout(core::time::Duration::from_millis(1), $endpoint.offer()).await;
                    assert_eq!(resolver_audits.get(), resolver_calls.get(),
                        "offering a generated branch must record each resolver call");
                    match offered {
                        Ok(Ok(branch)) => {
                            parked_label = Some(branch.label());
                            drop(branch);
                        }
                        Ok(Err(_)) | Err(_) => return (outcome, None),
                    }
                }};
            }
            let view = if command.receive {
                command.message.to
            } else {
                command.message.from
            };
            match view {
                0 => match views0.iter_mut().find_map(Option::as_mut) {
                    Some(endpoint) => park!(endpoint),
                    None => return (Outcome::Unavailable, None),
                },
                1 => match views1.iter_mut().find_map(Option::as_mut) {
                    Some(endpoint) => park!(endpoint),
                    None => return (Outcome::Unavailable, None),
                },
                2 => match views2.iter_mut().find_map(Option::as_mut) {
                    Some(endpoint) => park!(endpoint),
                    None => return (Outcome::Unavailable, None),
                },
                3 => match views3.iter_mut().find_map(Option::as_mut) {
                    Some(endpoint) => park!(endpoint),
                    None => return (Outcome::Unavailable, None),
                },
                _ => panic!("preview view outside generated role domain"),
            }
        }
        macro_rules! observe {
            ($operation:expr) => {
                match in_process::timeout(core::time::Duration::from_millis(1), $operation).await {
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
        macro_rules! unit {
            ($label:literal, $views:ident) => {{
                type Unit = g::Msg<$label, ()>;
                match $views.iter().position(Option::is_some) {
                    None => Outcome::Unavailable,
                    Some(actor) => {
                        let endpoint = $views[actor].as_mut().unwrap();
                        if command.receive {
                            observe!(receive!(endpoint, Unit, command))
                        } else {
                            observe!(endpoint.send::<Unit>(&()))
                        }
                    }
                }
            }};
        }
        macro_rules! loan {
            ($label:literal, $resource:literal, $carrier:ident, $payload:ident) => {{
                type Loan<'a> = g::Msg<$label, Endpoint<'a, $resource>>;
                match $carrier.iter().position(Option::is_some) {
                    None => Outcome::Unavailable,
                    Some(actor) => {
                        if command.receive {
                            match in_process::timeout(core::time::Duration::from_millis(1), receive!(
                                    $carrier[actor].as_mut().unwrap(),
                                    Loan,
                                    command
                                )).await {
                                Ok(Ok(endpoint)) => {
                                    assert!(
                                        $payload[actor].replace(endpoint).is_none(),
                                        "duplicated owned endpoint"
                                    );
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
                            }
                        } else {
                            match $payload[actor].take() {
                                Some(endpoint) => observe!(
                                    $carrier[actor]
                                        .as_mut()
                                        .expect("an Endpoint cannot carry itself")
                                        .send::<Loan>(endpoint)
                                ),
                                None => Outcome::Unavailable,
                            }
                        }
                    }
                }
            }};
        }
        let view = if command.receive { command.message.to } else { command.message.from };
        outcome = match (command.message.contract, view, command.message.endpoint) {
            (0, 0, None) => dispatch_operations!(),
            _ => panic!("generated message contract outside dispatch inventory"),
        };
        assert_eq!(resolver_audits.get(), resolver_calls.get(),
            "generated typed operations must preserve resolver observations");
        for count in [
            views0.iter().filter(|value| value.is_some()).count(),
            views1.iter().filter(|value| value.is_some()).count(),
            views2.iter().filter(|value| value.is_some()).count(),
            views3.iter().filter(|value| value.is_some()).count(),
        ] {
            assert!(count <= 1, "duplicated owned endpoint");
        }
        if index + 1 != commands.len() {
            assert_eq!(
                outcome,
                Outcome::Accepted,
                "prefix failed at {index}: {commands:?}"
            );
        }
    }
    (outcome, parked_label)
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

#[hibana_runtime::main(in_process::InProcess::default())]
#[test]
async fn alias_replay_keeps_the_recorded_resolver_input() -> core::result::Result<(), hibana::EndpointError> {
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
        issuance: &[],
        resolver_roles: &[0],
    };
    let alias = Message {
        from: 0,
        to: 1,
        event: 1,
        contract: 0,
        endpoint: None,
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
            ],
            false
        )
        .await
        .0,
        Outcome::Accepted,
    );
    Ok(())
}

#[hibana_runtime::main(in_process::InProcess::default())]
#[test]
async fn replay_preserves_selected_holders_across_two_resource_transfers()
-> core::result::Result<(), hibana::EndpointError> {
    fn programs() -> Programs {
        type Notice = g::Msg<REPLAY_NOTICE, ()>;
        type Loan<'r> = g::Msg<REPLAY_LOAN, Endpoint<'r, 3>>;
        type Proxy<'r> = g::Msg<REPLAY_PROXY, Endpoint<'r, 2>>;
        type Work = g::Msg<REPLAY_WORK, ()>;
        type Returned<'r> = g::Msg<REPLAY_RETURNED, Endpoint<'r, 3>>;
        let global = g::seq(
            g::route(
                g::send::<0, 1, Notice>(),
                g::seq(g::send::<0, 1, Loan>(), g::send::<0, 1, Proxy>()),
            ),
            g::seq(g::send::<3, 0, Work>().roll(), g::send::<2, 0, Returned>()),
        );
        Programs {
            p0: Some(project::<0>(&global)),
            p1: Some(project::<1>(&global)),
            p2: Some(project::<2>(&global)),
            p3: Some(project::<3>(&global)),
            issuance: &[(2, 0), (3, 0)],
            resolver_roles: &[],
        }
    }
    let programs = programs();
    let messages = [
        Message {
            from: 0,
            to: 1,
            event: 0,
            contract: REPLAY_NOTICE,
            endpoint: None,
            decisions: &[],
        },
        Message {
            from: 0,
            to: 1,
            event: 1,
            contract: REPLAY_LOAN,
            endpoint: Some(3),
            decisions: &[],
        },
        Message {
            from: 0,
            to: 1,
            event: 2,
            contract: REPLAY_PROXY,
            endpoint: Some(2),
            decisions: &[],
        },
        Message {
            from: 3,
            to: 0,
            event: 3,
            contract: REPLAY_WORK,
            endpoint: None,
            decisions: &[],
        },
        Message {
            from: 2,
            to: 0,
            event: 4,
            contract: REPLAY_RETURNED,
            endpoint: Some(3),
            decisions: &[],
        },
    ];
    for preview in [false, true] {
        for delegated in [false, true] {
            let events: &[usize] = if delegated { &[1, 2, 3, 4] } else { &[0, 3, 4] };
            let mut commands = events
                .iter()
                .flat_map(|&event| {
                    [false, true].map(|receive| Command {
                        message: messages[event],
                        receive,
                        preview: receive && preview && event == events[0],
                    })
                })
                .collect::<Vec<_>>();
            commands.push(Command {
                message: messages[3],
                receive: false,
                preview: false,
            });
            let actual = replay(&programs, 0, &Decisions::Constant(&[]), &commands, false)
                .await
                .0;
            assert_eq!(
                actual,
                if delegated {
                    Outcome::Rejected
                } else {
                    Outcome::Accepted
                },
                "selected holder differs: delegated={delegated}, preview={preview}"
            );
        }
    }
    Ok(())
}

#[hibana_runtime::main(in_process::InProcess::default())]
#[test]
async fn replay_allows_retained_work_when_an_unused_resource_moves()
-> core::result::Result<(), hibana::EndpointError> {
    fn programs() -> Programs {
        type Loan<'r> = g::Msg<REPLAY_LOAN, Endpoint<'r, 3>>;
        type Work = g::Msg<REPLAY_WORK, ()>;
        type Proxy<'r> = g::Msg<REPLAY_PROXY, Endpoint<'r, 2>>;
        let global = g::seq(
            g::send::<0, 1, Loan>(),
            g::seq(g::send::<3, 0, Work>().roll(), g::send::<0, 1, Proxy>()),
        );
        Programs {
            p0: Some(project::<0>(&global)),
            p1: Some(project::<1>(&global)),
            p2: Some(project::<2>(&global)),
            p3: Some(project::<3>(&global)),
            issuance: &[(2, 0), (3, 0)],
            resolver_roles: &[],
        }
    }
    let programs = programs();
    let messages = [
        Message {
            from: 0,
            to: 1,
            event: 0,
            contract: REPLAY_LOAN,
            endpoint: Some(3),
            decisions: &[],
        },
        Message {
            from: 3,
            to: 0,
            event: 1,
            contract: REPLAY_WORK,
            endpoint: None,
            decisions: &[],
        },
        Message {
            from: 0,
            to: 1,
            event: 2,
            contract: REPLAY_PROXY,
            endpoint: Some(2),
            decisions: &[],
        },
    ];
    let commands = [0, 1, 2, 1]
        .into_iter()
        .flat_map(|event| {
            [false, true].map(|receive| Command {
                message: messages[event],
                receive,
                preview: false,
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        replay(&programs, 0, &Decisions::Constant(&[]), &commands, false)
            .await
            .0,
        Outcome::Accepted,
        "moving an unused resource must preserve the retained operation"
    );
    Ok(())
}

async fn qualify(programs: Programs, scenario: Scenario<'_>) {
    use std::io::Write;
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
    let directory = observation_root().join("observations");
    std::fs::create_dir_all(&directory).unwrap();
    let mut observations = std::io::BufWriter::new(
        std::fs::File::create(directory.join(format!("{case}.jsonl"))).unwrap(),
    );
    let events = messages.iter().map(|message| message.event).collect::<Vec<_>>();
    writeln!(observations, r#"{{"name":{case},"choices":{choices:?},"events":{events:?}}}"#).unwrap();
    write!(observations, r#"{{"prefix":[],"prefixChoices":[],"answers":["#).unwrap();
    let mut separator = "";
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
                    false,
                )
                .await
                .0;
                let (parked, label) = replay(
                    &programs,
                    resolver_count,
                    &Decisions::Constant(*choice),
                    &[command],
                    true,
                )
                .await;
                let parked = label.map_or_else(
                    || "[]".into(),
                    |label| format!("[[{label},{}]]", parked == Outcome::Accepted),
                );
                write!(observations,
                    r#"{separator}[{},{parked}]"#,
                    actual == Outcome::Accepted
                ).unwrap();
                separator = ",";
                if prefix_visits == 1 && actual == Outcome::Accepted {
                    prefixes
                        .entry((
                            vec![2 * u16::from(message.event) + u16::from(receive)],
                            vec![choice.to_vec()],
                        ))
                        .or_insert_with(|| vec![command]);
                }
                // The independent occurrence-model checker decides whether
                // this public call was permitted. Syntactic global order is
                // insufficient: an uninvolved sender can queue a later frame.
            }
        }
    }
    writeln!(observations, "]}}").unwrap();
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
                    .entry((key.clone(), history.clone()))
                    .or_insert_with(|| commands[..=index].to_vec());
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
    while let Some(((key, history), prefix)) = prefixes.pop_first() {
        assert!(
            checked_prefixes.insert((key.clone(), history.clone())),
            "duplicate prefix replay"
        );
        write!(observations, r#"{{"prefix":{key:?},"prefixChoices":{history:?},"answers":["#).unwrap();
        separator = "";
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
                        false,
                    )
                    .await
                    .0;
                    let (parked, label) = replay(
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
                        true,
                    )
                    .await;
                    let parked = label.map_or_else(
                        || "[]".into(),
                        |label| format!("[[{label},{}]]", parked == Outcome::Accepted),
                    );
                    write!(observations,
                        r#"{separator}[{},{parked}]"#,
                        actual == Outcome::Accepted
                    ).unwrap();
                    separator = ",";
                    if prefix_visits == 1 && key.len() < 4 && actual == Outcome::Accepted {
                        let mut next = key.clone();
                        next.push(2 * u16::from(message.event) + u16::from(receive));
                        let mut next_history = history.clone();
                        next_history.push(choice.to_vec());
                        let next = (next, next_history);
                        if !checked_prefixes.contains(&next) {
                            prefixes.entry(next).or_insert(commands);
                        }
                    }
                }
            }
        }
        writeln!(observations, "]}}").unwrap();
    }
    observations.flush().unwrap();
}
