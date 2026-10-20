//! Enumerate atomic public-operation schedules and compare them with executable
//! Lean semantics. Scheduling data belongs only to this test, never to an app.
use super::*;
use crate::{
    g::ProgramShape,
    runtime::{SessionKitStorage, ids::SessionId},
    test_support::{
        lean_choreo::LeanChoreo, lean_descriptor::descriptor_source, transport::TestTransport,
    },
};
use std::{format, string::String, vec, vec::Vec};

mod phases;

type Start = g::Msg<221, u32>;
type Ack = g::Msg<222, u32>;
type Finish = g::Msg<223, u32>;
type Body = g::Seq<g::Send<0, 1, Start>, g::Send<1, 0, Ack>>;
type Sequential = g::Seq<g::Roll<Body>, g::Send<1, 0, Finish>>;
type Parallel = g::Par<g::Roll<Body>, g::Send<1, 0, Finish>>;
type Loan = g::Msg<224, Endpoint<'static, 10>>;
type Return = g::Msg<225, Endpoint<'static, 10>>;
type Read = g::Msg<226, u32>;
type Tick = g::Msg<227, u32>;
type Done = g::Msg<228, u32>;
type SequentialContinuation =
    g::Seq<g::Roll<Body>, g::Seq<g::Send<1, 0, Finish>, g::Send<0, 1, Done>>>;
type LoanBody = g::Seq<g::Send<0, 1, Loan>, g::Seq<g::Send<10, 2, Read>, g::Send<1, 0, Return>>>;
type OwnedParallel = g::Par<g::Roll<LoanBody>, g::Send<0, 2, Tick>>;

// (source operation = 2 * event + receive, physical caller). Alternate
// resource holders probe the same logical operation, not another protocol step.
const EXCHANGE_PROBES: &[(u8, u8)] = &[(0, 0), (1, 1), (2, 1), (3, 0), (4, 1), (5, 0)];
const CONTINUATION_PROBES: &[(u8, u8)] = &[
    (0, 0),
    (1, 1),
    (2, 1),
    (3, 0),
    (4, 1),
    (5, 0),
    (6, 0),
    (7, 1),
];
const OWNED_PROBES: &[(u8, u8)] = &[
    (0, 0),
    (1, 1),
    (2, 1),
    (3, 2),
    (4, 1),
    (5, 0),
    (6, 0),
    (7, 2),
    (2, 0),
];

#[derive(Clone, Debug, Eq, PartialEq)]
enum Outcome {
    Accepted,
    Pending,
    Rejected(String),
    Unavailable,
}

impl Outcome {
    fn code(&self) -> u8 {
        match self {
            Self::Accepted => 0,
            Self::Pending => 1,
            Self::Rejected(_) => 2,
            Self::Unavailable => 3,
        }
    }
}

// Drive each operation under Tokio, including internal wakeups. The virtual
// watchdog classifies quiescence without making wall-clock speed a protocol
// condition. Cancelling the final candidate cannot poison another replay.
macro_rules! observe {
    ($operation:expr) => {{
        match tokio::time::timeout(core::time::Duration::from_millis(1), $operation).await {
            Ok(Ok(_)) => Outcome::Accepted,
            Ok(Err(error)) => Outcome::Rejected(format!("{error:?}")),
            Err(_) => Outcome::Pending,
        }
    }};
}

async fn replay<Steps: ProgramShape>(commands: &[u8]) -> Outcome {
    let a_program = crate::runtime::program::project::<0>(&g::Program::<Steps>::new());
    let b_program = crate::runtime::program::project::<1>(&g::Program::<Steps>::new());
    let network = TestTransport::new();
    let mut a_slab = [0; 16 * 1024];
    let mut b_slab = [0; 16 * 1024];
    let mut a_storage = SessionKitStorage::uninit();
    let mut b_storage = SessionKitStorage::uninit();
    let a_rv = a_storage
        .init()
        .rendezvous(&mut a_slab, network.clone())
        .unwrap();
    let b_rv = b_storage.init().rendezvous(&mut b_slab, network).unwrap();
    let sid = SessionId::new(9074);
    let mut a = a_rv.enter(sid, &a_program).unwrap();
    let mut b = b_rv.enter(sid, &b_program).unwrap();
    let mut outcome = Outcome::Accepted;
    for (index, command) in commands.iter().enumerate() {
        outcome = match command {
            0 => observe!(a.send::<Start>(&7)),
            1 => observe!(b.recv::<Start>()),
            2 => observe!(b.send::<Ack>(&7)),
            3 => observe!(a.recv::<Ack>()),
            4 => observe!(b.send::<Finish>(&7)),
            5 => observe!(a.recv::<Finish>()),
            6 => observe!(a.send::<Done>(&7)),
            7 => observe!(b.recv::<Done>()),
            _ => unreachable!(),
        };
        if index + 1 != commands.len() {
            assert_eq!(
                outcome,
                Outcome::Accepted,
                "successful prefix changed: {commands:?}"
            );
        }
    }
    outcome
}

