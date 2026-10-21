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

struct Programs {
    p0: RoleProgram<0>,
    p1: RoleProgram<1>,
    p2: RoleProgram<2>,
    p3: RoleProgram<3>,
    third_actor: bool,
}
#[derive(Clone, Copy, Debug)]
struct Message {
    from: u8,
    to: u8,
    label: u8,
    owned: bool,
}
#[derive(Clone, Copy, Debug)]
struct Command {
    message: Message,
    receive: bool,
}
#[derive(Debug, PartialEq)]
enum Outcome {
    Accepted,
    Pending,
    Rejected,
    Unavailable,
}

macro_rules! observe {
    ($operation:expr) => {
        match tokio::time::timeout(core::time::Duration::from_millis(1), $operation).await {
            Ok(Ok(_)) => Outcome::Accepted,
            Ok(Err(_)) => Outcome::Rejected,
            Err(_) => Outcome::Pending,
        }
    };
}

async fn replay(
    programs: &Programs,
    resolver_count: u16,
    choice: u32,
    commands: &[Command],
) -> Outcome {
    let sid = SessionId::new(1);
    // SAFETY: a replay owns an isolated resource and its only initial issuer.
    let network = InMemoryTransport::new([((sid, 3), unsafe { EndpointBinding::new() })]);
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
                rv0.set_resolver(
                    &programs.p0,
                    ResolverRef::<$id>::decision_state(&choice, |c| {
                        Ok(if c & (1 << $id) == 0 {
                            DecisionArm::Left
                        } else {
                            DecisionArm::Right
                        })
                    }),
                )
                .unwrap();
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
    let mut owner_db = Some(rv0.enter(sid, &programs.p3).unwrap());
    let mut worker_db: Option<Endpoint<'_, 3>> = None;
    let mut outcome = Outcome::Accepted;
    for (index, command) in commands.iter().enumerate() {
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
                    (0, true, false) => observe!(e0.recv::<Unit>()),
                    (1, true, false) => observe!(e1.recv::<Unit>()),
                    (2, true, false) => observe!(e2.as_mut().unwrap().recv::<Unit>()),
                    (3, false, false) => match worker_db.as_mut() {
                        Some(db) => observe!(db.send::<Unit>(&())),
                        None => Outcome::Unavailable,
                    },
                    (3, true, false) => match worker_db.as_mut() {
                        Some(db) => observe!(db.recv::<Unit>()),
                        None => Outcome::Unavailable,
                    },
                    (0, false, true) => match owner_db.take() {
                        Some(db) => observe!(e0.send::<Loan<'_>>(db)),
                        None => Outcome::Unavailable,
                    },
                    (1, false, true) => match worker_db.take() {
                        Some(db) => observe!(e1.send::<Loan<'_>>(db)),
                        None => Outcome::Unavailable,
                    },
                    (0, true, true) => match tokio::time::timeout(
                        core::time::Duration::from_millis(1),
                        e0.recv::<Loan<'_>>(),
                    )
                    .await
                    {
                        Ok(Ok(db)) => {
                            assert!(owner_db.replace(db).is_none());
                            Outcome::Accepted
                        }
                        Ok(Err(_)) => Outcome::Rejected,
                        Err(_) => Outcome::Pending,
                    },
                    (1, true, true) => match tokio::time::timeout(
                        core::time::Duration::from_millis(1),
                        e1.recv::<Loan<'_>>(),
                    )
                    .await
                    {
                        Ok(Ok(db)) => {
                            assert!(worker_db.replace(db).is_none());
                            Outcome::Accepted
                        }
                        Ok(Err(_)) => Outcome::Rejected,
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

async fn qualify(
    programs: Programs,
    resolver_count: u16,
    case: usize,
    name: &str,
    messages: &[Message],
    initial: &[(u32, &[u8])],
    traces: &[(u32, &[u8])],
) {
    let mut observations = Vec::new();
    for (choice, first) in initial {
        for message in messages {
            for receive in [false, true] {
                let command = Command {
                    message: *message,
                    receive,
                };
                let actual = replay(&programs, resolver_count, *choice, &[command]).await;
                observations.push(format!(
                    r#"{{"choice":{choice},"label":{},"receive":{receive},"accepted":{}}}"#,
                    message.label,
                    actual == Outcome::Accepted
                ));
                let expected = !receive && first.contains(&message.label);

                assert_eq!(
                    actual == Outcome::Accepted,
                    expected,
                    "shape={name} choice={choice} initial={command:?} actual={actual:?}"
                );
            }
        }
    }
    for (choice, labels) in traces {
        let commands = labels
            .iter()
            .flat_map(|label| {
                let message = *messages
                    .iter()
                    .find(|message| message.label == *label)
                    .unwrap();
                [
                    Command {
                        message,
                        receive: false,
                    },
                    Command {
                        message,
                        receive: true,
                    },
                ]
            })
            .collect::<Vec<_>>();
        let actual = replay(&programs, resolver_count, *choice, &commands).await;
        assert_eq!(
            actual,
            Outcome::Accepted,
            "shape={name} choice={choice} full trace={labels:?}"
        );
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