// Each Option is an actual affine Rust value, not a shadow owner table. The
// generator cannot invoke a method on a value already moved into a send.
async fn replay_owned(commands: &[u8]) -> Outcome {
    let owner_program = crate::runtime::program::project::<0>(&g::Program::<OwnedParallel>::new());
    let worker_program = crate::runtime::program::project::<1>(&g::Program::<OwnedParallel>::new());
    let service_program =
        crate::runtime::program::project::<2>(&g::Program::<OwnedParallel>::new());
    let db_program = crate::runtime::program::project::<10>(&g::Program::<OwnedParallel>::new());
    let network = TestTransport::new();
    let mut owner_slab = [0; 32 * 1024];
    let mut worker_slab = [0; 32 * 1024];
    let mut service_slab = [0; 32 * 1024];
    let mut owner_storage = SessionKitStorage::uninit();
    let mut worker_storage = SessionKitStorage::uninit();
    let mut service_storage = SessionKitStorage::uninit();
    let owner_rv = owner_storage
        .init()
        .rendezvous(&mut owner_slab, network.clone())
        .unwrap();
    let worker_rv = worker_storage
        .init()
        .rendezvous(&mut worker_slab, network.clone())
        .unwrap();
    let service_rv = service_storage
        .init()
        .rendezvous(&mut service_slab, network)
        .unwrap();
    let sid = SessionId::new(9075);
    let mut owner = owner_rv.enter(sid, &owner_program).unwrap();
    let mut owner_db = Some(owner_rv.enter(sid, &db_program).unwrap());
    let mut worker_db: Option<Endpoint<'_, 10>> = None;
    let mut worker = worker_rv.enter(sid, &worker_program).unwrap();
    let mut service = service_rv.enter(sid, &service_program).unwrap();
    let [mut loans, mut ticker] = owner.par().unwrap();
    let [mut reader, mut ticks] = service.par().unwrap();
    let mut outcome = Outcome::Accepted;
    for (index, command) in commands.iter().enumerate() {
        outcome = match command {
            0 => match owner_db.take() {
                Some(db) => observe!(loans.send::<Loan>(db)),
                None => Outcome::Unavailable,
            },
            1 => match tokio::time::timeout(
                core::time::Duration::from_millis(1),
                worker.recv::<Loan>(),
            )
            .await
            {
                Ok(Ok(db)) => {
                    assert!(
                        worker_db.replace(db).is_none(),
                        "duplicate live worker value"
                    );
                    Outcome::Accepted
                }
                Ok(Err(error)) => Outcome::Rejected(format!("{error:?}")),
                Err(_) => Outcome::Pending,
            },
            2 => match worker_db.as_mut() {
                Some(db) => observe!(db.send::<Read>(&7)),
                None => Outcome::Unavailable,
            },
            3 => observe!(reader.recv::<Read>()),
            4 => match worker_db.take() {
                Some(db) => observe!(worker.send::<Return>(db)),
                None => Outcome::Unavailable,
            },
            5 => match tokio::time::timeout(
                core::time::Duration::from_millis(1),
                loans.recv::<Return>(),
            )
            .await
            {
                Ok(Ok(db)) => {
                    assert!(owner_db.replace(db).is_none(), "duplicate live owner value");
                    Outcome::Accepted
                }
                Ok(Err(error)) => Outcome::Rejected(format!("{error:?}")),
                Err(_) => Outcome::Pending,
            },
            6 => observe!(ticker.send::<Tick>(&7)),
            7 => observe!(ticks.recv::<Tick>()),
            8 => match owner_db.as_mut() {
                Some(db) => observe!(db.send::<Read>(&7)),
                None => Outcome::Unavailable,
            },
            _ => unreachable!(),
        };
        assert!(
            owner_db.is_none() || worker_db.is_none(),
            "both actors retained a usable Rust value: {commands:?}"
        );
        if index + 1 != commands.len() {
            assert_eq!(
                outcome,
                Outcome::Accepted,
                "successful owned prefix changed: {commands:?}"
            );
        }
    }
    outcome
}

async fn enumerate(
    name: &str,
    caps: &[u8],
    probes: &[(u8, u8)],
    replay: impl AsyncFn(&[u8]) -> Outcome,
) -> (String, usize, usize) {
    use core::fmt::Write;
    let mut frontier = vec![Vec::<u8>::new()];
    let mut observations = String::new();
    let mut attempts = 0_usize;
    let mut completed = 0;
    observations.push('[');
    // Test bounds do not add runtime admission rules. Lean independently
    // derives these bounds and requires every remaining candidate probe.
    let depth = caps.iter().map(|&cap| usize::from(cap)).sum();
    for _depth in 0..depth {
        let mut next = Vec::new();
        for prefix in frontier {
            for (command, &(operation, _actor)) in probes.iter().enumerate() {
                let command = u8::try_from(command).unwrap();
                let cap = caps[usize::from(operation)];
                if prefix
                    .iter()
                    .filter(|&&prior| probes[usize::from(prior)].0 == operation)
                    .count()
                    == usize::from(cap)
                {
                    continue;
                }
                let mut trace = prefix.clone();
                trace.push(command);
                let outcome = replay(&trace).await;
                let detail = format!("{outcome:?}");
                if attempts != 0 {
                    observations.push(',');
                }
                writeln!(
                    observations,
                    "{{\"commands\":{trace:?},\"outcome\":{},\"runtime\":{}}}",
                    outcome.code(),
                    json_string(&detail),
                )
                .unwrap();
                attempts += 1;
                if outcome == Outcome::Accepted {
                    if trace.len() == depth {
                        completed += 1;
                    }
                    next.push(trace);
                }
            }
        }
        frontier = next;
    }
    assert!(
        completed > 0,
        "the generator must reach complete bounded runs: {name}"
    );
    observations.push(']');
    (observations, attempts, completed)
}

fn json_string(value: &str) -> String {
    use core::fmt::Write;
    let mut encoded = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\u{0}'..='\u{1f}' => write!(encoded, "\\u{:04x}", u32::from(character)).unwrap(),
            _ => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

fn probe_source(probes: &[(u8, u8)]) -> String {
    use core::fmt::Write;
    let mut output = String::from("[");
    for &(operation, actor) in probes {
        writeln!(output, "{{ operation := {operation}, actor := {actor} }},").unwrap();
    }
    output.push(']');
    output
}

#[test]
#[cfg_attr(
    miri,
    ignore = "native exhaustive schedules; actual holder replays have a separate Miri test"
)]
fn bounded_public_schedules_emit_executable_lean_conformance_cases() {
    let (sequential, parallel, owned, continuation) = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            (
                enumerate(
                    "seqRoll",
                    &[2, 2, 2, 2, 1, 1],
                    EXCHANGE_PROBES,
                    replay::<Sequential>,
                )
                .await,
                enumerate(
                    "parRoll",
                    &[2, 2, 2, 2, 1, 1],
                    EXCHANGE_PROBES,
                    replay::<Parallel>,
                )
                .await,
                enumerate(
                    "ownedParRoll",
                    &[2, 2, 2, 2, 2, 2, 1, 1],
                    OWNED_PROBES,
                    replay_owned,
                )
                .await,
                enumerate(
                    "seqRollContinuation",
                    &[2, 2, 2, 2, 1, 1, 1, 1],
                    CONTINUATION_PROBES,
                    replay::<SequentialContinuation>,
                )
                .await,
            )
        });
    let inputs = format!(
        "def conformanceInputs : List ConformanceCase := [\n\
        {{ name := \"seq-roll\", choreo := {}, images := [{}, {}], probes := {}, attempts := [] }},\n\
        {{ name := \"par-roll\", choreo := {}, images := [{}, {}], probes := {}, attempts := [] }},\n\
        {{ name := \"owned-par-roll\", choreo := {}, images := [{}, {}, {}], probes := {}, attempts := [] }},\n\
        {{ name := \"seq-roll-continuation\", choreo := {}, images := [{}, {}], probes := {}, attempts := [] }}]\n",
        Sequential::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(
            crate::runtime::program::project::<0>(&g::Program::<Sequential>::new())
                .role_image_ref()
        )),
        descriptor_source(RoleDescriptorRef::from_resident(
            crate::runtime::program::project::<1>(&g::Program::<Sequential>::new())
                .role_image_ref()
        )),
        probe_source(EXCHANGE_PROBES),
        Parallel::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(
            crate::runtime::program::project::<0>(&g::Program::<Parallel>::new()).role_image_ref()
        )),
        descriptor_source(RoleDescriptorRef::from_resident(
            crate::runtime::program::project::<1>(&g::Program::<Parallel>::new()).role_image_ref()
        )),
        probe_source(EXCHANGE_PROBES),
        OwnedParallel::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(
            crate::runtime::program::project::<0>(&g::Program::<OwnedParallel>::new())
                .role_image_ref()
        )),
        descriptor_source(RoleDescriptorRef::from_resident(
            crate::runtime::program::project::<1>(&g::Program::<OwnedParallel>::new())
                .role_image_ref()
        )),
        descriptor_source(RoleDescriptorRef::from_resident(
            crate::runtime::program::project::<2>(&g::Program::<OwnedParallel>::new())
                .role_image_ref()
        )),
        probe_source(OWNED_PROBES),
        SequentialContinuation::lean_source(),
        descriptor_source(RoleDescriptorRef::from_resident(
            crate::runtime::program::project::<0>(&g::Program::<SequentialContinuation>::new())
                .role_image_ref()
        )),
        descriptor_source(RoleDescriptorRef::from_resident(
            crate::runtime::program::project::<1>(&g::Program::<SequentialContinuation>::new())
                .role_image_ref()
        )),
        probe_source(CONTINUATION_PROBES),
    );
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/lean-proof");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("EndpointConformanceInputs.lean"), inputs).unwrap();
    let observations = format!(
        "[{{\"name\":\"seq-roll\",\"attempts\":{}}},\
        {{\"name\":\"par-roll\",\"attempts\":{}}},\
        {{\"name\":\"owned-par-roll\",\"attempts\":{}}},\
        {{\"name\":\"seq-roll-continuation\",\"attempts\":{}}}]\n",
        sequential.0, parallel.0, owned.0, continuation.0,
    );
    std::fs::write(
        directory.join("EndpointConformanceCases.json"),
        observations,
    )
    .unwrap();
    std::println!(
        "bounded Endpoint schedules: seq attempts={} complete={}, par attempts={} complete={}, owned attempts={} complete={}, continuation attempts={} complete={}",
        sequential.1,
        sequential.2,
        parallel.1,
        parallel.2,
        owned.1,
        owned.2,
        continuation.1,
        continuation.2,
    );
}

#[test]
fn actual_holder_values_survive_two_visits_and_refuse_another_callers_operation() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            assert!(matches!(replay_owned(&[8]).await, Outcome::Rejected(_)));
            assert_eq!(replay_owned(&[0, 1, 8]).await, Outcome::Unavailable);
            assert_eq!(
                replay_owned(&[0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5, 6, 7]).await,
                Outcome::Accepted
            );
        });
}

#[test]
fn multiple_continuation_steps_keep_their_local_order_after_reentry() {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            // The old Finish can be received before the new Ack. The outgoing
            // Done must still follow this role's new body, even after Finish.
            assert!(matches!(
                replay::<SequentialContinuation>(&[0, 1, 2, 3, 4, 0, 1, 2, 5, 6]).await,
                Outcome::Rejected(_)
            ));
            assert_eq!(
                replay::<SequentialContinuation>(&[0, 1, 2, 3, 4, 0, 1, 2, 5, 3, 6, 7]).await,
                Outcome::Accepted
            );
        });
}
